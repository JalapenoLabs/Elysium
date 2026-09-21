// Copyright © 2026 Jalapeno Labs

//! `work_propose_changes`: what an agent may send, and staging against a real Postgres,
//! scoped to the session's project while another project holds work it must never reach.

use serde_json::json;

use super::*;
use crate::action_items::Actor;
use crate::action_items::changesets::{
    AddComment, CreateItem, Existing, FinishItem, Membership, Proposed,
};
use crate::models::action_item::{
    self, ActionItemFilter, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::changeset::{ChangesetDecision, ChangesetState};
use crate::models::initiative::{self, NewInitiative};
use crate::models::project::{self, NewProject};
use crate::test_support::migrated_database;

#[test]
fn agents_link_only_pull_requests_by_url_and_never_choose_projects() {
    let item = json!({ "id": Uuid::nil() });

    let pull_request = parse_change(json!({
        "kind": "link-pull-request",
        "item": item,
        "url": "https://github.com/JalapenoLabs/Elysium/pull/12",
    }))
    .expect("a pull request by its URL");
    let Draft::LinkPullRequest { reference, .. } = pull_request else {
        panic!("a pull request draft");
    };
    assert_eq!(reference.to_string(), "JalapenoLabs/Elysium#12");

    let issue_url = parse_change(json!({
        "kind": "link-pull-request",
        "item": item,
        "url": "https://github.com/JalapenoLabs/Elysium/issues/12",
    }));
    assert!(
        issue_url.is_err_and(|refusal| refusal.starts_with("url is not a GitHub pull request"))
    );

    let any_link = parse_change(json!({
        "kind": "link",
        "item": item,
        "target": {
            "provider": "jira",
            "credentialId": Uuid::nil(),
            "kind": "issue",
            "reference": "ELY-12",
        },
    }));
    assert!(any_link.is_err_and(|refusal| refusal.contains("link-pull-request")));

    let elsewhere = parse_change(json!({
        "kind": "create-item",
        "title": "Somewhere else",
        "projectIds": [Uuid::nil()],
    }));
    assert!(elsewhere.is_err_and(|refusal| refusal.starts_with("leave projectIds out")));

    let resolve = parse_change(json!({ "kind": "resolve-item", "item": item }))
        .expect("resolving is proposed like any operation");
    assert!(matches!(
        resolve,
        Draft::Operation(Operation::ResolveItem(_))
    ));
    parse_change(json!({ "kind": "delete-item", "item": item }))
        .expect_err("an agent cannot propose deleting");
}

async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("project")
    .id
}

async fn item_in(connection: &mut AsyncPgConnection, title: &str, project_id: Uuid) -> Uuid {
    let new_item = NewActionItem {
        title: title.to_owned(),
        notes: String::new(),
        state: ActionItemState::Open,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: vec![project_id],
        initiative_ids: Vec::new(),
    };
    action_item::create(connection, new_item, Actor::User, Utc::now())
        .await
        .expect("item")
        .record
        .id
}

fn proposed_operation(operation: Operation) -> ProposedOperation {
    ProposedOperation {
        operation,
        reason: "The fix is merged.".to_owned(),
        quote: None,
        source: Some("src/auth.rs:12".to_owned()),
    }
}

fn existing(id: Uuid) -> Target {
    Target::Existing(Existing { id })
}

/// The session's project with one item, and another project with an item and an initiative
/// the session must never reach.
struct Fixture {
    connection: AsyncPgConnection,
    scope: WorkScope,
    own: Uuid,
    theirs: Uuid,
    their_initiative: Uuid,
}

async fn fixture() -> Fixture {
    let (_url, mut connection) = migrated_database().await;
    let project_id = project_named(&mut connection, "Elysium").await;
    let other_project = project_named(&mut connection, "Farworlds").await;
    let own = item_in(&mut connection, "Fix the login bug", project_id).await;
    let theirs = item_in(&mut connection, "Their bug", other_project).await;
    let their_initiative = initiative::create(
        &mut connection,
        NewInitiative {
            name: "Their launch".to_owned(),
            description: String::new(),
            target_at: None,
            project_ids: vec![other_project],
        },
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("initiative")
    .record
    .id;
    Fixture {
        connection,
        scope: WorkScope {
            session_id: 7,
            project_id,
            action_item_id: None,
        },
        own,
        theirs,
        their_initiative,
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_proposal_naming_another_projects_work_is_refused() {
    let Fixture {
        mut connection,
        scope,
        own,
        theirs,
        their_initiative,
    } = fixture().await;

    let resolve_theirs = stage(
        &mut connection,
        scope,
        "Tidy up".to_owned(),
        vec![proposed_operation(Operation::ResolveItem(FinishItem {
            item: existing(theirs),
        }))],
        Utc::now(),
    )
    .await
    .expect_err("another project's item");
    let Failure::Refused(error) = resolve_theirs else {
        panic!("a refusal the agent reads");
    };
    assert_eq!(error, ToolError::ItemUnavailable(theirs));

    let into_theirs = stage(
        &mut connection,
        scope,
        "Tidy up".to_owned(),
        vec![proposed_operation(Operation::AddToInitiative(Membership {
            item: existing(own),
            initiative: existing(their_initiative),
        }))],
        Utc::now(),
    )
    .await
    .expect_err("another project's initiative");
    let Failure::Refused(error) = into_theirs else {
        panic!("a refusal the agent reads");
    };
    assert_eq!(error, ToolError::InitiativeUnavailable(their_initiative));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_proposal_stages_the_projects_own_work_and_writes_nothing() {
    let Fixture {
        mut connection,
        scope,
        own,
        ..
    } = fixture().await;
    let project_id = scope.project_id;

    let staged = stage(
        &mut connection,
        scope,
        "Follow-ups from fixing the login bug".to_owned(),
        vec![
            proposed_operation(Operation::ResolveItem(FinishItem {
                item: existing(own),
            })),
            proposed_operation(Operation::CreateItem(CreateItem {
                title: "Add a regression test".to_owned(),
                notes: String::new(),
                priority: ActionItemPriority::High,
                due_at: None,
                project_ids: Vec::new(),
                initiatives: Vec::new(),
            })),
            proposed_operation(Operation::Comment(AddComment {
                item: Target::Proposed(Proposed { operation: 2 }),
                body: "The login test is missing.".to_owned(),
            })),
        ],
        Utc::now(),
    )
    .await
    .expect("staged");

    assert_eq!(staged.changeset.proposer, "session:7");
    assert_eq!(staged.changeset.project_id, Some(project_id));
    assert_eq!(staged.changeset.state, ChangesetState::Pending);
    assert_eq!(
        staged.operations[1].operation["projectIds"],
        json!([project_id]),
        "what the agent proposes joins its project"
    );
    assert!(
        staged
            .operations
            .iter()
            .all(|row| row.decision == ChangesetDecision::Pending)
    );

    let answer = staged_answer(&staged);
    assert_eq!(answer["changeset"]["state"], "pending");
    assert_eq!(
        answer["changeset"]["operations"],
        json!([
            { "position": 1, "kind": "resolve-item", "dependsOn": [] },
            { "position": 2, "kind": "create-item", "dependsOn": [] },
            { "position": 3, "kind": "comment", "dependsOn": [2] },
        ])
    );
    assert!(
        answer["message"]
            .as_str()
            .is_some_and(|message| message.contains("Nothing changes until the user approves"))
    );

    let items = action_item::list(&mut connection, &ActionItemFilter::default(), Utc::now())
        .await
        .expect("items");
    assert_eq!(items.len(), 2, "proposing created nothing");
    assert!(
        items.iter().all(|item| item.state == ActionItemState::Open),
        "and resolved nothing"
    );
}
