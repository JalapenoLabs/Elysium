// Copyright © 2026 Jalapeno Labs

//! The watcher end to end: items linked through real credentials in Postgres, providers
//! faked with state, and whole passes run the way the running watcher runs them. Each test
//! is one rule of `docs/action-items.md`, "An action item is a commitment, not a copy".

use chrono::TimeDelta;

use super::*;
use crate::action_items::links::tests::{
    DONE, FakeJiraIssue, JIRA_ACCOUNT_ID, LinkFixture, REPOSITORY,
};
use crate::github::fake::{FakeIssue, LOGIN};
use crate::models::action_item::{ActionItemChanges, Owner};
use crate::models::action_item_event::list_for_item;
use crate::models::action_item_link::LinkProvider;
use crate::models::initiative::NewInitiative;
use crate::models::initiative_link::{ContainerKind, NewContainer};
use crate::models::project::{self, NewProject};

/// An item created from `reference` on GitHub by the user, with that link as its primary.
async fn item_from_github(fixture: &mut LinkFixture, reference: &str, kind: LinkKind) -> Uuid {
    let remote = fixture
        .links
        .provider(LinkProvider::Github)
        .find(fixture.github_credential.id, kind, reference)
        .await
        .expect("found");
    let new_item = NewActionItem {
        title: remote.title.clone(),
        notes: String::new(),
        state: ActionItemState::Open,
        priority: remote.starting_priority(),
        due_at: None,
        owner: remote.owner.clone(),
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    };
    let new_link = NewLink {
        credential: fixture.github_credential,
        kind,
        external_id: remote.external_id.clone(),
        observation: remote.observation(),
    };
    action_item_link::create_linked_item(
        &mut fixture.connection,
        new_item,
        new_link,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("created")
    .item
    .record
    .id
}

/// Links `reference` to an existing item, as `actor`, the way the route or the agent's tool
/// does.
async fn link_github(
    fixture: &mut LinkFixture,
    item_id: Uuid,
    reference: &str,
    kind: LinkKind,
    actor: Actor,
) {
    let remote = fixture
        .links
        .provider(LinkProvider::Github)
        .find(fixture.github_credential.id, kind, reference)
        .await
        .expect("found");
    let new_link = NewLink {
        credential: fixture.github_credential,
        kind,
        external_id: remote.external_id.clone(),
        observation: remote.observation(),
    };
    action_item_link::add(
        &mut fixture.connection,
        item_id,
        new_link,
        actor == Actor::User,
        actor,
        Utc::now(),
    )
    .await
    .expect("linked");
}

async fn item(fixture: &mut LinkFixture, id: Uuid) -> ActionItem {
    action_item::find(&mut fixture.connection, id)
        .await
        .expect("the item")
}

async fn move_item(fixture: &mut LinkFixture, id: Uuid, transition: Transition) {
    action_item::transition(
        &mut fixture.connection,
        id,
        transition,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("moved");
}

async fn owed(fixture: &mut LinkFixture, item_id: Uuid) -> Vec<LinkWrite> {
    let links = action_item_link::list_for_item(&mut fixture.connection, item_id)
        .await
        .expect("links");
    let ids: Vec<Uuid> = links.iter().map(|link| link.id).collect();
    action_item_link_write::for_links(&mut fixture.connection, &ids)
        .await
        .expect("writes")
}

/// The initiative's live members, in any state but dismissed.
async fn members(fixture: &mut LinkFixture, initiative_id: Uuid) -> Vec<ActionItem> {
    let filter = action_item::ActionItemFilter {
        initiative: Some(initiative_id),
        states: vec![
            ActionItemState::Inbox,
            ActionItemState::Open,
            ActionItemState::Resolved,
        ],
        ..action_item::ActionItemFilter::default()
    };
    action_item::list(&mut fixture.connection, &filter, Utc::now())
        .await
        .expect("members")
}

/// Every entry of the item's history as `kind by actor`.
async fn history(fixture: &mut LinkFixture, item_id: Uuid) -> Vec<String> {
    list_for_item(&mut fixture.connection, item_id)
        .await
        .expect("history")
        .into_iter()
        .map(|entry| format!("{} by {}", entry.kind, entry.actor))
        .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn resolving_moves_every_open_linked_issue_once_and_never_a_pull_request() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));
    fixture.github.put_issue(
        REPOSITORY,
        13,
        FakeIssue {
            is_open: false,
            state_reason: Some("completed".to_owned()),
            ..FakeIssue::open("Already done")
        },
    );
    fixture
        .github
        .put_issue(REPOSITORY, 15, FakeIssue::pull_request("The fix"));
    let id = item_from_github(&mut fixture, "JalapenoLabs/Elysium#12", LinkKind::Issue).await;
    link_github(
        &mut fixture,
        id,
        "JalapenoLabs/Elysium#13",
        LinkKind::Issue,
        Actor::User,
    )
    .await;
    link_github(
        &mut fixture,
        id,
        "JalapenoLabs/Elysium#15",
        LinkKind::PullRequest,
        Actor::User,
    )
    .await;

    move_item(&mut fixture, id, Transition::Resolve).await;
    let writes = owed(&mut fixture, id).await;
    assert_eq!(
        writes.len(),
        1,
        "only the open issue owes a close: not the done one, never the pull request"
    );
    assert_eq!(writes[0].kind, LinkWriteKind::Close);

    let context = fixture.context();
    pass(&context).await;
    let closed = fixture.github.issue(REPOSITORY, 12);
    assert!(!closed.is_open);
    assert_eq!(closed.state_reason.as_deref(), Some("completed"));
    assert!(
        fixture.github.issue(REPOSITORY, 15).is_open,
        "a pull request is never closed"
    );
    assert!(owed(&mut fixture, id).await.is_empty());

    pass(&context).await;
    assert_eq!(
        fixture.github.writes().len(),
        1,
        "a close Elysium made is recorded, so no pass sends it again"
    );
    assert_eq!(
        item(&mut fixture, id).await.state,
        ActionItemState::Resolved
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_failed_write_stays_pending_every_pass_until_it_lands_or_is_cancelled() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));
    fixture
        .github
        .put_issue(REPOSITORY, 13, FakeIssue::open("Another"));
    let landing = item_from_github(&mut fixture, "JalapenoLabs/Elysium#12", LinkKind::Issue).await;
    let cancelled =
        item_from_github(&mut fixture, "JalapenoLabs/Elysium#13", LinkKind::Issue).await;
    let context = fixture.context();

    fixture.github.refuse_writes(true);
    move_item(&mut fixture, landing, Transition::Resolve).await;
    move_item(&mut fixture, cancelled, Transition::Resolve).await;
    pass(&context).await;
    pass(&context).await;

    let pending = owed(&mut fixture, landing).await;
    assert_eq!(
        pending.len(),
        1,
        "the item's own change stands and the close waits"
    );
    assert_eq!(pending[0].attempts, 2, "tried again on every pass");
    let error = pending[0].last_error.as_deref().unwrap_or_default();
    assert!(
        error.contains("not accessible"),
        "the provider's own answer: {error}"
    );
    assert_eq!(
        item(&mut fixture, landing).await.state,
        ActionItemState::Resolved
    );

    let write = owed(&mut fixture, cancelled).await.remove(0);
    action_item_link_write::cancel(
        &mut fixture.connection,
        cancelled,
        write.link_id,
        write.id,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("cancelled");
    assert!(
        history(&mut fixture, cancelled)
            .await
            .contains(&"link_write_cancelled by user".to_owned())
    );

    fixture.github.refuse_writes(false);
    pass(&context).await;
    assert!(owed(&mut fixture, landing).await.is_empty());
    assert!(
        !fixture.github.issue(REPOSITORY, 12).is_open,
        "it lands once allowed"
    );
    assert!(
        fixture.github.issue(REPOSITORY, 13).is_open,
        "a cancelled write is never sent"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_item_reopened_before_its_close_lands_keeps_its_issue_open() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));
    let id = item_from_github(&mut fixture, "JalapenoLabs/Elysium#12", LinkKind::Issue).await;
    let context = fixture.context();

    fixture.github.refuse_writes(true);
    move_item(&mut fixture, id, Transition::Resolve).await;
    pass(&context).await;
    move_item(&mut fixture, id, Transition::Reopen).await;
    fixture.github.refuse_writes(false);
    pass(&context).await;

    assert!(fixture.github.issue(REPOSITORY, 12).is_open);
    assert!(
        owed(&mut fixture, id).await.is_empty(),
        "the close no longer applies"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_comment_is_posted_to_the_primary_link_only() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));
    fixture
        .github
        .put_issue(REPOSITORY, 15, FakeIssue::pull_request("The fix"));
    let id = item_from_github(&mut fixture, "JalapenoLabs/Elysium#12", LinkKind::Issue).await;
    link_github(
        &mut fixture,
        id,
        "JalapenoLabs/Elysium#15",
        LinkKind::PullRequest,
        Actor::Session(4),
    )
    .await;

    action_item_comment::create(
        &mut fixture.connection,
        id,
        "Asked Sam for the logs.".to_owned(),
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("commented");
    pass(&fixture.context()).await;

    let writes = fixture.github.writes();
    assert_eq!(writes.len(), 1);
    assert!(
        writes[0].path.ends_with("/issues/12/comments"),
        "{}",
        writes[0].path
    );
    assert_eq!(writes[0].body["body"], "Asked Sam for the logs.");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_providers_changes_move_the_item_by_the_rules() {
    let mut fixture = LinkFixture::start().await;
    for number in [12, 20, 30] {
        fixture
            .github
            .put_issue(REPOSITORY, number, FakeIssue::open("Some work"));
    }
    let resolved = item_from_github(&mut fixture, "JalapenoLabs/Elysium#12", LinkKind::Issue).await;
    let dismissed =
        item_from_github(&mut fixture, "JalapenoLabs/Elysium#20", LinkKind::Issue).await;
    let reopened_by_hand =
        item_from_github(&mut fixture, "JalapenoLabs/Elysium#30", LinkKind::Issue).await;
    let context = fixture.context();

    for number in [12, 30] {
        fixture.github.change_issue(REPOSITORY, number, |issue| {
            issue.is_open = false;
            issue.state_reason = Some("completed".to_owned());
        });
    }
    fixture.github.change_issue(REPOSITORY, 20, |issue| {
        issue.is_open = false;
        issue.state_reason = Some("not_planned".to_owned());
    });
    pass(&context).await;
    assert_eq!(
        item(&mut fixture, resolved).await.state,
        ActionItemState::Resolved
    );
    assert_eq!(
        item(&mut fixture, dismissed).await.state,
        ActionItemState::Dismissed
    );
    assert!(
        history(&mut fixture, resolved)
            .await
            .contains(&"state_changed by watcher:github".to_owned()),
        "the watcher is the actor"
    );

    // The user reopens an item the watcher resolved, and its issue stays closed: nothing
    // moves it back, since the issue did not change again.
    assert_eq!(
        item(&mut fixture, reopened_by_hand).await.state,
        ActionItemState::Resolved
    );
    move_item(&mut fixture, reopened_by_hand, Transition::Reopen).await;
    pass(&context).await;
    pass(&context).await;
    assert_eq!(
        item(&mut fixture, reopened_by_hand).await.state,
        ActionItemState::Open
    );

    // Reopened on GitHub: a resolved item returns to open, a dismissed one stays dismissed.
    for number in [12, 20] {
        fixture.github.change_issue(REPOSITORY, number, |issue| {
            issue.is_open = true;
            issue.state_reason = Some("reopened".to_owned());
        });
    }
    pass(&context).await;
    assert_eq!(
        item(&mut fixture, resolved).await.state,
        ActionItemState::Open
    );
    assert_eq!(
        item(&mut fixture, dismissed).await.state,
        ActionItemState::Dismissed
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_merged_pull_request_resolves_its_item_and_one_closed_unmerged_is_only_recorded() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));
    fixture
        .github
        .put_issue(REPOSITORY, 15, FakeIssue::pull_request("The fix"));
    fixture
        .github
        .put_issue(REPOSITORY, 16, FakeIssue::pull_request("A dead end"));
    let fixed = item_from_github(&mut fixture, "JalapenoLabs/Elysium#12", LinkKind::Issue).await;
    link_github(
        &mut fixture,
        fixed,
        "JalapenoLabs/Elysium#15",
        LinkKind::PullRequest,
        Actor::Session(4),
    )
    .await;
    let abandoned = item_from_github(
        &mut fixture,
        "JalapenoLabs/Elysium#16",
        LinkKind::PullRequest,
    )
    .await;
    let context = fixture.context();

    fixture.github.change_issue(REPOSITORY, 15, |pull_request| {
        pull_request.is_open = false;
        pull_request.pull_request = Some(true);
    });
    fixture.github.change_issue(REPOSITORY, 16, |pull_request| {
        pull_request.is_open = false;
    });
    pass(&context).await;
    assert_eq!(
        item(&mut fixture, fixed).await.state,
        ActionItemState::Resolved
    );
    assert_eq!(
        item(&mut fixture, abandoned).await.state,
        ActionItemState::Open
    );
    assert!(
        history(&mut fixture, abandoned)
            .await
            .contains(&"pull_request_closed by watcher:github".to_owned())
    );

    // The merge resolved the item, and the item's resolve moves the issue it fixes.
    pass(&context).await;
    assert!(!fixture.github.issue(REPOSITORY, 12).is_open);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_owner_follows_the_primary_links_assignee_when_it_changes() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));
    fixture
        .github
        .put_issue(REPOSITORY, 15, FakeIssue::pull_request("The fix"));
    let id = item_from_github(&mut fixture, "JalapenoLabs/Elysium#12", LinkKind::Issue).await;
    link_github(
        &mut fixture,
        id,
        "JalapenoLabs/Elysium#15",
        LinkKind::PullRequest,
        Actor::Session(4),
    )
    .await;
    assert_eq!(item(&mut fixture, id).await.owner(), Owner::Nobody);
    let context = fixture.context();

    fixture.github.change_issue(REPOSITORY, 12, |issue| {
        issue.assignee = Some(LOGIN.to_owned());
    });
    pass(&context).await;
    assert_eq!(
        item(&mut fixture, id).await.owner(),
        Owner::User,
        "assigned to the token's own account: the user's"
    );

    let pat = Owner::Other {
        name: "Pat".to_owned(),
    };
    let by_hand = ActionItemChanges {
        owner: Some(pat.clone()),
        ..ActionItemChanges::default()
    };
    action_item::update(
        &mut fixture.connection,
        id,
        by_hand,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("owned by hand");
    fixture.github.change_issue(REPOSITORY, 15, |pull_request| {
        pull_request.assignee = Some("sam".to_owned());
    });
    pass(&context).await;
    assert_eq!(
        item(&mut fixture, id).await.owner(),
        pat,
        "the primary's assignee did not change, and the pull request is not primary"
    );

    fixture.github.change_issue(REPOSITORY, 12, |issue| {
        issue.assignee = Some("sam".to_owned());
    });
    pass(&context).await;
    assert_eq!(
        item(&mut fixture, id).await.owner(),
        Owner::Other {
            name: "sam".to_owned()
        }
    );
}

/// An initiative in a project, linked to milestone 3 of [`REPOSITORY`]. Answers the project
/// and the initiative.
async fn initiative_on_a_milestone(fixture: &mut LinkFixture) -> (Uuid, Uuid) {
    let project_id = project::create(
        &mut fixture.connection,
        &NewProject {
            name: "Elysium".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("project")
    .id;
    let initiative_id = crate::models::initiative::create(
        &mut fixture.connection,
        NewInitiative {
            name: "Links".to_owned(),
            description: String::new(),
            target_at: None,
            project_ids: vec![project_id],
        },
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("initiative")
    .record
    .id;

    fixture.github.put_milestone(REPOSITORY, 3, "Links");
    let container = fixture
        .links
        .provider(LinkProvider::Github)
        .find_container(
            fixture.github_credential.id,
            ContainerKind::Milestone,
            "JalapenoLabs/Elysium#3",
        )
        .await
        .expect("a milestone");
    initiative_link::add(
        &mut fixture.connection,
        initiative_id,
        NewContainer {
            credential: fixture.github_credential,
            kind: ContainerKind::Milestone,
            external_id: container.external_id,
            external_key: container.key,
            url: container.url,
            title: container.title,
        },
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("linked");
    (project_id, initiative_id)
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_containers_children_become_open_items_owned_as_reported() {
    let mut fixture = LinkFixture::start().await;
    let (project_id, initiative_id) = initiative_on_a_milestone(&mut fixture).await;
    fixture.github.put_issue(
        REPOSITORY,
        1,
        FakeIssue {
            milestone: Some(3),
            assignee: Some(LOGIN.to_owned()),
            ..FakeIssue::open("Open child")
        },
    );
    fixture.github.put_issue(
        REPOSITORY,
        2,
        FakeIssue {
            milestone: Some(3),
            is_open: false,
            state_reason: Some("completed".to_owned()),
            assignee: Some("sam".to_owned()),
            ..FakeIssue::open("Done child")
        },
    );
    pass(&fixture.context()).await;

    let children = members(&mut fixture, initiative_id).await;
    assert_eq!(children.len(), 2);
    let open_child = children
        .iter()
        .find(|child| child.title == "Open child")
        .expect("the open child");
    assert_eq!(
        open_child.state,
        ActionItemState::Open,
        "linking the container accepted it"
    );
    assert_eq!(open_child.owner(), Owner::User);
    let done_child = children
        .iter()
        .find(|child| child.title == "Done child")
        .expect("the done child");
    assert_eq!(done_child.state, ActionItemState::Resolved);
    assert_eq!(
        done_child.owner(),
        Owner::Other {
            name: "sam".to_owned()
        },
        "owned as the provider reports; someone else's child stays out of Next"
    );
    let memberships = action_item::memberships(&mut fixture.connection, &[open_child.id])
        .await
        .expect("memberships");
    assert_eq!(memberships.project_ids(open_child.id), [project_id]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_child_leaves_with_the_container_and_an_item_already_tracked_joins_once() {
    let mut fixture = LinkFixture::start().await;
    let (_project_id, initiative_id) = initiative_on_a_milestone(&mut fixture).await;
    fixture.github.put_issue(
        REPOSITORY,
        1,
        FakeIssue {
            milestone: Some(3),
            ..FakeIssue::open("Open child")
        },
    );
    fixture
        .github
        .put_issue(REPOSITORY, 3, FakeIssue::open("Tracked already"));
    let tracked = item_from_github(&mut fixture, "JalapenoLabs/Elysium#3", LinkKind::Issue).await;
    let context = fixture.context();
    pass(&context).await;

    fixture
        .github
        .change_issue(REPOSITORY, 3, |issue| issue.milestone = Some(3));
    fixture
        .github
        .change_issue(REPOSITORY, 1, |issue| issue.milestone = None);
    pass(&context).await;
    let titles: Vec<String> = members(&mut fixture, initiative_id)
        .await
        .into_iter()
        .map(|member| member.title)
        .collect();
    assert_eq!(
        titles,
        ["Tracked already"],
        "the item already tracked joins, and the child removed from the milestone leaves"
    );
    let linked_to_three = action_item_link::find_by_external(
        &mut fixture.connection,
        fixture.github_credential,
        LinkKind::Issue,
        "JalapenoLabs/Elysium#3",
    )
    .await
    .expect("read")
    .expect("linked");
    assert_eq!(linked_to_three.action_item_id, tracked);

    let container_id = initiative_link::list_for_initiative(&mut fixture.connection, initiative_id)
        .await
        .expect("containers")[0]
        .id;
    initiative_link::remove(
        &mut fixture.connection,
        initiative_id,
        container_id,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("unlinked");
    assert!(
        members(&mut fixture, initiative_id).await.is_empty(),
        "unlinking takes out every item the container brought in"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_cursor_survives_a_restart() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));
    item_from_github(&mut fixture, "JalapenoLabs/Elysium#12", LinkKind::Issue).await;

    let before = Utc::now();
    pass(&fixture.context()).await;
    let cursor = link_watch_cursor::find(&mut fixture.connection, fixture.github_credential)
        .await
        .expect("read")
        .expect("a cursor after a pass");
    assert!(cursor.watched_through >= before);
    assert_eq!(cursor.last_error, None);

    // A watcher started afresh, as after a restart, reads from the stored cursor.
    let restarted = WatchContext {
        links: Links::new(
            fixture.database.clone(),
            std::sync::Arc::new(crate::test_support::cipher()),
            &fixture.jira.jira,
            &fixture.github.github,
        ),
        ..fixture.context()
    };
    pass(&restarted).await;
    let listing = fixture
        .github
        .received()
        .into_iter()
        .rev()
        .find(|request| request.path.ends_with("/issues"))
        .expect("a listing");
    let since = url::form_urlencoded::parse(listing.query.as_bytes())
        .find(|(key, _value)| key == "since")
        .map(|(_key, value)| value.into_owned())
        .expect("since the cursor");
    let since = DateTime::parse_from_rfc3339(&since)
        .expect("a timestamp")
        .with_timezone(&Utc);
    assert!(
        since <= cursor.watched_through && since >= cursor.watched_through - TimeDelta::minutes(10),
        "{since} is the cursor, less a few minutes of overlap"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_jira_issue_done_in_jira_resolves_its_item_and_a_resolve_moves_it() {
    let mut fixture = LinkFixture::start().await;
    fixture.jira.put_issue(FakeJiraIssue {
        assignee: Some((JIRA_ACCOUNT_ID.to_owned(), "Alex Navarro".to_owned())),
        ..FakeJiraIssue::new("10042", "ELY-12")
    });
    fixture
        .jira
        .put_issue(FakeJiraIssue::new("10043", "ELY-13"));
    let mut create = async |key: &str| {
        let remote = fixture
            .links
            .provider(LinkProvider::Jira)
            .find(fixture.jira_credential.id, LinkKind::Issue, key)
            .await
            .expect("found");
        let new_item = NewActionItem {
            title: remote.title.clone(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: remote.starting_priority(),
            due_at: remote.starting_due_at(),
            owner: remote.owner.clone(),
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        };
        let new_link = NewLink {
            credential: fixture.jira_credential,
            kind: LinkKind::Issue,
            external_id: remote.external_id.clone(),
            observation: remote.observation(),
        };
        action_item_link::create_linked_item(
            &mut fixture.connection,
            new_item,
            new_link,
            Actor::User,
            Utc::now(),
        )
        .await
        .expect("created")
        .item
        .record
        .id
    };
    let done_in_jira = create("ELY-12").await;
    let resolved_here = create("ELY-13").await;
    let context = fixture.context();

    fixture
        .jira
        .change_issue("ELY-12", |issue| issue.status_id = DONE.to_owned());
    move_item(&mut fixture, resolved_here, Transition::Resolve).await;
    pass(&context).await;

    assert_eq!(
        item(&mut fixture, done_in_jira).await.state,
        ActionItemState::Resolved
    );
    assert!(
        history(&mut fixture, done_in_jira)
            .await
            .contains(&"state_changed by watcher:jira".to_owned())
    );
    assert_eq!(
        fixture.jira.issue("ELY-13").status_id,
        DONE,
        "the project's only done transition"
    );
}
