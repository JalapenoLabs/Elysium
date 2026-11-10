// Copyright © 2026 Jalapeno Labs

//! Assigning an action item to its lead project's Project Manager, which wakes it to plan the item.
//! See `docs/action-items.md`, Assigning to a Project Manager.
//!
//! # A person picks the work
//!
//! Attribution decides where an item belongs and wakes nobody (`crate::models::action_item_attribution`).
//! Assigning is a person deciding the item is to be worked: it wakes the lead project's Project
//! Manager with an `assignment` wake, in the same transaction, and starts no worker. The Project
//! Manager plans it as it plans anything, into a proposed batch a person approves
//! (`docs/engine.md`, Proposed batches). Only a person assigns: Elysia suggests assignments as
//! changeset operations, and the person accepting one is who assigned it ([`Assigner`]).
//!
//! # Always its lead's
//!
//! Only the lead project's Project Manager may submit work for an item, so an item is assigned to
//! that one and no other, and the assignment names no project of its own. An item with no lead
//! cannot be assigned. When the lead moves to another project, the assignment ends
//! ([`record_end`]), since nobody asked the new lead's Project Manager to plan it, and its wake, if
//! it still waits, goes with it. Deleting the item ends it too, so a restored item is not read as
//! planned when nothing planned it.
//!
//! # Where its plan stands
//!
//! [`plan`] reads it at the moment it is asked, from the assignment's wake and the batches naming
//! the item, so nothing is stored that could disagree with them.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::action_items::{Actor, Transition, WorkError};
use crate::database::schema::{
    action_items, project_manager_turns, project_manager_wakes, work_batches,
};
use crate::models::action_item::{self, ActionItem, ActionItemState};
use crate::models::action_item_event::{
    self, Change, HistoryEntry, HistoryKind, Recorded, Subject,
};
use crate::models::project_manager::{self, TurnStatus, WakeKind};
use crate::models::user;
use crate::models::work_batch::BatchState;

/// How many of the batches naming an item its plan lists, newest first. An item is one
/// commitment, so its Project Manager plans it in a few batches; more are on its project's page.
pub const PLAN_BATCHES_SHOWN: i64 = 10;

/// Who assigns an item: the actor its history names, and the person who decided. They differ
/// when a person accepts Elysia's suggestion, which applies as hers (`elysia`, naming the
/// changeset) while the person is who assigned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assigner {
    pub actor: Actor,
    pub person: Uuid,
}

impl Assigner {
    /// A person assigning an item themselves.
    pub const fn person(person: Uuid) -> Self {
        Self {
            actor: Actor::User(person),
            person,
        }
    }
}

/// Assigns live item `id` to its lead project's Project Manager and wakes it to plan the item.
/// An item in the inbox is accepted first, since assigning it is taking it on. Assigning an item
/// already assigned changes nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item, [`WorkError::Conflict`] for a
/// deleted, resolved, or dismissed one, or one that no project leads, [`WorkError::Refused`] when
/// the person is not one of the company's members, and any other database error.
pub async fn assign(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    assigner: Assigner,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            require_member(connection, assigner.person).await?;
            let item = action_item::lock_live(connection, id).await?;
            if matches!(
                item.state,
                ActionItemState::Resolved | ActionItemState::Dismissed
            ) {
                return Err(WorkError::Conflict(
                    "only an item in the inbox or open can be assigned to a Project Manager",
                ));
            }
            let Some(lead_project_id) = item.lead_project_id else {
                return Err(WorkError::Conflict(
                    "no project leads this item, so no Project Manager can plan it; add it to a \
                     project first",
                ));
            };
            if item.manager_assigned_at.is_some() {
                return Ok(Recorded {
                    record: item,
                    history: Vec::new(),
                });
            }

            let mut history = Vec::new();
            if item.state == ActionItemState::Inbox {
                let accepted = action_item::transition(
                    connection,
                    id,
                    Transition::Accept,
                    assigner.actor,
                    now,
                )
                .await?;
                history.extend(accepted.history);
            }
            let record = diesel::update(action_items::table.find(id))
                .set((
                    action_items::manager_assigned_at.eq(now),
                    action_items::manager_assigned_by.eq(assigner.person),
                ))
                .returning(ActionItem::as_returning())
                .get_result(connection)
                .await?;
            history.push(
                action_item_event::record(
                    connection,
                    Change {
                        subject: Subject::Item(id),
                        kind: HistoryKind::ManagerAssigned,
                        actor: assigner.actor,
                        data: json!({ "projectId": lead_project_id }),
                        at: now,
                    },
                )
                .await?,
            );
            // The wake names the person, not the actor: a batch planned from it answers a person.
            project_manager::wake_for_assignment(connection, lead_project_id, id, assigner.person)
                .await?;
            Ok(Recorded { record, history })
        })
        .await
}

/// Ends live item `id`'s assignment to its lead project's Project Manager, as `actor`, a person. A
/// wake for it no turn has taken yet goes too; batches already planned for it stay, each a person's
/// to decide. Unassigning an item that is not assigned changes nothing.
///
/// An assignment always has a lead while Elysium moves leads itself (`action_item::move_lead`
/// ends it), but one a stray delete left without a lead is still cleared here.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item, [`WorkError::Conflict`] for a
/// deleted one, [`WorkError::Refused`] when `actor` is not one of the company's members, and any
/// other database error.
pub async fn unassign(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            require_member(connection, actor.user_id()).await?;
            let item = action_item::lock_live(connection, id).await?;
            if item.manager_assigned_at.is_none() {
                return Ok(Recorded {
                    record: item,
                    history: Vec::new(),
                });
            }
            let record = diesel::update(action_items::table.find(id))
                .set((
                    action_items::manager_assigned_at.eq(None::<DateTime<Utc>>),
                    action_items::manager_assigned_by.eq(None::<Uuid>),
                ))
                .returning(ActionItem::as_returning())
                .get_result(connection)
                .await?;
            let entry = record_end(connection, &item, actor, Ending::TakenBack, now).await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Refuses `user_id` unless they are one of the company's members: a Project Manager's turns quote
/// what was ingested, so only members see Project Managers or give them work. The routes check
/// their caller as well; this also holds a changeset's decider, since its routes take any person.
async fn require_member(
    connection: &mut AsyncPgConnection,
    user_id: Uuid,
) -> Result<(), WorkError> {
    if user::find(connection, user_id).await?.is_company_member() {
        return Ok(());
    }
    Err(WorkError::Refused(
        "only the company's members assign items to Project Managers or take them back".to_owned(),
    ))
}

/// Why an assignment ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// A person took the item back, or deleted it, which takes it from its Project Manager too.
    TakenBack,
    /// The lead moved to another project, or to none, whose Project Manager nobody assigned it to.
    LeadMoved,
}

/// Records, as `actor`, the end of the locked `item`'s assignment to the Project Manager of the
/// project that led it as it was read, or to none when its lead was already gone, and drops the wake
/// no turn has taken yet. The caller clears the assignment in the update that ends it. Call it
/// inside that transaction.
///
/// # Errors
/// Propagates any database error.
pub async fn record_end(
    connection: &mut AsyncPgConnection,
    item: &ActionItem,
    actor: Actor,
    because: Ending,
    now: DateTime<Utc>,
) -> QueryResult<HistoryEntry> {
    diesel::delete(
        project_manager_wakes::table
            .filter(project_manager_wakes::kind.eq(WakeKind::Assignment))
            .filter(project_manager_wakes::action_item_id.eq(item.id))
            .filter(project_manager_wakes::turn_id.is_null()),
    )
    .execute(connection)
    .await?;
    let mut data = json!({ "projectId": item.lead_project_id });
    if because == Ending::LeadMoved {
        data["leadMoved"] = json!(true);
    }
    action_item_event::record(
        connection,
        Change {
            subject: Subject::Item(item.id),
            kind: HistoryKind::ManagerUnassigned,
            actor,
            data,
            at: now,
        },
    )
    .await
}

/// Where an assigned item's plan stands, from its Project Manager's side: waiting for a turn,
/// being planned, or planned in a turn that did or did not finish its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStage {
    /// The assignment waits for its Project Manager's next turn.
    Waiting,
    /// A turn of its Project Manager is planning it now.
    Planning,
    /// The turn that took it finished its work: what it submitted is in the plan's batches, and
    /// its reply says why when it submitted none.
    Planned,
    /// The turn that took it failed; its project's page says why.
    Failed,
}

/// What became of an assignment's wake: still waiting, or taken by a turn in this state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeTaken {
    Waiting,
    By(TurnStatus),
}

/// The stage a plan is at, from what became of its assignment's wake. An assignment always
/// writes a wake, but one cascaded away with its turn is read as planned: nothing waits for it.
pub fn stage(wake: Option<WakeTaken>) -> PlanStage {
    match wake {
        Some(WakeTaken::Waiting) => PlanStage::Waiting,
        Some(WakeTaken::By(TurnStatus::Running)) => PlanStage::Planning,
        Some(WakeTaken::By(TurnStatus::Succeeded)) | None => PlanStage::Planned,
        Some(WakeTaken::By(TurnStatus::Failed)) => PlanStage::Failed,
    }
}

/// A batch of the lead project naming the item, as its plan lists it.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanBatch {
    pub id: Uuid,
    pub title: String,
    pub state: BatchState,
    pub created_at: DateTime<Utc>,
}

/// Where an assigned item's plan stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The lead project, whose Project Manager it is assigned to.
    pub project_id: Uuid,
    pub assigned_at: DateTime<Utc>,
    pub assigned_by: Uuid,
    pub stage: PlanStage,
    /// The turn that took the assignment, once one did.
    pub turn_id: Option<Uuid>,
    /// The lead project's batches naming the item, newest first, at most [`PLAN_BATCHES_SHOWN`].
    pub batches: Vec<PlanBatch>,
}

/// Where `item`'s plan stands, or `None` while it is not assigned.
///
/// # Errors
/// Propagates any database error.
pub async fn plan(
    connection: &mut AsyncPgConnection,
    item: &ActionItem,
) -> QueryResult<Option<Plan>> {
    let (Some(assigned_at), Some(assigned_by), Some(project_id)) = (
        item.manager_assigned_at,
        item.manager_assigned_by,
        item.lead_project_id,
    ) else {
        return Ok(None);
    };

    // Each assignment writes a new wake and unassigning drops one still waiting, so the newest of
    // the item's wakes for the project is this assignment's.
    let wake: Option<(Option<Uuid>, Option<TurnStatus>)> = project_manager_wakes::table
        .left_join(project_manager_turns::table)
        .filter(project_manager_wakes::kind.eq(WakeKind::Assignment))
        .filter(project_manager_wakes::action_item_id.eq(item.id))
        .filter(project_manager_wakes::project_id.eq(project_id))
        // UUIDv7 ids break a tie between wakes written in one transaction, which share `now()`.
        .order((
            project_manager_wakes::created_at.desc(),
            project_manager_wakes::id.desc(),
        ))
        .select((
            project_manager_wakes::turn_id,
            project_manager_turns::status.nullable(),
        ))
        .first(connection)
        .await
        .optional()?;
    let taken = wake.map(|(_turn, status)| status.map_or(WakeTaken::Waiting, WakeTaken::By));
    let turn_id = wake.and_then(|(turn, _status)| turn);

    let batches = work_batches::table
        .filter(work_batches::project_id.eq(project_id))
        .filter(work_batches::action_item_ids.contains(json!([item.id])))
        .order(work_batches::created_at.desc())
        .limit(PLAN_BATCHES_SHOWN)
        .select((
            work_batches::id,
            work_batches::title,
            work_batches::state,
            work_batches::created_at,
        ))
        .load(connection)
        .await?;

    Ok(Some(Plan {
        project_id,
        assigned_at,
        assigned_by,
        stage: stage(taken),
        turn_id,
        batches,
    }))
}

/// Which of `item_ids` are assigned to `project_id`'s Project Manager now: live, in the inbox or
/// open, and led by the project. A turn leaves out an assignment that ended, moved with the lead,
/// or whose item was resolved or dismissed before it ran: finished work is not planned.
///
/// # Errors
/// Propagates any database error.
pub async fn assigned_among(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    item_ids: &[Uuid],
) -> QueryResult<Vec<ActionItem>> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }
    action_items::table
        .filter(action_items::id.eq_any(item_ids))
        .filter(action_items::lead_project_id.eq(project_id))
        .filter(action_items::manager_assigned_at.is_not_null())
        .filter(action_items::deleted_at.is_null())
        .filter(action_items::state.eq_any([ActionItemState::Inbox, ActionItemState::Open]))
        .order(action_items::manager_assigned_at.asc())
        .select(ActionItem::as_select())
        .load(connection)
        .await
}

#[cfg(test)]
mod tests;
