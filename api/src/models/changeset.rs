// Copyright © 2026 Jalapeno Labs

//! Changesets and their operations: what anyone but the user proposes, the user's decision
//! on each operation, and what applying and undoing them did.
//!
//! A changeset moves `pending` to `applied` (some operations approved) or `rejected` (none),
//! and an applied one to `undone`. Its operations are decided one by one while it is pending
//! ([`decide`]); [`apply`] then runs the approved ones in order and [`undo`] reverses them.
//! Every change an operation makes goes through the same model functions the user's own
//! writes use, recording history as the proposer, and the history it records names the
//! changeset as its source (`action_item_event::attribute`).
//!
//! The rules that need no database, such as how operations depend on each other, are in
//! `crate::action_items::changesets`. See `docs/action-items.md`.

mod apply;
mod undo;

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub use self::apply::apply;
pub use self::undo::undo;
use crate::action_items::WorkError;
use crate::action_items::changesets::{
    self, CreateInitiative, CreateItem, Operation, Proposal, Subject, Target,
};
use crate::database::schema::{changeset_operations, changesets as changesets_table};
use crate::models::action_item_comment::Comment;
use crate::models::action_item_event::HistoryEntry;
use crate::models::action_item_link::ActionItemLink;
use crate::models::{action_item, initiative, project};

/// Where a changeset stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ChangesetState"]
#[serde(rename_all = "kebab-case")]
pub enum ChangesetState {
    /// Waiting for the user's review.
    Pending,
    /// The approved operations were applied.
    Applied,
    /// Every operation was rejected, so nothing was written.
    Rejected,
    /// Applied, then undone as a whole.
    Undone,
}

/// The user's decision on one operation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ChangesetDecision"]
#[serde(rename_all = "kebab-case")]
pub enum ChangesetDecision {
    Pending,
    Approved,
    Rejected,
}

/// What applying, and later undoing, did with one operation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ChangesetOutcome"]
#[serde(rename_all = "kebab-case")]
pub enum ChangesetOutcome {
    /// Not applied yet.
    Pending,
    Applied,
    /// Approved, but it could not be applied; `error` says why.
    Failed,
    /// Not applied: rejected, or it depends on an operation that was not applied.
    Skipped,
    /// Applied, then reversed by an undo.
    Undone,
}

/// A stored changeset.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = changesets_table, check_for_backend(diesel::pg::Pg))]
pub struct Changeset {
    pub id: Uuid,
    /// A [`changesets::Proposer`] in its stored form: `elysia` or `session:<number>`.
    pub proposer: String,
    /// The project a coding session proposed it in.
    pub project_id: Option<Uuid>,
    pub summary: String,
    pub state: ChangesetState,
    /// When it was applied or rejected.
    pub decided_at: Option<DateTime<Utc>>,
    pub undone_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A stored operation.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = changeset_operations, check_for_backend(diesel::pg::Pg))]
pub struct ChangesetOperation {
    pub id: Uuid,
    pub changeset_id: Uuid,
    /// From 1, in the order operations apply.
    pub position: i32,
    /// An [`Operation`] as it was validated.
    pub operation: Value,
    pub reason: String,
    pub quote: Option<String>,
    pub source: Option<String>,
    pub decision: ChangesetDecision,
    pub outcome: ChangesetOutcome,
    /// Why it failed or was skipped.
    pub error: Option<String>,
    /// What applying it did: the ids it created or acted on, and the values it replaced.
    pub result: Value,
    /// What undoing it did and could not do; `None` until the changeset is undone.
    pub undo: Option<Value>,
}

impl ChangesetOperation {
    /// The operation this row holds.
    ///
    /// # Panics
    /// Panics if the stored JSON is not an [`Operation`]. Every row was validated as one
    /// when it was stored, so that means a kind this version no longer reads.
    pub fn parsed(&self) -> Operation {
        serde_json::from_value(self.operation.clone())
            .expect("changeset operations are validated before they are stored")
    }

    /// The position as the rules count it.
    ///
    /// # Panics
    /// Panics on a position below 1, which `changeset_operations_position_positive` refuses.
    pub fn number(&self) -> u32 {
        u32::try_from(self.position).expect("positions are positive")
    }
}

/// A changeset with its operations in order.
#[derive(Debug, Clone)]
pub struct Staged {
    pub changeset: Changeset,
    pub operations: Vec<ChangesetOperation>,
}

/// Everything applying or undoing a changeset changed, for the caller to tell clients.
#[derive(Debug, Default)]
pub struct Touched {
    /// Every history entry recorded, in the order it was written.
    pub history: Vec<HistoryEntry>,
    /// Items written; each is read again to publish it as it ended up.
    pub item_ids: BTreeSet<Uuid>,
    /// Initiatives written, created, or joined or left by an item.
    pub initiative_ids: BTreeSet<Uuid>,
    /// Items whose links, or the writes their links owe, changed.
    pub link_item_ids: BTreeSet<Uuid>,
    pub comments: Vec<Comment>,
    /// Comments an undo took back.
    pub withdrawn_comments: Vec<Comment>,
    /// Links an undo removed.
    pub removed_links: Vec<ActionItemLink>,
}

impl Touched {
    fn absorb(&mut self, other: Self) {
        self.history.extend(other.history);
        self.item_ids.extend(other.item_ids);
        self.initiative_ids.extend(other.initiative_ids);
        self.link_item_ids.extend(other.link_item_ids);
        self.comments.extend(other.comments);
        self.withdrawn_comments.extend(other.withdrawn_comments);
        self.removed_links.extend(other.removed_links);
    }
}

/// A changeset as applying or undoing it left it, with what that changed.
#[derive(Debug)]
pub struct Written {
    pub staged: Staged,
    pub touched: Touched,
}

#[derive(Insertable)]
#[diesel(table_name = changesets_table)]
struct ChangesetRow {
    id: Uuid,
    proposer: String,
    project_id: Option<Uuid>,
    summary: String,
    created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = changeset_operations)]
struct OperationRow {
    id: Uuid,
    changeset_id: Uuid,
    position: i32,
    operation: Value,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
}

/// Stores a proposal as a pending changeset, once it passes [`changesets::validate`] and
/// everything it names exists: every item and initiative live, every project there.
///
/// Checking scope, such as a coding session reaching only its own project, is the caller's.
///
/// # Errors
/// Returns [`WorkError::Refused`] naming the operation that cannot be stored, and any
/// database error.
pub async fn propose(
    connection: &mut AsyncPgConnection,
    proposal: Proposal,
    now: DateTime<Utc>,
) -> Result<Staged, WorkError> {
    if let Some(refusal) = changesets::validate(&proposal) {
        return Err(WorkError::Refused(refusal));
    }
    let id = Uuid::now_v7();
    let changeset = ChangesetRow {
        id,
        proposer: proposal.proposer.actor().to_string(),
        project_id: proposal.project_id,
        summary: proposal.summary,
        created_at: now,
    };
    let rows = proposal
        .operations
        .into_iter()
        .zip(1..)
        .map(|(proposed, position)| OperationRow {
            id: Uuid::now_v7(),
            changeset_id: id,
            position,
            operation: serde_json::to_value(&proposed.operation)
                .expect("an operation always serializes"),
            reason: proposed.reason,
            quote: proposed.quote,
            source: proposed.source,
        })
        .collect::<Vec<_>>();

    connection
        .transaction(async move |connection| {
            for row in &rows {
                let operation: Operation = serde_json::from_value(row.operation.clone())
                    .expect("an operation reads back what it wrote");
                if let Some(refusal) = missing_reference(connection, &operation).await? {
                    return Err(WorkError::Refused(format!(
                        "operation {}: {refusal}",
                        row.position
                    )));
                }
            }
            let changeset = diesel::insert_into(changesets_table::table)
                .values(changeset)
                .returning(Changeset::as_returning())
                .get_result(connection)
                .await?;
            let operations = diesel::insert_into(changeset_operations::table)
                .values(rows)
                .returning(ChangesetOperation::as_returning())
                .get_results(connection)
                .await?;
            Ok(Staged {
                changeset,
                operations,
            })
        })
        .await
}

/// Why an operation names something that is not there, if it does: an item or initiative
/// that does not exist or is deleted, or a project that does not exist.
async fn missing_reference(
    connection: &mut AsyncPgConnection,
    operation: &Operation,
) -> QueryResult<Option<String>> {
    for (target, subject) in operation.targets() {
        let Target::Existing(existing) = target else {
            continue;
        };
        let deleted_at = match subject {
            Subject::Item => action_item::find(connection, existing.id)
                .await
                .optional()?
                .map(|item| item.deleted_at),
            Subject::Initiative => initiative::find(connection, existing.id)
                .await
                .optional()?
                .map(|found| found.deleted_at),
        };
        if !matches!(deleted_at, Some(None)) {
            let named = match subject {
                Subject::Item => "action item",
                Subject::Initiative => "initiative",
            };
            return Ok(Some(format!(
                "{named} {} does not exist or is deleted",
                existing.id
            )));
        }
    }

    let (Operation::CreateItem(CreateItem { project_ids, .. })
    | Operation::CreateInitiative(CreateInitiative { project_ids, .. })) = operation
    else {
        return Ok(None);
    };
    let found = project::find_many(connection, project_ids).await?;
    let missing = project_ids
        .iter()
        .find(|&&id| !found.iter().any(|project| project.id == id));
    Ok(missing.map(|id| format!("project {id} does not exist")))
}

/// Loads a changeset for a write, locking its row until the transaction ends, so decisions,
/// applying, and undoing one changeset happen one after another.
async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Changeset> {
    changesets_table::table
        .find(id)
        .for_update()
        .select(Changeset::as_select())
        .first(connection)
        .await
}

/// The operations of `changeset_ids`, by changeset, each changeset's in order.
async fn operations_of(
    connection: &mut AsyncPgConnection,
    changeset_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<ChangesetOperation>>> {
    let rows: Vec<ChangesetOperation> = changeset_operations::table
        .filter(changeset_operations::changeset_id.eq_any(changeset_ids))
        .order((
            changeset_operations::changeset_id,
            changeset_operations::position,
        ))
        .select(ChangesetOperation::as_select())
        .load(connection)
        .await?;
    let mut by_changeset: HashMap<Uuid, Vec<ChangesetOperation>> = HashMap::new();
    for row in rows {
        by_changeset.entry(row.changeset_id).or_default().push(row);
    }
    Ok(by_changeset)
}

/// One changeset with its operations.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no changeset has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Staged> {
    let changeset = changesets_table::table
        .find(id)
        .select(Changeset::as_select())
        .first(connection)
        .await?;
    let operations = operations_of(connection, &[id])
        .await?
        .remove(&id)
        .unwrap_or_default();
    Ok(Staged {
        changeset,
        operations,
    })
}

/// The changesets in any of `states`, or every one for none, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    states: &[ChangesetState],
) -> QueryResult<Vec<Staged>> {
    let mut query = changesets_table::table.into_boxed();
    if !states.is_empty() {
        query = query.filter(changesets_table::state.eq_any(states.to_vec()));
    }
    let found: Vec<Changeset> = query
        .order(changesets_table::id.desc())
        .select(Changeset::as_select())
        .load(connection)
        .await?;
    let ids: Vec<Uuid> = found.iter().map(|changeset| changeset.id).collect();
    let mut operations = operations_of(connection, &ids).await?;
    Ok(found
        .into_iter()
        .map(|changeset| Staged {
            operations: operations.remove(&changeset.id).unwrap_or_default(),
            changeset,
        })
        .collect())
}

/// Records the user's `decision` on the operations `operation_ids` names, or on every one
/// for `None`, while the changeset is pending. Rejecting an operation rejects everything that
/// depends on it; see [`changesets::decide`].
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown changeset,
/// [`WorkError::Conflict`] for one that is no longer pending, [`WorkError::Refused`] for an
/// operation that is not in it or a decision the rules refuse, and any other database error.
pub async fn decide(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    decision: ChangesetDecision,
    operation_ids: Option<Vec<Uuid>>,
) -> Result<Staged, WorkError> {
    connection
        .transaction(async move |connection| {
            let changeset = lock(connection, id).await?;
            if changeset.state != ChangesetState::Pending {
                return Err(WorkError::Conflict(
                    "the changeset was already applied or rejected, so its decisions are final",
                ));
            }
            let mut operations = operations_of(connection, &[id])
                .await?
                .remove(&id)
                .unwrap_or_default();

            let chosen: BTreeSet<u32> = match operation_ids {
                None => operations.iter().map(ChangesetOperation::number).collect(),
                Some(operation_ids) => operation_ids
                    .iter()
                    .map(|operation_id| {
                        operations
                            .iter()
                            .find(|row| row.id == *operation_id)
                            .map(ChangesetOperation::number)
                            .ok_or_else(|| {
                                WorkError::Refused(format!(
                                    "operation {operation_id} is not in this changeset"
                                ))
                            })
                    })
                    .collect::<Result<_, _>>()?,
            };
            let parsed: Vec<Operation> =
                operations.iter().map(ChangesetOperation::parsed).collect();
            let dependencies = changesets::dependencies(&parsed);
            let mut decisions: Vec<ChangesetDecision> =
                operations.iter().map(|row| row.decision).collect();
            changesets::decide(&mut decisions, &dependencies, &chosen, decision)
                .map_err(WorkError::Refused)?;

            for (row, decided) in operations.iter_mut().zip(decisions) {
                if row.decision == decided {
                    continue;
                }
                diesel::update(changeset_operations::table.find(row.id))
                    .set(changeset_operations::decision.eq(decided))
                    .execute(connection)
                    .await?;
                row.decision = decided;
            }
            Ok(Staged {
                changeset,
                operations,
            })
        })
        .await
}

/// Moves a changeset on from pending or applied, stamping when: `decided_at` for applied
/// or rejected, `undone_at` for undone.
async fn conclude(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    state: ChangesetState,
    now: DateTime<Utc>,
) -> QueryResult<Changeset> {
    let update = diesel::update(changesets_table::table.find(id));
    if state == ChangesetState::Undone {
        return update
            .set((
                changesets_table::state.eq(state),
                changesets_table::undone_at.eq(now),
            ))
            .returning(Changeset::as_returning())
            .get_result(connection)
            .await;
    }
    update
        .set((
            changesets_table::state.eq(state),
            changesets_table::decided_at.eq(now),
        ))
        .returning(Changeset::as_returning())
        .get_result(connection)
        .await
}

/// What one operation ended as, written back to its row.
struct Settled {
    outcome: ChangesetOutcome,
    error: Option<String>,
    result: Value,
}

/// The longest error an operation keeps, matching its column.
const ERROR_MAX_CHARACTERS: usize = 2000;

/// Writes what applying one operation did.
async fn settle(
    connection: &mut AsyncPgConnection,
    operation_id: Uuid,
    settled: Settled,
) -> QueryResult<ChangesetOperation> {
    let error: Option<String> = settled
        .error
        .map(|error| error.chars().take(ERROR_MAX_CHARACTERS).collect());
    diesel::update(changeset_operations::table.find(operation_id))
        .set((
            changeset_operations::outcome.eq(settled.outcome),
            changeset_operations::error.eq(error),
            changeset_operations::result.eq(settled.result),
        ))
        .returning(ChangesetOperation::as_returning())
        .get_result(connection)
        .await
}

#[cfg(test)]
mod tests;
