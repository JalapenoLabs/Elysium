// Copyright © 2026 Jalapeno Labs

//! What a Project Manager puts on its project's timeline: each turn that did its work, each
//! batch it submits, and each change it makes ([`record_write`]), so the project reads its
//! decisions in order beside the meetings and messages that caused them.
//!
//! Each is a timeline record of kind [`SOURCE_KIND`], owned by the Project Managers' machine user,
//! so everyone who reads the project's timeline reads it (`crate::models::timeline_record`,
//! `SHARED_OWNERS`), attributed to its project as certain, and indexed for search like every
//! record. Its external id names the turn, batch, or changeset operation, so it is recorded
//! once.

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde_json::json;
use uuid::Uuid;

use crate::models::attribution::{self, Attributor};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::{Priority, WorkBatch};

/// The kind every Project Manager's record is.
pub const SOURCE_KIND: &str = "project_manager";

/// Who a Project Manager's records say they came from.
pub const ACTOR: &str = "Project Manager";

/// The most of a record's title a summary or batch title fills, matching what a timeline shows.
const TITLE_MAX_CHARACTERS: usize = 300;

/// A finished turn of `project_id`'s Project Manager, as a record: headed by its summary, holding
/// its reply. Call it in the transaction that finishes the turn.
///
/// # Errors
/// Propagates any database error.
pub async fn record_turn(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    turn_id: Uuid,
    summary: &str,
    reply: Option<&str>,
    finished_at: DateTime<Utc>,
) -> diesel::QueryResult<TimelineRecord> {
    let record = NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SOURCE_KIND.to_owned(),
        connector_id: None,
        external_id: Some(format!("turn:{turn_id}")),
        occurred_at: finished_at,
        actor: ACTOR.to_owned(),
        title: summary.chars().take(TITLE_MAX_CHARACTERS).collect(),
        body: reply.unwrap_or_default().to_owned(),
        url: None,
        media: None,
        metadata: json!({ "kind": "turn", "turnId": turn_id }),
        created_by: PROJECT_MANAGER_USER_ID,
    };
    store(connection, project_id, record).await
}

/// Batch `batch` as it was submitted, as a record: its title, priority, the goal in the brief it
/// serves, and its goal, said to come from `actor`, its Project Manager ([`ACTOR`]) or the person
/// who accepted the finding it was made from. Call it in the transaction that submits the batch.
///
/// # Errors
/// Propagates any database error.
pub async fn record_batch(
    connection: &mut AsyncPgConnection,
    batch: &WorkBatch,
    actor: &str,
) -> diesel::QueryResult<TimelineRecord> {
    let priority = match batch.priority {
        Priority::Asap => "asap",
        Priority::Normal => "normal",
        Priority::Scheduled => "scheduled",
    };
    let record = NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SOURCE_KIND.to_owned(),
        connector_id: None,
        external_id: Some(format!("batch:{}", batch.id)),
        occurred_at: batch.created_at,
        actor: actor.to_owned(),
        title: batch.title.chars().take(TITLE_MAX_CHARACTERS).collect(),
        body: batch.goal.clone(),
        url: None,
        media: None,
        metadata: json!({
            "kind": "batch",
            "batchId": batch.id,
            "turnId": batch.turn_id,
            "priority": priority,
            "scheduledFor": batch.scheduled_for,
            "serves": batch.serves,
        }),
        created_by: PROJECT_MANAGER_USER_ID,
    };
    store(connection, batch.project_id, record).await
}

/// One change a Project Manager made, as its record tells it.
#[derive(Debug, Clone)]
pub struct Write<'write> {
    pub project_id: Uuid,
    pub changeset_id: Uuid,
    pub operation_id: Uuid,
    /// The operation's kind, such as `file-issue`.
    pub kind: &'write str,
    /// What it did, in a line: `Filed in Jira ELY: Fix the login page`.
    pub title: String,
    pub reason: &'write str,
    pub quote: Option<&'write str>,
    /// The segment or message that caused it.
    pub cause_record_id: Option<Uuid>,
    /// The task heard in a meeting that caused it.
    pub meeting_task_id: Option<Uuid>,
    /// The item it wrote, when it wrote one.
    pub item_id: Option<Uuid>,
    /// Whether a person approved it before it applied, rather than the project letting its
    /// Project Manager write directly.
    pub reviewed: bool,
    pub applied_at: DateTime<Utc>,
}

/// A change a Project Manager made, as a record: headed by what it did, holding why, and naming
/// the record that caused it, so the project's timeline shows each write beside its cause. Call
/// it in the transaction that applies the change.
///
/// # Errors
/// Propagates any database error.
pub async fn record_write(
    connection: &mut AsyncPgConnection,
    write: Write<'_>,
) -> diesel::QueryResult<TimelineRecord> {
    let body = match write.quote {
        Some(quote) => format!("{}\n\n> {quote}", write.reason),
        None => write.reason.to_owned(),
    };
    let record = NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SOURCE_KIND.to_owned(),
        connector_id: None,
        external_id: Some(format!("write:{}", write.operation_id)),
        occurred_at: write.applied_at,
        actor: ACTOR.to_owned(),
        title: write.title.chars().take(TITLE_MAX_CHARACTERS).collect(),
        body,
        url: None,
        media: None,
        metadata: json!({
            "kind": "write",
            "write": write.kind,
            "changesetId": write.changeset_id,
            "operationId": write.operation_id,
            "itemId": write.item_id,
            "causeRecordId": write.cause_record_id,
            "meetingTaskId": write.meeting_task_id,
            "reviewed": write.reviewed,
        }),
        created_by: PROJECT_MANAGER_USER_ID,
    };
    store(connection, write.project_id, record).await
}

/// Stores `record` and attributes it to `project_id`, which wakes nothing: it is the Project
/// Manager's own.
async fn store(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    record: NewTimelineRecord,
) -> diesel::QueryResult<TimelineRecord> {
    // Its connector is none, so no earlier record conflicts with it; one always comes back.
    let stored = timeline_record::insert_new(connection, record)
        .await?
        .ok_or(diesel::result::Error::NotFound)?;
    attribution::attribute(
        connection,
        stored.id,
        project_id,
        attribution::PERSON_CONFIDENCE,
        Attributor::ProjectManager,
    )
    .await?;
    Ok(stored)
}
