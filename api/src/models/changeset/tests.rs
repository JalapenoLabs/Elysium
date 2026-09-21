// Copyright © 2026 Jalapeno Labs

//! Changesets against a real Postgres: proposing, deciding with dependencies, applying in
//! part with failures isolated, provider writes left to the watcher, and undoing.

use chrono::TimeDelta;
use serde_json::json;

use super::*;
use crate::action_items::changesets::{
    AddComment, AddLink, Existing, FinishItem, LinkReads, Membership, Proposed, ProposedOperation,
    Proposer, UpdateItem, read_link_targets,
};
use crate::action_items::links::tests::{LinkFixture, REPOSITORY};
use crate::action_items::{Actor, Transition, watcher};
use crate::github::fake::FakeIssue;
use crate::models::action_item::{
    ActionItem, ActionItemChanges, ActionItemFilter, ActionItemPriority, ActionItemState,
    NewActionItem, Owner,
};
use crate::models::action_item_event::list_for_item;
use crate::models::action_item_link::{LinkKind, LinkProvider, NewLink};
use crate::models::initiative::NewInitiative;
use crate::models::project::NewProject;
use crate::models::{action_item_comment, action_item_link, action_item_link_write};
use crate::routes::v1::action_items::LinkTarget;
use crate::test_support::migrated_database;

fn minute(offset: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-21T12:00:00Z")
        .expect("a valid instant")
        .with_timezone(&Utc)
        + TimeDelta::minutes(offset)
}

fn existing(id: Uuid) -> Target {
    Target::Existing(Existing { id })
}

fn proposed(operation: u32) -> Target {
    Target::Proposed(Proposed { operation })
}

fn new_item(title: &str) -> CreateItem {
    CreateItem {
        title: title.to_owned(),
        notes: String::new(),
        priority: ActionItemPriority::Normal,
        due_at: None,
        project_ids: Vec::new(),
        initiatives: Vec::new(),
    }
}

fn new_initiative(name: &str) -> CreateInitiative {
    CreateInitiative {
        name: name.to_owned(),
        description: String::new(),
        target_at: None,
        project_ids: Vec::new(),
    }
}

fn update_title(item: Target, title: &str) -> Operation {
    Operation::UpdateItem(UpdateItem {
        item,
        title: Some(title.to_owned()),
        notes: None,
        priority: None,
        due_at: None,
    })
}

fn comment(item: Target, body: &str) -> Operation {
    Operation::Comment(AddComment {
        item,
        body: body.to_owned(),
    })
}

/// A proposal from Elysia, each operation with a reason.
fn from_elysia(operations: Vec<Operation>) -> Proposal {
    Proposal {
        proposer: Proposer::Elysia,
        project_id: None,
        summary: "Follow-ups from the planning meeting".to_owned(),
        operations: operations
            .into_iter()
            .map(|operation| ProposedOperation {
                operation,
                reason: "Sam asked for it in the meeting".to_owned(),
                quote: Some("Can we get that done this week?".to_owned()),
                source: Some("Planning, 00:12:03".to_owned()),
            })
            .collect(),
    }
}

async fn item_titled(
    connection: &mut AsyncPgConnection,
    title: &str,
    state: ActionItemState,
) -> ActionItem {
    let new_item = NewActionItem {
        title: title.to_owned(),
        notes: String::new(),
        state,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    };
    action_item::create(connection, new_item, Actor::User, minute(0))
        .await
        .expect("item")
        .record
}

async fn initiative_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    let new_initiative = NewInitiative {
        name: name.to_owned(),
        description: String::new(),
        target_at: None,
        project_ids: Vec::new(),
    };
    initiative::create(connection, new_initiative, Actor::User, minute(0))
        .await
        .expect("initiative")
        .record
        .id
}

/// Every live item's title, sorted.
async fn live_titles(connection: &mut AsyncPgConnection) -> Vec<String> {
    let mut titles: Vec<String> =
        action_item::list(connection, &ActionItemFilter::default(), minute(100))
            .await
            .expect("items")
            .into_iter()
            .map(|item| item.title)
            .collect();
    titles.sort();
    titles
}

async fn history(connection: &mut AsyncPgConnection, item_id: Uuid) -> Vec<HistoryEntry> {
    list_for_item(connection, item_id).await.expect("history")
}

fn outcomes(staged: &Staged) -> Vec<ChangesetOutcome> {
    staged.operations.iter().map(|row| row.outcome).collect()
}

fn decisions(staged: &Staged) -> Vec<ChangesetDecision> {
    staged.operations.iter().map(|row| row.decision).collect()
}

/// The ids of the operations at `positions`.
fn operation_ids(staged: &Staged, positions: &[i32]) -> Vec<Uuid> {
    staged
        .operations
        .iter()
        .filter(|row| positions.contains(&row.position))
        .map(|row| row.id)
        .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_proposal_is_stored_pending_and_writes_nothing_until_it_is_applied() {
    let (_url, mut connection) = migrated_database().await;
    let existing_item = item_titled(&mut connection, "Reply to Sam", ActionItemState::Open).await;

    let staged = propose(
        &mut connection,
        from_elysia(vec![
            Operation::CreateItem(new_item("Draft the launch post")),
            comment(proposed(1), "Sam will review it."),
            Operation::ResolveItem(FinishItem {
                item: existing(existing_item.id),
            }),
        ]),
        minute(1),
    )
    .await
    .expect("proposed");

    assert_eq!(staged.changeset.state, ChangesetState::Pending);
    assert_eq!(staged.changeset.proposer, "elysia");
    assert_eq!(decisions(&staged), [ChangesetDecision::Pending; 3]);
    assert_eq!(outcomes(&staged), [ChangesetOutcome::Pending; 3]);
    assert_eq!(
        staged.operations[0].quote.as_deref(),
        Some("Can we get that done this week?")
    );
    assert_eq!(
        live_titles(&mut connection).await,
        ["Reply to Sam"],
        "nothing is created until the user applies it"
    );
    let unchanged = action_item::find(&mut connection, existing_item.id)
        .await
        .expect("item");
    assert_eq!(unchanged.state, ActionItemState::Open);

    let listed = list(&mut connection, &[ChangesetState::Pending])
        .await
        .expect("listed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].operations.len(), 3);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_proposal_naming_what_is_not_there_is_refused_whole() {
    let (_url, mut connection) = migrated_database().await;
    let deleted = item_titled(&mut connection, "Gone", ActionItemState::Open).await;
    action_item::soft_delete(&mut connection, deleted.id, Actor::User, minute(1))
        .await
        .expect("deleted");

    let refused = propose(
        &mut connection,
        from_elysia(vec![
            Operation::CreateItem(new_item("Fine")),
            comment(existing(deleted.id), "About the deleted one."),
        ]),
        minute(2),
    )
    .await;
    let Err(WorkError::Refused(message)) = refused else {
        panic!("a deleted item is refused: {refused:?}");
    };
    assert_eq!(
        message,
        format!(
            "operation 2: action item {} does not exist or is deleted",
            deleted.id
        )
    );

    let unknown_project = CreateItem {
        project_ids: vec![Uuid::now_v7()],
        ..new_item("Somewhere")
    };
    let refused = propose(
        &mut connection,
        from_elysia(vec![Operation::CreateItem(unknown_project)]),
        minute(2),
    )
    .await;
    assert!(matches!(refused, Err(WorkError::Refused(_))), "{refused:?}");

    let forward = propose(
        &mut connection,
        from_elysia(vec![comment(proposed(2), "Too early.")]),
        minute(2),
    )
    .await;
    assert!(matches!(forward, Err(WorkError::Refused(_))), "{forward:?}");

    assert!(
        list(&mut connection, &[]).await.expect("listed").is_empty(),
        "nothing refused is stored"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn rejecting_an_operation_rejects_its_dependents_and_none_of_them_apply() {
    let (_url, mut connection) = migrated_database().await;
    let launch = initiative_named(&mut connection, "Launch").await;
    let reply = item_titled(&mut connection, "Reply to Sam", ActionItemState::Open).await;
    let staged = propose(
        &mut connection,
        from_elysia(vec![
            Operation::CreateItem(new_item("Draft the launch post")),
            comment(proposed(1), "Sam will review it."),
            Operation::AddToInitiative(Membership {
                item: proposed(1),
                initiative: existing(launch),
            }),
            update_title(existing(reply.id), "Reply to Sam about logs"),
        ]),
        minute(1),
    )
    .await
    .expect("proposed");
    let id = staged.changeset.id;

    let decided = decide(
        &mut connection,
        id,
        ChangesetDecision::Rejected,
        Some(operation_ids(&staged, &[1])),
    )
    .await
    .expect("rejected");
    assert_eq!(
        decisions(&decided),
        [
            ChangesetDecision::Rejected,
            ChangesetDecision::Rejected,
            ChangesetDecision::Rejected,
            ChangesetDecision::Pending,
        ],
        "rejecting the create rejects what acts on its item"
    );

    let refused = decide(
        &mut connection,
        id,
        ChangesetDecision::Approved,
        Some(operation_ids(&staged, &[2])),
    )
    .await;
    assert!(
        matches!(&refused, Err(WorkError::Refused(message)) if message.contains("operation 1")),
        "{refused:?}"
    );

    let undecided = apply(&mut connection, id, LinkReads::new(), minute(2)).await;
    assert!(
        matches!(&undecided, Err(WorkError::Refused(message)) if message.ends_with("undecided: 4")),
        "{undecided:?}"
    );

    decide(
        &mut connection,
        id,
        ChangesetDecision::Approved,
        Some(operation_ids(&staged, &[4])),
    )
    .await
    .expect("approved");
    let applied = apply(&mut connection, id, LinkReads::new(), minute(3))
        .await
        .expect("applied");

    assert_eq!(applied.staged.changeset.state, ChangesetState::Applied);
    assert_eq!(
        outcomes(&applied.staged),
        [
            ChangesetOutcome::Skipped,
            ChangesetOutcome::Skipped,
            ChangesetOutcome::Skipped,
            ChangesetOutcome::Applied,
        ]
    );
    assert_eq!(
        live_titles(&mut connection).await,
        ["Reply to Sam about logs"],
        "the rejected create wrote nothing"
    );
    let entry = history(&mut connection, reply.id)
        .await
        .pop()
        .expect("the update's entry");
    assert_eq!(entry.kind, "updated");
    assert_eq!(entry.actor, "elysia", "the proposer made the change");
    assert_eq!(entry.changeset_id, Some(id), "the changeset is its source");

    let again = decide(&mut connection, id, ChangesetDecision::Approved, None).await;
    assert!(
        matches!(again, Err(WorkError::Conflict(_))),
        "decisions are final once applied"
    );
    let reapplied = apply(&mut connection, id, LinkReads::new(), minute(4)).await;
    assert!(matches!(reapplied, Err(WorkError::Conflict(_))));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_failing_operation_rolls_back_alone_and_the_rest_still_apply() {
    let (_url, mut connection) = migrated_database().await;
    let archived = initiative_named(&mut connection, "Archived").await;
    let reply = item_titled(&mut connection, "Reply to Sam", ActionItemState::Open).await;
    let staged = propose(
        &mut connection,
        from_elysia(vec![
            Operation::CreateItem(CreateItem {
                initiatives: vec![existing(archived)],
                ..new_item("Into the archive")
            }),
            comment(proposed(1), "About the archived work."),
            Operation::ResolveItem(FinishItem {
                item: existing(reply.id),
            }),
            Operation::CreateInitiative(new_initiative("Launch")),
            Operation::CreateItem(CreateItem {
                initiatives: vec![proposed(4)],
                ..new_item("Draft the launch post")
            }),
        ]),
        minute(1),
    )
    .await
    .expect("proposed");
    let id = staged.changeset.id;

    // The initiative the first operation joins is deleted after it was proposed.
    initiative::soft_delete(&mut connection, archived, Actor::User, minute(2))
        .await
        .expect("deleted");
    decide(&mut connection, id, ChangesetDecision::Approved, None)
        .await
        .expect("approved");
    let applied = apply(&mut connection, id, LinkReads::new(), minute(3))
        .await
        .expect("applied");

    assert_eq!(
        outcomes(&applied.staged),
        [
            ChangesetOutcome::Failed,
            ChangesetOutcome::Skipped,
            ChangesetOutcome::Applied,
            ChangesetOutcome::Applied,
            ChangesetOutcome::Applied,
        ]
    );
    let operations = &applied.staged.operations;
    assert_eq!(
        operations[0].error.as_deref(),
        Some("initiativeIds lists an initiative that does not exist or is deleted")
    );
    assert_eq!(
        operations[1].error.as_deref(),
        Some("operation 1 was not applied")
    );
    assert_eq!(
        live_titles(&mut connection).await,
        ["Draft the launch post", "Reply to Sam"],
        "the failed create left nothing behind"
    );

    let resolved = action_item::find(&mut connection, reply.id)
        .await
        .expect("item");
    assert_eq!(resolved.state, ActionItemState::Resolved);
    let launch_id: Uuid =
        serde_json::from_value(operations[3].result["initiativeId"].clone()).expect("an id");
    let post_id: Uuid =
        serde_json::from_value(operations[4].result["itemId"].clone()).expect("an id");
    let memberships = action_item::memberships(&mut connection, &[post_id])
        .await
        .expect("memberships");
    assert_eq!(
        memberships.initiative_ids(post_id),
        [launch_id],
        "a later operation names what an earlier one created"
    );
    assert!(applied.touched.item_ids.contains(&post_id));
    assert!(applied.touched.initiative_ids.contains(&launch_id));
    assert!(
        applied
            .touched
            .history
            .iter()
            .all(|entry| entry.changeset_id == Some(id)),
        "every entry the changeset recorded names it"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn rejecting_everything_applies_nothing_and_marks_it_rejected() {
    let (_url, mut connection) = migrated_database().await;
    let staged = propose(
        &mut connection,
        from_elysia(vec![Operation::CreateItem(new_item("Not wanted"))]),
        minute(1),
    )
    .await
    .expect("proposed");
    let id = staged.changeset.id;
    decide(&mut connection, id, ChangesetDecision::Rejected, None)
        .await
        .expect("rejected");
    let written = apply(&mut connection, id, LinkReads::new(), minute(2))
        .await
        .expect("settled");
    assert_eq!(written.staged.changeset.state, ChangesetState::Rejected);
    assert_eq!(written.staged.changeset.decided_at, Some(minute(2)));
    assert!(live_titles(&mut connection).await.is_empty());
    let undo_rejected = undo(&mut connection, id, minute(3)).await;
    assert!(matches!(undo_rejected, Err(WorkError::Conflict(_))));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario end to end: every kind applied, one field edited after, then undone"
)]
async fn undo_reverses_what_was_applied_and_keeps_what_the_user_changed_since() {
    let (_url, mut connection) = migrated_database().await;
    let project = project::create(
        &mut connection,
        &NewProject {
            name: "Elysium".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("project");
    let launch = initiative_named(&mut connection, "Launch").await;
    let reply = item_titled(&mut connection, "Reply to Sam", ActionItemState::Open).await;
    let triage = item_titled(&mut connection, "Triage me", ActionItemState::Inbox).await;
    let staged = propose(
        &mut connection,
        from_elysia(vec![
            Operation::CreateItem(CreateItem {
                project_ids: vec![project.id],
                ..new_item("Draft the launch post")
            }),
            Operation::UpdateItem(UpdateItem {
                item: existing(reply.id),
                title: Some("Reply to Sam about logs".to_owned()),
                notes: None,
                priority: Some(ActionItemPriority::High),
                due_at: None,
            }),
            Operation::ResolveItem(FinishItem {
                item: existing(triage.id),
            }),
            comment(existing(reply.id), "Logs are attached."),
            Operation::AddToInitiative(Membership {
                item: existing(reply.id),
                initiative: existing(launch),
            }),
            Operation::CreateInitiative(new_initiative("Follow-ups")),
        ]),
        minute(1),
    )
    .await
    .expect("proposed");
    let id = staged.changeset.id;
    decide(&mut connection, id, ChangesetDecision::Approved, None)
        .await
        .expect("approved");
    let applied = apply(&mut connection, id, LinkReads::new(), minute(2))
        .await
        .expect("applied");
    assert_eq!(outcomes(&applied.staged), [ChangesetOutcome::Applied; 6]);
    let post_id: Uuid =
        serde_json::from_value(applied.staged.operations[0].result["itemId"].clone())
            .expect("an id");
    let follow_ups: Uuid =
        serde_json::from_value(applied.staged.operations[5].result["initiativeId"].clone())
            .expect("an id");

    // The user changes the priority after the changeset; undo must leave it.
    action_item::update(
        &mut connection,
        reply.id,
        ActionItemChanges {
            priority: Some(ActionItemPriority::Urgent),
            ..ActionItemChanges::default()
        },
        Actor::User,
        minute(3),
    )
    .await
    .expect("edited");

    let undone = undo(&mut connection, id, minute(4)).await.expect("undone");
    assert_eq!(undone.staged.changeset.state, ChangesetState::Undone);
    assert_eq!(undone.staged.changeset.undone_at, Some(minute(4)));
    assert_eq!(outcomes(&undone.staged), [ChangesetOutcome::Undone; 6]);
    assert_eq!(
        undone.staged.operations[1].undo,
        Some(json!({ "kept": ["priority"] })),
        "the review says which field kept the user's value"
    );

    let post = action_item::find(&mut connection, post_id)
        .await
        .expect("item");
    assert!(
        post.deleted_at.is_some(),
        "the created item is deleted softly"
    );
    let reply = action_item::find(&mut connection, reply.id)
        .await
        .expect("item");
    assert_eq!(reply.title, "Reply to Sam");
    assert_eq!(reply.priority, ActionItemPriority::Urgent);
    let triage = action_item::find(&mut connection, triage.id)
        .await
        .expect("item");
    assert_eq!(
        (triage.state, triage.resolved_at),
        (ActionItemState::Inbox, None),
        "an item resolved from the inbox returns to the inbox"
    );
    assert!(
        action_item_comment::list(&mut connection, reply.id)
            .await
            .expect("comments")
            .is_empty(),
        "the comment is taken back"
    );
    assert!(
        action_item::current_initiative_ids(&mut connection, reply.id)
            .await
            .expect("initiatives")
            .is_empty()
    );
    let follow_ups = initiative::find(&mut connection, follow_ups)
        .await
        .expect("initiative");
    assert!(follow_ups.deleted_at.is_some());

    let undo_entries: Vec<(String, String)> = history(&mut connection, reply.id)
        .await
        .into_iter()
        .filter(|entry| entry.changeset_id == Some(id) && entry.actor == "user")
        .map(|entry| (entry.kind, entry.actor))
        .collect();
    assert_eq!(
        undo_entries.len(),
        3,
        "the title, the comment, and the membership each record the undo: {undo_entries:?}"
    );

    let twice = undo(&mut connection, id, minute(5)).await;
    assert!(matches!(twice, Err(WorkError::Conflict(_))), "undone once");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn undo_leaves_a_membership_the_user_changed_since() {
    let (_url, mut connection) = migrated_database().await;
    let launch = initiative_named(&mut connection, "Launch").await;
    let archive = initiative_named(&mut connection, "Archive").await;
    let rejoined = item_titled(&mut connection, "Rejoined", ActionItemState::Open).await;
    let taken_out = item_titled(&mut connection, "Taken out", ActionItemState::Open).await;
    action_item::join_initiative(
        &mut connection,
        taken_out.id,
        archive,
        Actor::User,
        minute(0),
    )
    .await
    .expect("joined");
    let staged = propose(
        &mut connection,
        from_elysia(vec![
            Operation::AddToInitiative(Membership {
                item: existing(rejoined.id),
                initiative: existing(launch),
            }),
            Operation::RemoveFromInitiative(Membership {
                item: existing(taken_out.id),
                initiative: existing(archive),
            }),
        ]),
        minute(1),
    )
    .await
    .expect("proposed");
    let id = staged.changeset.id;
    decide(&mut connection, id, ChangesetDecision::Approved, None)
        .await
        .expect("approved");
    apply(&mut connection, id, LinkReads::new(), minute(2))
        .await
        .expect("applied");

    // The user takes the first item out and puts it back, and puts the second back in: each
    // membership now stands on a span the changeset did not open or close.
    action_item::leave_initiative(&mut connection, rejoined.id, launch, Actor::User, minute(3))
        .await
        .expect("left");
    action_item::join_initiative(&mut connection, rejoined.id, launch, Actor::User, minute(4))
        .await
        .expect("rejoined");
    action_item::join_initiative(
        &mut connection,
        taken_out.id,
        archive,
        Actor::User,
        minute(4),
    )
    .await
    .expect("put back");

    let undone = undo(&mut connection, id, minute(5)).await.expect("undone");
    assert_eq!(
        outcomes(&undone.staged),
        [ChangesetOutcome::Applied, ChangesetOutcome::Applied],
        "neither membership is reversed"
    );
    for operation in &undone.staged.operations {
        assert_eq!(operation.undo, Some(json!({ "movedSince": true })));
    }
    assert_eq!(
        action_item::current_initiative_ids(&mut connection, rejoined.id)
            .await
            .expect("initiatives"),
        [launch],
        "the item the user put back stays in"
    );
    assert_eq!(
        action_item::current_initiative_ids(&mut connection, taken_out.id)
            .await
            .expect("initiatives"),
        [archive],
        "and is not added twice"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn undo_reverses_several_membership_changes_to_one_initiative() {
    let (_url, mut connection) = migrated_database().await;
    let launch = initiative_named(&mut connection, "Launch").await;
    let outside = item_titled(&mut connection, "Starts outside", ActionItemState::Open).await;
    let inside = item_titled(&mut connection, "Starts inside", ActionItemState::Open).await;
    action_item::join_initiative(&mut connection, inside.id, launch, Actor::User, minute(0))
        .await
        .expect("joined");
    let membership = |item: Uuid| Membership {
        item: existing(item),
        initiative: existing(launch),
    };
    let staged = propose(
        &mut connection,
        from_elysia(vec![
            Operation::AddToInitiative(membership(outside.id)),
            Operation::RemoveFromInitiative(membership(outside.id)),
            Operation::RemoveFromInitiative(membership(inside.id)),
            Operation::AddToInitiative(membership(inside.id)),
        ]),
        minute(1),
    )
    .await
    .expect("proposed");
    let id = staged.changeset.id;
    decide(&mut connection, id, ChangesetDecision::Approved, None)
        .await
        .expect("approved");
    apply(&mut connection, id, LinkReads::new(), minute(2))
        .await
        .expect("applied");

    // Reversing the later change of each pair writes a span; the earlier change must read it
    // as the state it restored, not as the user moving the item.
    let undone = undo(&mut connection, id, minute(3)).await.expect("undone");
    assert_eq!(outcomes(&undone.staged), [ChangesetOutcome::Undone; 4]);
    assert!(
        action_item::current_initiative_ids(&mut connection, outside.id)
            .await
            .expect("initiatives")
            .is_empty(),
        "the item that started outside ends outside"
    );
    assert_eq!(
        action_item::current_initiative_ids(&mut connection, inside.id)
            .await
            .expect("initiatives"),
        [launch],
        "the item that started inside ends inside"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn undo_keeps_an_item_the_user_moved_and_one_deleted_since() {
    let (_url, mut connection) = migrated_database().await;
    let reopened = item_titled(&mut connection, "Reopened", ActionItemState::Open).await;
    let deleted = item_titled(&mut connection, "Deleted", ActionItemState::Open).await;
    let staged = propose(
        &mut connection,
        from_elysia(vec![
            Operation::ResolveItem(FinishItem {
                item: existing(reopened.id),
            }),
            Operation::DismissItem(FinishItem {
                item: existing(deleted.id),
            }),
        ]),
        minute(1),
    )
    .await
    .expect("proposed");
    let id = staged.changeset.id;
    decide(&mut connection, id, ChangesetDecision::Approved, None)
        .await
        .expect("approved");
    apply(&mut connection, id, LinkReads::new(), minute(2))
        .await
        .expect("applied");

    action_item::transition(
        &mut connection,
        reopened.id,
        Transition::Reopen,
        Actor::User,
        minute(3),
    )
    .await
    .expect("reopened");
    action_item::soft_delete(&mut connection, deleted.id, Actor::User, minute(3))
        .await
        .expect("deleted");

    let undone = undo(&mut connection, id, minute(4)).await.expect("undone");
    assert_eq!(
        outcomes(&undone.staged),
        [ChangesetOutcome::Applied, ChangesetOutcome::Applied],
        "neither could be reversed"
    );
    assert_eq!(
        undone.staged.operations[0].undo,
        Some(json!({ "movedSince": true }))
    );
    assert_eq!(
        undone.staged.operations[1].undo,
        Some(json!({ "refusal": "the item is deleted; restore it first" }))
    );
    assert_eq!(undone.staged.changeset.state, ChangesetState::Undone);
}

/// An item made from GitHub issue `number` of the fake, with that issue as its primary link.
async fn item_linked_to_issue(fixture: &mut LinkFixture, number: u64) -> Uuid {
    let reference = format!("{REPOSITORY}#{number}");
    let remote = fixture
        .github_provider()
        .find(fixture.github_credential.id, LinkKind::Issue, &reference)
        .await
        .expect("found");
    let new_item = NewActionItem {
        title: remote.title.clone(),
        notes: String::new(),
        state: ActionItemState::Open,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    };
    let new_link = NewLink {
        credential: fixture.github_credential,
        kind: LinkKind::Issue,
        external_id: remote.external_id.clone(),
        observation: remote.observation(),
    };
    action_item_link::create_linked_item(
        &mut fixture.connection,
        new_item,
        new_link,
        Actor::User,
        minute(0),
    )
    .await
    .expect("created")
    .item
    .record
    .id
}

fn link_pull_request(fixture: &LinkFixture, item: Target, number: u64) -> Operation {
    Operation::Link(AddLink {
        item,
        target: LinkTarget {
            provider: LinkProvider::Github,
            credential_id: fixture.github_credential.id,
            kind: LinkKind::PullRequest,
            reference: format!("{REPOSITORY}#{number}"),
        },
    })
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario end to end: a refusing provider, then landing, then undoing"
)]
async fn a_provider_that_refuses_fails_only_its_operation_and_its_writes_stay_owed() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 12, FakeIssue::open("Fix the login bug"));
    fixture
        .github
        .put_issue(REPOSITORY, 13, FakeIssue::pull_request("Fix login"));
    let linked = item_linked_to_issue(&mut fixture, 12).await;
    let unlinked = item_titled(
        &mut fixture.connection,
        "Write it up",
        ActionItemState::Open,
    )
    .await;

    let proposal = from_elysia(vec![
        // The fake holds no pull request 99, so reading it fails.
        link_pull_request(&fixture, existing(unlinked.id), 99),
        link_pull_request(&fixture, existing(unlinked.id), 13),
        comment(existing(linked), "Fixed in the pull request."),
        Operation::ResolveItem(FinishItem {
            item: existing(linked),
        }),
    ]);
    let staged = propose(&mut fixture.connection, proposal, minute(1))
        .await
        .expect("proposed");
    let id = staged.changeset.id;
    let decided = decide(
        &mut fixture.connection,
        id,
        ChangesetDecision::Approved,
        None,
    )
    .await
    .expect("approved");

    let reads = read_link_targets(&fixture.links, &decided.operations).await;
    fixture.github.refuse_writes(true);
    let applied = apply(&mut fixture.connection, id, reads, minute(2))
        .await
        .expect("applied");
    assert_eq!(
        outcomes(&applied.staged),
        [
            ChangesetOutcome::Failed,
            ChangesetOutcome::Applied,
            ChangesetOutcome::Applied,
            ChangesetOutcome::Applied,
        ]
    );
    assert!(
        applied.staged.operations[0].error.is_some(),
        "the provider's answer is kept"
    );
    let pull_request = action_item_link::list_for_item(&mut fixture.connection, unlinked.id)
        .await
        .expect("links")
        .pop()
        .expect("the pull request's link");
    assert!(
        pull_request.is_primary,
        "Elysia's first link on an item becomes its primary, as the user's does"
    );

    watcher::pass(&fixture.context()).await;
    let links = action_item_link::list_for_item(&mut fixture.connection, linked)
        .await
        .expect("links");
    let link_ids: Vec<Uuid> = links.iter().map(|link| link.id).collect();
    let owed = action_item_link_write::for_links(&mut fixture.connection, &link_ids)
        .await
        .expect("owed");
    assert_eq!(owed.len(), 2, "the comment and the close both wait");
    assert!(owed.iter().all(|write| write.last_error.is_some()));
    let resolved = action_item::find(&mut fixture.connection, linked)
        .await
        .expect("item");
    assert_eq!(
        resolved.state,
        ActionItemState::Resolved,
        "Elysium's change stands while the provider refuses"
    );

    // Once the provider takes the writes, undoing cannot take them back, and says so.
    fixture.github.refuse_writes(false);
    watcher::pass(&fixture.context()).await;
    // A comment the user writes after owes its own post, which says nothing about whether
    // the issue is closed; the closed issue must still be named.
    action_item_comment::create(
        &mut fixture.connection,
        linked,
        "Following up.".to_owned(),
        Actor::User,
        minute(3),
    )
    .await
    .expect("commented");
    let undone = undo(&mut fixture.connection, id, minute(3))
        .await
        .expect("undone");
    let key = format!("{REPOSITORY}#12");
    assert_eq!(
        undone.staged.operations[2].undo,
        Some(json!({ "stillPosted": { "provider": "github", "key": key } }))
    );
    assert_eq!(
        undone.staged.operations[3].undo,
        Some(json!({ "stillClosed": [{ "provider": "github", "key": key }] }))
    );
    let returned = action_item::find(&mut fixture.connection, linked)
        .await
        .expect("item");
    assert_eq!(returned.state, ActionItemState::Open);
}
