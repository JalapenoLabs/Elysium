// Copyright © 2026 Jalapeno Labs

//! `/api/v1/meetings`: the signed-in person's meetings, how far each one's processing got, and
//! its transcripts with their correction. See `crate::meetings` and `docs/meetings.md`.
//!
//! A meeting belongs to whoever owns the account it was read from, and every route answers
//! them alone. Its transcript, raw and corrected, is served by its own route, for its page's
//! diff; what a meeting said reaches everyone else as its segments on the timeline.

mod approve_correction;
mod decide_duplicate;
mod get_duplicate;
mod get_meeting;
mod get_transcript;
mod hold_meeting;
mod list_meetings;
mod retry_meeting;
mod set_capture_offset;
mod split_capture;

use std::collections::HashMap;

use axum::Router;
use axum::routing::{get, post, put};
use chrono::{DateTime, Utc};
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::meetings::workflow;
use crate::models::meeting::Meeting;
use crate::models::meeting_capture::{self, MeetingCapture};
use crate::models::meeting_correction::{self, MeetingCorrection};
use crate::models::workflow_run::{self, WorkflowRun, WorkflowRunState};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_meetings::handle))
        .route("/{id}", get(get_meeting::handle))
        .route("/{id}/retry", post(retry_meeting::handle))
        .route("/{id}/transcript", get(get_transcript::handle))
        .route("/{id}/correction/approve", post(approve_correction::handle))
        .route(
            "/{id}/hold",
            post(hold_meeting::hold).delete(hold_meeting::release),
        )
        .route(
            "/{id}/captures/{capture_id}/offset",
            put(set_capture_offset::handle),
        )
        .route(
            "/{id}/captures/{capture_id}/split",
            post(split_capture::handle),
        )
        .route("/duplicates", get(get_duplicate::handle))
        .route("/duplicates/{id}/merge", post(decide_duplicate::merge))
        .route(
            "/duplicates/{id}/not-the-same",
            post(decide_duplicate::not_the_same),
        )
}

/// A meeting as its owner sees it. Where it came from (its connector, its source, its page there,
/// and whether a recording is kept) is its first capture's, the one a meeting holds until
/// captures can be merged.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingResponse {
    id: Uuid,
    connector_id: Option<Uuid>,
    source_kind: String,
    title: String,
    started_at: DateTime<Utc>,
    duration_ms: Option<i64>,
    organizer: String,
    participants: Value,
    url: Option<String>,
    /// Where its processing stands: `pending`, `recorded`, `segmented`, or `done`, or `failed`
    /// or `skipped` as its newest run says (`workflow::processing`).
    state: &'static str,
    /// Why its newest run failed, while it has.
    last_error: Option<String>,
    has_recording: bool,
    /// How many segments Elysia found, or `null` before she has read it.
    segment_count: Option<usize>,
    /// Whether its person asked to approve its corrected transcript before it is acted on.
    hold_for_approval: bool,
    /// Its corrected transcript, or `null` before it is corrected.
    correction: Option<CorrectionSummary>,
    /// Its newest run of the meeting workflow, or `null` before it has one.
    run_id: Option<Uuid>,
    /// Whether that run is held, waiting for its person to approve the corrected transcript.
    awaiting_approval: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// What a meeting's list says of its corrected transcript.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionSummary {
    changed_lines: i32,
    /// When its person approved it, and who did; `null` until then.
    approved_at: Option<DateTime<Utc>>,
    approved_by: Option<Uuid>,
    /// The inbox item raised to review it, while that item exists.
    review_item_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl From<&MeetingCorrection> for CorrectionSummary {
    fn from(correction: &MeetingCorrection) -> Self {
        Self {
            changed_lines: correction.changed_lines,
            approved_at: correction.approved_at,
            approved_by: correction.approved_by,
            review_item_id: correction.review_item_id,
            created_at: correction.created_at,
        }
    }
}

impl MeetingResponse {
    /// `meeting` as answered, with its first capture, its correction, and its newest run, which
    /// segments when `run_segments`.
    fn new(
        meeting: Meeting,
        capture: Option<&MeetingCapture>,
        correction: Option<&MeetingCorrection>,
        run: Option<&WorkflowRun>,
        run_segments: bool,
    ) -> Self {
        let processing =
            workflow::processing(meeting.state(), run.map(|run| run.state), run_segments);
        let last_error = run
            .filter(|_| processing == workflow::Processing::Failed)
            .and_then(|run| run.error.clone());
        Self {
            state: processing.as_str(),
            last_error,
            segment_count: meeting
                .segments
                .as_ref()
                .and_then(Value::as_array)
                .map(Vec::len),
            has_recording: capture.is_some_and(|capture| capture.recording_path.is_some()),
            correction: correction.map(CorrectionSummary::from),
            run_id: run.map(|run| run.id),
            awaiting_approval: run.is_some_and(|run| run.state == WorkflowRunState::Held),
            hold_for_approval: meeting.hold_for_approval,
            id: meeting.id,
            connector_id: capture.and_then(|capture| capture.connector_id),
            source_kind: capture
                .map(|capture| capture.source_kind.clone())
                .unwrap_or_default(),
            title: meeting.title,
            started_at: meeting.started_at,
            duration_ms: meeting.duration_ms,
            organizer: meeting.organizer,
            participants: meeting.participants,
            url: capture.and_then(|capture| capture.url.clone()),
            created_at: meeting.created_at,
            updated_at: meeting.updated_at,
        }
    }
}

/// `meetings` as answered, in their order, each with its first capture, its correction, and its
/// newest run, read in one query each rather than one per meeting.
///
/// # Errors
/// Propagates any database error.
pub async fn responses(
    connection: &mut AsyncPgConnection,
    meetings: Vec<Meeting>,
) -> QueryResult<Vec<MeetingResponse>> {
    let ids: Vec<Uuid> = meetings.iter().map(|meeting| meeting.id).collect();
    let source_ids: Vec<String> = ids.iter().map(Uuid::to_string).collect();
    let captures: HashMap<Uuid, MeetingCapture> = meeting_capture::first_of_each(connection, &ids)
        .await?
        .into_iter()
        .map(|capture| (capture.meeting_id, capture))
        .collect();
    let corrections: HashMap<Uuid, MeetingCorrection> =
        meeting_correction::find_many(connection, &ids)
            .await?
            .into_iter()
            .map(|correction| (correction.meeting_id, correction))
            .collect();
    let runs: HashMap<String, WorkflowRun> = workflow_run::newest_each(
        connection,
        workflow::workflow().name(),
        workflow::SOURCE_KIND,
        &source_ids,
    )
    .await?
    .into_iter()
    .map(|run| (run.source_id.clone(), run))
    .collect();
    let newest: Vec<&WorkflowRun> = runs.values().collect();
    let segmenting = workflow::segmenting_runs(connection, &newest).await?;
    Ok(meetings
        .into_iter()
        .map(|meeting| {
            let capture = captures.get(&meeting.id);
            let correction = corrections.get(&meeting.id);
            let run = runs.get(&meeting.id.to_string());
            let run_segments = run.is_some_and(|run| segmenting.contains(&run.id));
            MeetingResponse::new(meeting, capture, correction, run, run_segments)
        })
        .collect())
}

/// One meeting as answered, with its correction and its newest run.
///
/// # Errors
/// Propagates any database error.
pub async fn response(
    connection: &mut AsyncPgConnection,
    meeting: Meeting,
) -> QueryResult<MeetingResponse> {
    let mut answered = responses(connection, vec![meeting]).await?;
    Ok(answered.pop().expect("one meeting in, one response out"))
}
