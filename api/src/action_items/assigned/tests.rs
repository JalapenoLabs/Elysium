// Copyright © 2026 Jalapeno Labs

//! Assigned issues coming in, end to end: Jira, GitHub, and YouTrack faked with state, their
//! connectors and accounts in Postgres, and whole watcher passes run the way the running watcher
//! runs them. Each test is one rule of `docs/action-items.md`, Assigned issues.

use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde_json::json;

use super::*;
use crate::action_items::Transition;
use crate::action_items::links::tests::{
    DONE, FakeJiraIssue, JIRA_ACCOUNT_ID, LinkFixture, REPOSITORY,
};
use crate::action_items::watcher::pass;
use crate::database::schema::{action_items, assigned_issues};
use crate::github::fake::{FakeIssue, LOGIN};
use crate::models::action_item::{self, ActionItem, ActionItemPriority};
use crate::models::action_item_event::list_for_item;
use crate::models::action_item_link::ActionItemLink;
use crate::models::connector::{self, AccountRef, NewConnector};
use crate::models::link_watch_cursor;
use crate::test_support::{TEST_PERSON, TEST_PERSON_ID};
use crate::youtrack::tests::{FakeIssue as FakeYoutrackIssue, sample_issue};

/// Someone other than the person every connector reads as.
const SAM: (&str, &str) = ("62a1", "Sam");

/// An issue in `key`'s project, to do, assigned to the person the Jira credential reads as.
fn jira_issue_of_mine(id: &str, key: &str) -> FakeJiraIssue {
    FakeJiraIssue {
        assignee: Some((JIRA_ACCOUNT_ID.to_owned(), "Alex Navarro".to_owned())),
        ..FakeJiraIssue::new(id, key)
    }
}

/// An open GitHub issue assigned to the token's account.
fn github_issue_of_mine(title: &str) -> FakeIssue {
    FakeIssue {
        assignee: Some(LOGIN.to_owned()),
        ..FakeIssue::open(title)
    }
}

/// A YouTrack connector on the fixture's YouTrack account, reading `projects`. The fixture
/// holds the account alone, so the other tests' passes bring nothing in from YouTrack.
async fn youtrack_connector(fixture: &mut LinkFixture, projects: serde_json::Value) {
    let mut new_connector = NewConnector::new(
        ConnectorKind::Youtrack.as_str(),
        AccountRef::Token(fixture.youtrack_account.id),
        TEST_PERSON_ID,
    );
    new_connector.content = json!({ "projects": projects });
    connector::create(&mut fixture.connection, new_connector)
        .await
        .expect("a YouTrack connector");
}

/// The GitHub connector the fixture's token carries, reading `repositories`.
async fn choose_repositories(fixture: &mut LinkFixture, repositories: serde_json::Value) {
    let github = connector::find_by_account(
        &mut fixture.connection,
        AccountRef::Github(fixture.github_credential.id),
    )
    .await
    .expect("read")
    .expect("the token carries its connector");
    connector::set_content(
        &mut fixture.connection,
        github.id,
        json!({ "repositories": repositories }),
        None,
    )
    .await
    .expect("chosen");
}

/// The links of live items made through `credential`, oldest first.
async fn links_through(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
) -> Vec<ActionItemLink> {
    let mut links = action_item_link::watched(connection, credential)
        .await
        .expect("links");
    links.sort_by_key(|link| link.created_at);
    links
}

/// How many items exist at all, deleted ones included.
async fn item_count(fixture: &mut LinkFixture) -> i64 {
    action_items::table
        .count()
        .get_result(&mut fixture.connection)
        .await
        .expect("counted")
}

async fn item(fixture: &mut LinkFixture, id: Uuid) -> ActionItem {
    action_item::find(&mut fixture.connection, id)
        .await
        .expect("the item")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn everything_assigned_comes_in_once_as_an_inbox_item_linked_to_its_issue() {
    let mut fixture = LinkFixture::start().await;
    // Jira: its account's allowlist is ELY and OPS.
    fixture.jira.put_issue(FakeJiraIssue {
        priority: Some("High".to_owned()),
        ..jira_issue_of_mine("10001", "ELY-1")
    });
    fixture.jira.put_issue(FakeJiraIssue {
        status_id: DONE.to_owned(),
        ..jira_issue_of_mine("10002", "ELY-2")
    });
    fixture.jira.put_issue(FakeJiraIssue {
        assignee: Some((SAM.0.to_owned(), SAM.1.to_owned())),
        ..FakeJiraIssue::new("10003", "ELY-3")
    });
    fixture
        .jira
        .put_issue(jira_issue_of_mine("10099", "SECRET-1"));
    // GitHub: the connector reads REPOSITORY alone.
    choose_repositories(&mut fixture, json!([REPOSITORY])).await;
    fixture
        .github
        .put_issue(REPOSITORY, 1, github_issue_of_mine("Fix sign-in"));
    fixture.github.put_issue(
        REPOSITORY,
        2,
        FakeIssue {
            assignee: Some(LOGIN.to_owned()),
            ..FakeIssue::pull_request("The fix")
        },
    );
    fixture
        .github
        .put_issue("acme/other", 3, github_issue_of_mine("Elsewhere"));
    // YouTrack: the connector reads JL alone.
    youtrack_connector(&mut fixture, json!(["0-1"])).await;
    fixture.put_youtrack_issue(sample_issue("2-1", "JL-1", 1_000_000));
    fixture.put_youtrack_issue(sample_issue("2-2", "WAY-1", 1_000_000));
    fixture.put_youtrack_issue(FakeYoutrackIssue {
        assignee: Some("bea".to_owned()),
        ..sample_issue("2-3", "JL-2", 1_000_000)
    });

    pass(&fixture.context()).await;

    let jira = links_through(&mut fixture.connection, fixture.jira_credential).await;
    let keys: Vec<&str> = jira.iter().map(|link| link.external_key.as_str()).collect();
    assert_eq!(
        keys,
        ["ELY-1"],
        "open, assigned to the person, and inside the allowlist"
    );
    let github = links_through(&mut fixture.connection, fixture.github_credential).await;
    let keys: Vec<&str> = github
        .iter()
        .map(|link| link.external_key.as_str())
        .collect();
    assert_eq!(
        keys,
        [format!("{REPOSITORY}#1")],
        "an issue, not a pull request, in a chosen repository"
    );
    let youtrack = links_through(&mut fixture.connection, fixture.youtrack_account).await;
    let keys: Vec<&str> = youtrack
        .iter()
        .map(|link| link.external_key.as_str())
        .collect();
    assert_eq!(
        keys,
        ["JL-1"],
        "assigned to the person, in a chosen project"
    );

    let link = &jira[0];
    assert!(link.is_primary, "the issue is the item's own");
    let raised = item(&mut fixture, link.action_item_id).await;
    assert_eq!(
        raised.state,
        ActionItemState::Inbox,
        "nobody accepted it yet"
    );
    assert_eq!(raised.owner(), Owner::User);
    assert_eq!(raised.title, "Work on ELY-1");
    assert_eq!(raised.priority, ActionItemPriority::High);
    let history = list_for_item(&mut fixture.connection, raised.id)
        .await
        .expect("history");
    let entries: Vec<(&str, &str)> = history
        .iter()
        .map(|entry| (entry.kind.as_str(), entry.actor.as_str()))
        .collect();
    assert_eq!(
        entries,
        [("created", "watcher:jira"), ("link_added", "watcher:jira")]
    );

    // Read again, nothing comes in twice.
    pass(&fixture.context()).await;
    assert_eq!(item_count(&mut fixture).await, 3);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_issue_never_raises_a_second_item_once_it_has_one() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 5, github_issue_of_mine("Linked by hand"));
    fixture
        .github
        .put_issue(REPOSITORY, 6, github_issue_of_mine("Unlinked later"));

    // The person linked #5 to an item of their own before Elysium read what is assigned.
    let remote = fixture
        .github_provider()
        .find(
            fixture.github_credential.id,
            LinkKind::Issue,
            &format!("{REPOSITORY}#5"),
        )
        .await
        .expect("found");
    let by_hand = action_item_link::create_linked_item(
        &mut fixture.connection,
        NewActionItem {
            title: "Mine already".to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        },
        NewLink {
            credential: fixture.github_credential,
            kind: LinkKind::Issue,
            external_id: remote.external_id.clone(),
            observation: remote.observation(),
        },
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("linked by hand")
    .item
    .record
    .id;

    pass(&fixture.context()).await;
    assert_eq!(
        item_count(&mut fixture).await,
        2,
        "the hand-linked issue joins its item; only #6 raised one"
    );
    let named: Vec<Uuid> = assigned_issues::table
        .filter(assigned_issues::external_id.eq(format!("{REPOSITORY}#5")))
        .select(assigned_issues::action_item_id)
        .load(&mut fixture.connection)
        .await
        .expect("brought in");
    assert_eq!(named, [by_hand], "recorded as brought in, naming its item");

    // The person unlinks #6 from the item it raised and dismisses the item. It is still
    // assigned to them, and it raises nothing again.
    let unlinked = links_through(&mut fixture.connection, fixture.github_credential)
        .await
        .into_iter()
        .find(|link| link.external_key.ends_with("#6"))
        .expect("#6 came in");
    action_item_link::remove(
        &mut fixture.connection,
        unlinked.action_item_id,
        unlinked.id,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("unlinked");
    action_item::transition(
        &mut fixture.connection,
        unlinked.action_item_id,
        Transition::Dismiss,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("dismissed");

    pass(&fixture.context()).await;
    assert_eq!(item_count(&mut fixture).await, 2);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn two_passes_at_once_bring_an_issue_in_once() {
    let mut fixture = LinkFixture::start().await;
    fixture.jira.put_issue(jira_issue_of_mine("10001", "ELY-1"));
    let first = fixture.context();
    let second = fixture.context();

    tokio::join!(pass(&first), pass(&second));

    assert_eq!(item_count(&mut fixture).await, 1);
    assert_eq!(
        links_through(&mut fixture.connection, fixture.jira_credential)
            .await
            .len(),
        1
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn unassigning_leaves_the_item_and_says_so_and_closing_resolves_it() {
    let mut fixture = LinkFixture::start().await;
    fixture.jira.put_issue(jira_issue_of_mine("10001", "ELY-1"));
    pass(&fixture.context()).await;
    let item_id =
        links_through(&mut fixture.connection, fixture.jira_credential).await[0].action_item_id;

    // Sam takes it over.
    fixture.jira.change_issue("ELY-1", |issue| {
        issue.assignee = Some((SAM.0.to_owned(), SAM.1.to_owned()));
    });
    pass(&fixture.context()).await;

    let taken = item(&mut fixture, item_id).await;
    assert_eq!(taken.state, ActionItemState::Inbox, "the item stays");
    assert!(taken.deleted_at.is_none());
    let history = list_for_item(&mut fixture.connection, item_id)
        .await
        .expect("history");
    let unassigned = history
        .iter()
        .find(|entry| entry.kind == "unassigned")
        .expect("says so");
    assert_eq!(unassigned.actor, "watcher:jira");
    assert_eq!(unassigned.data["key"], "ELY-1");
    assert_eq!(unassigned.data["assignee"], "Sam");

    // It closes in Jira, which resolves its item as any link's closing does.
    fixture.jira.change_issue("ELY-1", |issue| {
        issue.status_id = DONE.to_owned();
    });
    pass(&fixture.context()).await;
    assert_eq!(
        item(&mut fixture, item_id).await.state,
        ActionItemState::Resolved
    );
    assert_eq!(item_count(&mut fixture).await, 1, "nothing came in twice");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_github_issue_shared_with_someone_else_is_still_the_persons() {
    let mut fixture = LinkFixture::start().await;
    // Sam was assigned first; the person shares it.
    fixture.github.put_issue(
        REPOSITORY,
        8,
        FakeIssue {
            assignee: Some("sam".to_owned()),
            co_assignees: vec![LOGIN.to_owned()],
            ..FakeIssue::open("Pair on it")
        },
    );

    pass(&fixture.context()).await;

    let github = links_through(&mut fixture.connection, fixture.github_credential).await;
    assert_eq!(github.len(), 1, "it came in");
    let raised = item(&mut fixture, github[0].action_item_id).await;
    assert_eq!(
        raised.owner(),
        Owner::User,
        "the person is one of its assignees"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn chosen_repositories_are_each_asked_for_what_is_assigned_in_them() {
    let mut fixture = LinkFixture::start().await;
    choose_repositories(&mut fixture, json!([REPOSITORY])).await;
    fixture
        .github
        .put_issue(REPOSITORY, 1, github_issue_of_mine("Here"));
    fixture.github.put_issue(
        REPOSITORY,
        2,
        FakeIssue {
            assignee: Some("sam".to_owned()),
            ..FakeIssue::open("Sam's")
        },
    );
    fixture
        .github
        .put_issue("acme/other", 3, github_issue_of_mine("Elsewhere"));

    pass(&fixture.context()).await;

    let keys: Vec<String> = links_through(&mut fixture.connection, fixture.github_credential)
        .await
        .into_iter()
        .map(|link| link.external_key)
        .collect();
    assert_eq!(keys, [format!("{REPOSITORY}#1")]);
    // The bound is in what GitHub was asked, not a filter over the account's listing, so what is
    // assigned elsewhere never uses up the read before a chosen repository is reached.
    let requests = fixture.github.received();
    assert!(
        requests.iter().all(|request| request.path != "/issues"),
        "the account's listing is not read when repositories are chosen"
    );
    let asked = requests
        .iter()
        .find(|request| {
            request.path == format!("/repos/{REPOSITORY}/issues")
                && request.query.contains(&format!("assignee={LOGIN}"))
        })
        .expect("the chosen repository is listed by assignee");
    assert!(asked.query.contains("state=open"), "{}", asked.query);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_assigned_read_that_fails_never_holds_the_link_cursor_back() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Linked"));
    let remote = fixture
        .github_provider()
        .find(
            fixture.github_credential.id,
            LinkKind::Issue,
            &format!("{REPOSITORY}#12"),
        )
        .await
        .expect("found");
    action_item_link::create_linked_item(
        &mut fixture.connection,
        NewActionItem {
            title: remote.title.clone(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::Nobody,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        },
        NewLink {
            credential: fixture.github_credential,
            kind: LinkKind::Issue,
            external_id: remote.external_id.clone(),
            observation: remote.observation(),
        },
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("linked");
    // A content the GitHub read cannot take, so every assigned read fails.
    choose_repositories(&mut fixture, json!(5)).await;

    let before = Utc::now();
    pass(&fixture.context()).await;

    let cursor = link_watch_cursor::find(&mut fixture.connection, fixture.github_credential)
        .await
        .expect("read")
        .expect("a cursor after a pass");
    assert!(
        cursor.watched_through >= before,
        "the links were read, so the next pass asks only for what changed since"
    );
    assert!(
        cursor.last_error.is_some(),
        "the failed read still says why"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_paused_youtrack_connector_is_not_read_by_the_watcher() {
    let mut fixture = LinkFixture::start().await;
    youtrack_connector(&mut fixture, json!(["0-1"])).await;
    let youtrack = connector::find_by_account(
        &mut fixture.connection,
        AccountRef::Token(fixture.youtrack_account.id),
    )
    .await
    .expect("read")
    .expect("the account carries its connector");
    connector::set_active(&mut fixture.connection, youtrack.id, false)
        .await
        .expect("paused");
    fixture.put_youtrack_issue(sample_issue("2-1", "JL-1", 1_000_000));

    let paused = connector::paused_link_accounts(&mut fixture.connection)
        .await
        .expect("listed");
    assert_eq!(paused, [fixture.youtrack_account]);
    pass(&fixture.context()).await;
    assert!(
        links_through(&mut fixture.connection, fixture.youtrack_account)
            .await
            .is_empty(),
        "nothing comes in through a paused connector"
    );
}
