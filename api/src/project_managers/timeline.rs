// Copyright © 2026 Jalapeno Labs

//! What a Project Manager puts on its project's timeline: each turn that did its work, and each
//! batch it submits, so the project reads its decisions in order beside the meetings and
//! messages that caused them.
//!
//! Each is a timeline record of kind [`SOURCE_KIND`], owned by the Project Managers' machine user,
//! so everyone who reads the project's timeline reads it (`crate::models::timeline_record`,
//! `SHARED_OWNERS`), attributed to its project as certain, and indexed for search like every
//! record. Its external id names the turn or batch, so it is recorded once.

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
const ACTOR: &str = "Project Manager";

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

/// Batch `batch` as it was submitted, as a record: its title, priority, and goal. Call it in the
/// transaction that submits the batch.
///
/// # Errors
/// Propagates any database error.
pub async fn record_batch(
    connection: &mut AsyncPgConnection,
    batch: &WorkBatch,
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
        actor: ACTOR.to_owned(),
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
        }),
        created_by: PROJECT_MANAGER_USER_ID,
    };
    store(connection, batch.project_id, record).await
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
