// Copyright © 2026 Jalapeno Labs

//! Applying a changeset: the approved operations run in order, each on its own, as the
//! proposer.
//!
//! The whole changeset applies in one transaction holding its row, so it applies once. Each
//! operation runs in a savepoint inside it: one the records refuse (an item deleted since
//! it was proposed, a link its credential cannot read) rolls back alone and is marked
//! `failed` with the reason, every operation depending on it is `skipped`, and the rest
//! apply. A fault inside Elysium rolls the whole changeset back instead.
//!
//! Elysium's own data is written here. The provider writes that follow from it, such as
//! closing a resolved item's issues, posting a comment to its primary link, or filing an item's
//! own issue, are owed in the same transaction, like the user's own writes, and the watcher
//! lands them afterwards.
//!
//! A changeset a person applies is reviewed: an item it creates starts `open`, since approving
//! it accepts it. A Project Manager's changeset that its project lets apply as it is made
//! (`crate::tools::work::propose`) is applied by the proposer's own machine, unreviewed, so an
//! item it creates starts in the inbox for the person to accept. Every operation a Project
//! Manager's changeset applies is recorded on its project's timeline with the record that caused
//! it (`crate::project_managers::timeline::record_write`), in the operation's own savepoint.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    ChangesetDecision, ChangesetOperation, ChangesetOutcome, ChangesetState, Settled, Staged,
    Touched, Written, conclude, lock, operations_of, settle,
};
use crate::action_items::changesets::{
    self, AddComment, AddLink, AssignItem, CreateInitiative, CreateItem, FileIssue, LinkReads,
    Membership, Operation, Proposer, Target, UpdateItem,
};
use crate::action_items::{Actor, Transition, WorkError};
use crate::models::action_item::{self, ActionItemChanges, ActionItemState, NewActionItem, Owner};
use crate::models::action_item_assignment::{self, Assigner};
use crate::models::action_item_event::{self, HistoryEntry, HistoryKind};
use crate::models::action_item_issue_filing::{self, NewFiling};
use crate::models::action_item_link::Claim;
use crate::models::initiative::{self, NewInitiative};
use crate::models::{action_item_comment, action_item_link, action_item_link_write};
use crate::project_managers::timeline::{self, Write};

/// Why one operation did not apply.
#[derive(Debug)]
enum Failure {
    /// The records refuse it, in words for the user; the rest of the changeset applies.
    Refused(String),
    /// A fault inside Elysium, which rolls the whole changeset back.
    Fault(DieselError),
}

impl From<DieselError> for Failure {
    fn from(error: DieselError) -> Self {
        match error {
            DieselError::NotFound => Self::Refused("what it names no longer exists".to_owned()),
            DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _) => {
                Self::Refused("it names a project that no longer exists".to_owned())
            }
            other => Self::Fault(other),
        }
    }
}

impl From<WorkError> for Failure {
    fn from(error: WorkError) -> Self {
        match error {
            WorkError::Database(database) => database.into(),
            WorkError::Conflict(message) | WorkError::Invalid(message) => {
                Self::Refused(message.to_owned())
            }
            WorkError::Refused(message) => Self::Refused(message),
        }
    }
}

/// What applying one operation did.
#[derive(Debug, Default)]
struct Done {
    /// The ids it acted on or created, and the values it replaced, which undo reads.
    result: Value,
    /// The item or initiative it created, which later operations may name.
    created: Option<Uuid>,
    /// The filing it owed, which the timeline record of the write is pointed at.
    filing: Option<Uuid>,
    touched: Touched,
}

impl Done {
    /// An operation's result, having written to `item_id` and recorded `history`.
    fn on_item(result: Value, item_id: Uuid, history: Vec<HistoryEntry>) -> Self {
        let mut done = Self {
            result,
            ..Self::default()
        };
        done.touched.item_ids.insert(item_id);
        done.touched.history = history;
        done
    }
}

/// One changeset being applied: what its operations share, and what they have done so far.
struct Run {
    changeset_id: Uuid,
    proposer: Proposer,
    /// The proposer, whom applied changes record as their actor.
    actor: Actor,
    /// Who decided the changeset: a person, or the proposer's own machine. A person accepting
    /// Elysia's suggested assignment is who assigned the item.
    decided_by: Uuid,
    /// Whether a person applied it, approving each operation, rather than the proposer's own
    /// machine applying a Project Manager's changes as they were made.
    reviewed: bool,
    /// Whether a link the changeset adds may become its item's primary: Elysia's may, as
    /// the user's first link does. A coding agent's never does, so linking the pull request
    /// it opened never moves whose list an item is on.
    claim: Claim,
    now: DateTime<Utc>,
    reads: LinkReads,
    /// The records applied operations created, by position.
    created: HashMap<u32, Uuid>,
    /// Each operation's outcome so far, in order.
    outcomes: Vec<ChangesetOutcome>,
    touched: Touched,
}

impl Run {
    /// The id a target names.
    ///
    /// # Panics
    /// Panics if it names an operation that created nothing. An operation runs only once
    /// every operation it depends on applied, so that is a bug.
    fn resolve(&self, target: Target) -> Uuid {
        match target {
            Target::Existing(existing) => existing.id,
            Target::Proposed(proposed) => *self
                .created
                .get(&proposed.operation)
                .expect("an operation runs only after the ones it names applied"),
        }
    }

    /// Applies the next operation if it is approved and everything it depends on applied,
    /// and answers how it ended.
    async fn settle_next(
        &mut self,
        connection: &mut AsyncPgConnection,
        row: &ChangesetOperation,
        depends_on: &[u32],
    ) -> Result<Settled, WorkError> {
        if row.decision == ChangesetDecision::Rejected {
            return Ok(Settled::skipped(None));
        }
        let blocker = depends_on.iter().find(|&&dependency| {
            self.outcomes[dependency as usize - 1] != ChangesetOutcome::Applied
        });
        if let Some(blocker) = blocker {
            return Ok(Settled::skipped(Some(format!(
                "operation {blocker} was not applied"
            ))));
        }

        let run = &*self;
        let operation = row.parsed();
        let attempt = connection
            .transaction(async move |connection| {
                let mut done = apply_one(connection, run, row.id, operation.clone()).await?;
                let history = std::mem::take(&mut done.touched.history);
                done.touched.history =
                    action_item_event::attribute(connection, history, run.changeset_id).await?;
                if let Proposer::ProjectManager(project_id) = run.proposer {
                    record_write(connection, run, project_id, row, &operation, &done).await?;
                }
                Ok::<_, Failure>(done)
            })
            .await;
        match attempt {
            Ok(done) => {
                if let Some(record_id) = done.created {
                    self.created.insert(row.number(), record_id);
                }
                self.touched.absorb(done.touched);
                Ok(Settled {
                    outcome: ChangesetOutcome::Applied,
                    error: None,
                    result: done.result,
                })
            }
            Err(Failure::Refused(message)) => Ok(Settled {
                outcome: ChangesetOutcome::Failed,
                error: Some(message),
                result: json!({}),
            }),
            Err(Failure::Fault(error)) => Err(WorkError::Database(error)),
        }
    }
}

impl Settled {
    fn skipped(error: Option<String>) -> Self {
        Self {
            outcome: ChangesetOutcome::Skipped,
            error,
            result: json!({}),
        }
    }
}

/// Applies the approved operations of a pending changeset whose operations are all decided,
/// for `decided_by`: a person, or the proposer's own machine applying its changes unreviewed.
/// Each operation still acts as the proposer.
/// `reads` holds what each approved link operation's target read as
/// ([`changesets::read_link_targets`]). A changeset with none approved is `rejected`.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown changeset,
/// [`WorkError::Conflict`] for one that is not pending, [`WorkError::Refused`] while any
/// operation is undecided, and any database fault, which applies nothing.
pub async fn apply(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    reads: LinkReads,
    decided_by: Uuid,
    now: DateTime<Utc>,
) -> Result<Written, WorkError> {
    connection
        .transaction(async move |connection| {
            let changeset = lock(connection, id).await?;
            if changeset.state != ChangesetState::Pending {
                return Err(WorkError::Conflict(
                    "the changeset was already applied or rejected",
                ));
            }
            let operations = operations_of(connection, &[id])
                .await?
                .remove(&id)
                .unwrap_or_default();
            let undecided: Vec<String> = operations
                .iter()
                .filter(|row| row.decision == ChangesetDecision::Pending)
                .map(|row| row.position.to_string())
                .collect();
            if !undecided.is_empty() {
                return Err(WorkError::Refused(format!(
                    "approve or reject every operation before applying; still undecided: {}",
                    undecided.join(", ")
                )));
            }

            let proposer = Proposer::parse(&changeset.proposer)
                .expect("changesets_proposer_shape admits only proposers");
            let parsed: Vec<Operation> =
                operations.iter().map(ChangesetOperation::parsed).collect();
            let dependencies = changesets::dependencies(&parsed);
            let mut run = Run {
                changeset_id: id,
                proposer,
                actor: proposer.actor(),
                decided_by,
                reviewed: decided_by != proposer.actor().user_id(),
                claim: if proposer == Proposer::Elysia {
                    Claim::Primary
                } else {
                    Claim::Secondary
                },
                now,
                reads,
                created: HashMap::new(),
                outcomes: Vec::with_capacity(operations.len()),
                touched: Touched::default(),
            };

            let mut settled_rows = Vec::with_capacity(operations.len());
            for (row, depends_on) in operations.iter().zip(&dependencies) {
                let settled = run.settle_next(connection, row, depends_on).await?;
                run.outcomes.push(settled.outcome);
                settled_rows.push(settle(connection, row.id, settled).await?);
            }

            let any_approved = settled_rows
                .iter()
                .any(|row| row.decision == ChangesetDecision::Approved);
            let state = if any_approved {
                ChangesetState::Applied
            } else {
                ChangesetState::Rejected
            };
            let changeset = conclude(connection, id, state, decided_by, now).await?;
            Ok(Written {
                staged: Staged {
                    changeset,
                    operations: settled_rows,
                },
                touched: run.touched,
            })
        })
        .await
}

/// Applies one operation as the proposer.
async fn apply_one(
    connection: &mut AsyncPgConnection,
    run: &Run,
    operation_id: Uuid,
    operation: Operation,
) -> Result<Done, Failure> {
    match operation {
        Operation::CreateItem(create) => create_item(connection, run, create).await,
        Operation::UpdateItem(update) => update_item(connection, run, update).await,
        Operation::ResolveItem(finish) => {
            transition(connection, run, finish.item, Transition::Resolve).await
        }
        Operation::DismissItem(finish) => {
            transition(connection, run, finish.item, Transition::Dismiss).await
        }
        Operation::Comment(comment) => add_comment(connection, run, comment).await,
        Operation::Link(link) => add_link(connection, run, operation_id, link).await,
        Operation::AddToInitiative(membership) => {
            change_membership(connection, run, membership, true).await
        }
        Operation::RemoveFromInitiative(membership) => {
            change_membership(connection, run, membership, false).await
        }
        Operation::CreateInitiative(create) => create_initiative(connection, run, create).await,
        Operation::FileIssue(file) => file_issue(connection, run, operation_id, file).await,
        Operation::AssignItem(assign) => assign_item(connection, run, assign).await,
    }
}

/// Assigns the item to its lead project's Project Manager as the proposer, for the person who
/// approved it. Only Elysia proposes assignments, and only a person decides hers, so the decider is
/// always a person here. Keeps whether it changed anything, so an undo takes back only an
/// assignment it made.
async fn assign_item(
    connection: &mut AsyncPgConnection,
    run: &Run,
    assign: AssignItem,
) -> Result<Done, Failure> {
    let id = run.resolve(assign.item);
    let assigner = Assigner {
        actor: run.actor,
        person: run.decided_by,
    };
    let written = action_item_assignment::assign(connection, id, assigner, run.now).await?;
    let made = written
        .history
        .iter()
        .any(|entry| entry.kind == HistoryKind::ManagerAssigned.as_str());
    // When it applied, so an undo takes back this assignment and not one a person made since.
    let result = json!({
        "itemId": id,
        "assigned": made,
        "projectId": written.record.lead_project_id,
        "assignedAt": written.record.manager_assigned_at,
    });
    Ok(Done::on_item(result, id, written.history))
}

/// Creates an item, the user's: open when a person approved it, which accepts it, and in the
/// inbox when it applied unreviewed, for the person to accept.
async fn create_item(
    connection: &mut AsyncPgConnection,
    run: &Run,
    create: CreateItem,
) -> Result<Done, Failure> {
    let initiative_ids = sorted_unique(
        create
            .initiatives
            .into_iter()
            .map(|target| run.resolve(target))
            .collect(),
    );
    let state = if run.reviewed {
        ActionItemState::Open
    } else {
        ActionItemState::Inbox
    };
    let new_item = NewActionItem {
        title: create.title,
        notes: create.notes,
        state,
        priority: create.priority,
        due_at: create.due_at,
        owner: Owner::User,
        project_ids: sorted_unique(create.project_ids),
        initiative_ids: initiative_ids.clone(),
    };
    let created = action_item::create(connection, new_item, run.actor, run.now).await?;
    let id = created.record.id;
    let mut done = Done::on_item(json!({ "itemId": id }), id, created.history);
    done.created = Some(id);
    done.touched.initiative_ids.extend(initiative_ids);
    Ok(done)
}

/// Replaces the fields an update names, keeping each one's value before and after for undo.
async fn update_item(
    connection: &mut AsyncPgConnection,
    run: &Run,
    update: UpdateItem,
) -> Result<Done, Failure> {
    let id = run.resolve(update.item);
    let changes = ActionItemChanges {
        title: update.title,
        notes: update.notes,
        priority: update.priority,
        due_at: update.due_at,
        ..ActionItemChanges::default()
    };
    let updated = action_item::update(connection, id, changes, run.actor, run.now).await?;
    // The entry's `changes` hold each field's value before and after, which undo puts back;
    // an update that changed nothing recorded no entry.
    let changes = updated
        .history
        .first()
        .map_or_else(|| json!({}), |entry| entry.data["changes"].clone());
    Ok(Done::on_item(
        json!({ "itemId": id, "changes": changes }),
        id,
        updated.history,
    ))
}

/// Resolves or dismisses an item, keeping the state it left for an undo to return it to.
/// Resolving owes a close to its linked issues, like any resolve.
async fn transition(
    connection: &mut AsyncPgConnection,
    run: &Run,
    item: Target,
    transition: Transition,
) -> Result<Done, Failure> {
    let id = run.resolve(item);
    let moved = action_item::transition(connection, id, transition, run.actor, run.now).await?;
    let from = moved
        .history
        .first()
        .map_or(Value::Null, |entry| entry.data["from"].clone());
    let mut done = Done::on_item(json!({ "itemId": id, "from": from }), id, moved.history);
    done.touched.link_item_ids.insert(id);
    Ok(done)
}

/// Writes the comment as the proposer, owed to the item's primary link like any comment.
async fn add_comment(
    connection: &mut AsyncPgConnection,
    run: &Run,
    comment: AddComment,
) -> Result<Done, Failure> {
    let id = run.resolve(comment.item);
    let written =
        action_item_comment::create(connection, id, comment.body, run.actor, run.now).await?;
    let comment_id = written.record.id;
    // The link the comment is owed to, if any, so an undo can tell whether it was posted
    // before the undo came.
    let owed_to = action_item_link_write::for_comment(connection, comment_id)
        .await?
        .map(|write| write.link_id);
    let result = json!({ "itemId": id, "commentId": comment_id, "linkId": owed_to });
    let mut done = Done::on_item(result, id, written.history);
    done.touched.link_item_ids.insert(id);
    done.touched.comments.push(written.record);
    Ok(done)
}

/// Links the item to the target read before the changeset began to apply, or fails with
/// the provider's answer when it could not be read.
async fn add_link(
    connection: &mut AsyncPgConnection,
    run: &Run,
    operation_id: Uuid,
    link: AddLink,
) -> Result<Done, Failure> {
    let id = run.resolve(link.item);
    // Targets are read for the operations approved when applying began; one approved in the
    // moment since has none, and fails rather than linking something unread.
    let read = run.reads.get(&operation_id).cloned().ok_or_else(|| {
        Failure::Refused(
            "it was approved while the changeset was being applied, so its target was not read"
                .to_owned(),
        )
    })?;
    let new_link = read.map_err(Failure::Refused)?;
    let linked =
        action_item_link::add(connection, id, new_link, run.claim, run.actor, run.now).await?;
    let link_id = linked.links.first().map(|linked| linked.id);
    // Linking something already linked to the item changes nothing, and so leaves nothing
    // for an undo to remove.
    let added = !linked.item.history.is_empty();
    let result = json!({ "itemId": id, "linkId": link_id, "added": added });
    let mut done = Done::on_item(result, id, linked.item.history);
    done.touched.link_item_ids.insert(id);
    Ok(done)
}

/// Puts the item into the initiative, or takes it out, keeping whether that changed
/// anything, so an undo reverses only a real change.
async fn change_membership(
    connection: &mut AsyncPgConnection,
    run: &Run,
    membership: Membership,
    joins: bool,
) -> Result<Done, Failure> {
    let id = run.resolve(membership.item);
    let initiative_id = run.resolve(membership.initiative);
    let span_before = action_item::latest_span(connection, id, initiative_id)
        .await?
        .map(|(span_id, _left_at)| span_id);
    let written = if joins {
        action_item::join_initiative(connection, id, initiative_id, run.actor, run.now).await?
    } else {
        action_item::leave_initiative(connection, id, initiative_id, run.actor, run.now).await?
    };
    let changed = !written.history.is_empty();
    // The item's latest span before and after: the one this opened or closed, and the one it
    // replaced. An undo reverses the change only while the span after is still the latest, so a
    // move the user made since stands; see `crate::models::changeset::undo`.
    let span_id = if changed {
        action_item::latest_span(connection, id, initiative_id)
            .await?
            .map(|(span_id, _left_at)| span_id)
    } else {
        None
    };
    let result = json!({
        "itemId": id,
        "initiativeId": initiative_id,
        "changed": changed,
        "spanId": span_id,
        "spanBefore": span_before,
    });
    let mut done = Done::on_item(result, id, written.history);
    done.touched.initiative_ids.insert(initiative_id);
    Ok(done)
}

/// Creates an initiative, active, in the projects it names.
async fn create_initiative(
    connection: &mut AsyncPgConnection,
    run: &Run,
    create: CreateInitiative,
) -> Result<Done, Failure> {
    let new_initiative = NewInitiative {
        name: create.name,
        description: create.description,
        target_at: create.target_at,
        project_ids: sorted_unique(create.project_ids),
    };
    let created = initiative::create(connection, new_initiative, run.actor, run.now).await?;
    let id = created.record.id;
    let mut done = Done {
        result: json!({ "initiativeId": id }),
        created: Some(id),
        ..Done::default()
    };
    done.touched.initiative_ids.insert(id);
    done.touched.history = created.history;
    Ok(done)
}

/// Owes the item's own issue in the tracker the operation names, for the watcher to open and link
/// as its primary. Only an item still to do, with no primary link, gets one: an item already
/// following an issue has its comments and status there.
async fn file_issue(
    connection: &mut AsyncPgConnection,
    run: &Run,
    operation_id: Uuid,
    file: FileIssue,
) -> Result<Done, Failure> {
    let id = run.resolve(file.item);
    let item = action_item::lock_live(connection, id).await?;
    if !matches!(item.state, ActionItemState::Inbox | ActionItemState::Open) {
        return Err(Failure::Refused(
            "the item is already resolved or dismissed, so it needs no issue".to_owned(),
        ));
    }
    let links = action_item_link::list_for_item(connection, id).await?;
    if links.iter().any(|link| link.is_primary) {
        return Err(Failure::Refused(
            "the item already follows a linked issue, where its comments and status go".to_owned(),
        ));
    }
    let filing = action_item_issue_filing::owe(
        connection,
        NewFiling {
            action_item_id: id,
            credential: file.tracker.credential(),
            destination: file.tracker.destination,
            filed_by: run.proposer,
            changeset_operation_id: Some(operation_id),
            timeline_record_id: None,
        },
        run.now,
    )
    .await?;
    let mut done = Done::on_item(
        json!({ "itemId": id, "filingId": filing.id }),
        id,
        Vec::new(),
    );
    done.filing = Some(filing.id);
    done.touched.link_item_ids.insert(id);
    Ok(done)
}

/// Records one applied operation of a Project Manager's changeset on its project's timeline,
/// with the record that caused it, and points the filing it owed, if any, at that record.
async fn record_write(
    connection: &mut AsyncPgConnection,
    run: &Run,
    project_id: Uuid,
    row: &ChangesetOperation,
    operation: &Operation,
    done: &Done,
) -> Result<(), Failure> {
    let item_id: Option<Uuid> = serde_json::from_value(done.result["itemId"].clone()).ok();
    let title = describe(connection, operation, item_id).await?;
    let record = timeline::record_write(
        connection,
        Write {
            project_id,
            changeset_id: run.changeset_id,
            operation_id: row.id,
            kind: row.operation["kind"].as_str().unwrap_or_default(),
            title,
            reason: &row.reason,
            quote: row.quote.as_deref(),
            cause_record_id: row.cause_record_id,
            item_id,
            reviewed: run.reviewed,
            applied_at: run.now,
        },
    )
    .await?;
    if let Some(filing_id) = done.filing {
        action_item_issue_filing::set_timeline_record(connection, filing_id, record.id).await?;
    }
    Ok(())
}

/// What an applied operation did, in a line for its timeline record: `Resolved: Fix the login
/// page`, or `Filed in Jira ELY: Fix the login page`.
async fn describe(
    connection: &mut AsyncPgConnection,
    operation: &Operation,
    item_id: Option<Uuid>,
) -> Result<String, Failure> {
    let item_title = match item_id {
        Some(id) => action_item::find(connection, id).await?.title,
        None => String::new(),
    };
    let initiative_name = async |connection: &mut AsyncPgConnection, target: Target| {
        let Target::Existing(existing) = target else {
            return Ok::<_, Failure>(String::new());
        };
        Ok(initiative::find(connection, existing.id).await?.name)
    };
    let line = match operation {
        Operation::CreateItem(_) => format!("Created an item: {item_title}"),
        Operation::UpdateItem(_) => format!("Updated: {item_title}"),
        Operation::ResolveItem(_) => format!("Resolved: {item_title}"),
        Operation::DismissItem(_) => format!("Dismissed: {item_title}"),
        Operation::Comment(_) => format!("Commented on: {item_title}"),
        Operation::Link(link) => format!("Linked {}: {item_title}", link.target.reference),
        Operation::AddToInitiative(membership) => {
            let name = initiative_name(connection, membership.initiative).await?;
            format!("Added to {name}: {item_title}")
        }
        Operation::RemoveFromInitiative(membership) => {
            let name = initiative_name(connection, membership.initiative).await?;
            format!("Taken out of {name}: {item_title}")
        }
        Operation::CreateInitiative(create) => format!("Created an initiative: {}", create.name),
        Operation::FileIssue(file) => format!(
            "Filed in {} {}: {item_title}",
            file.tracker.provider.name(),
            file.tracker.destination
        ),
        Operation::AssignItem(_) => format!("Assigned to its Project Manager: {item_title}"),
    };
    Ok(line.trim_end_matches([':', ' ']).to_owned())
}

/// Sorts ids and drops repeats, as the model's creates expect.
fn sorted_unique(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort_unstable();
    ids.dedup();
    ids
}
