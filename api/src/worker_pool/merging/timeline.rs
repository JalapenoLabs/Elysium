// Copyright © 2026 Jalapeno Labs

//! What a worker's merged pull request puts on its project's timeline: one record per merge,
//! whoever merged it, so the project reads its work landing beside the meetings and messages
//! that asked for it.
//!
//! The record is of kind [`SOURCE_KIND`], owned by the coding agent's machine user, so everyone
//! who reads the project's timeline reads it (`crate::models::timeline_record`, `SHARED_OWNERS`),
//! and attributed to the batch's project as certain by the batch's worker (`worker:<batch>`),
//! which wakes the project's Project Manager (`crate::models::attribution`). Elysia's intake
//! leaves it out, as it does every agent's record: the Project Manager is the one it is news for.
//!
//! Its title is the worker's, written from its batch, so it names the meetings the batch came from
//! (`crate::models::meeting_reads`): those the Project Manager's turn that submitted it had read,
//! and those of the records it names, the excerpts its worker read among them. Only whoever may
//! read every one of them reads it.

use diesel_async::AsyncPgConnection;
use serde_json::json;
use uuid::Uuid;

use crate::models::attribution::{self, Attributor};
use crate::models::meeting_reads::{self, Turn};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::models::user::CODING_AGENT_USER_ID;
use crate::models::work_batch::WorkBatch;
use crate::models::work_batch::pull_request::BatchPullRequest;

/// The kind every merged pull request's record is.
pub const SOURCE_KIND: &str = "merged_pull_request";

/// Who a merge Elysium made is said to come from.
const ELYSIUM_ACTOR: &str = "Elysium";

/// The most of a record's title a pull request's title fills, matching what a timeline shows.
const TITLE_MAX_CHARACTERS: usize = 300;

/// `pull_request`, which merged, as a record on `batch`'s project's timeline. Call it in the
/// transaction that records the merge (`pull_request::mark_merged`), so it is put there once.
///
/// # Errors
/// Propagates any database error.
pub async fn record_merge(
    connection: &mut AsyncPgConnection,
    batch: &WorkBatch,
    pull_request: &BatchPullRequest,
) -> diesel::QueryResult<TimelineRecord> {
    let merged_by = pull_request.merged_by.as_deref().unwrap_or_default();
    let (actor, how) = match pull_request.merge_method {
        Some(method) => (
            ELYSIUM_ACTOR.to_owned(),
            format!(
                "Elysium merged it ({}) once every check passed and no review was left open.",
                method.as_str()
            ),
        ),
        None => (
            merged_by.to_owned(),
            format!("{merged_by} merged it on GitHub."),
        ),
    };
    let title = format!(
        "Merged {}: {}",
        pull_request.reference(),
        pull_request.title
    );
    let record = NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SOURCE_KIND.to_owned(),
        connector_id: None,
        external_id: Some(format!("pull_request:{}", pull_request.id)),
        occurred_at: pull_request.merged_at.unwrap_or(pull_request.created_at),
        actor,
        title: title.chars().take(TITLE_MAX_CHARACTERS).collect(),
        body: format!("For the work batch \"{}\". {how}", batch.title),
        url: Some(pull_request.url.clone()),
        media: None,
        metadata: json!({
            "kind": "merged",
            "batchId": batch.id,
            "pullRequestId": pull_request.id,
            "repository": pull_request.repository,
            "number": pull_request.number,
            "mergedBy": pull_request.merged_by,
            "mergedByElysium": pull_request.merged_by_elysium,
            "mergeMethod": pull_request.merge_method,
        }),
        created_by: CODING_AGENT_USER_ID,
    };
    // Its connector is none, so no earlier record conflicts with it; one always comes back.
    let stored = timeline_record::insert_new(connection, record)
        .await?
        .ok_or(diesel::result::Error::NotFound)?;
    let mut came_from: Vec<Uuid> = meeting_reads::of_records(connection, &batch.source_records())
        .await?
        .into_values()
        .flatten()
        .collect();
    if let Some(turn_id) = batch.turn_id {
        came_from.extend(meeting_reads::of_turn(connection, Turn::ProjectManager(turn_id)).await?);
    }
    meeting_reads::name_on_record(connection, stored.id, &came_from).await?;
    attribution::attribute(
        connection,
        stored.id,
        batch.project_id,
        attribution::PERSON_CONFIDENCE,
        Attributor::Worker(batch.id),
    )
    .await?;
    Ok(stored)
}
