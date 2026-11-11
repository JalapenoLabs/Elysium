// Copyright © 2026 Jalapeno Labs

//! `/api/v1/meetings`: the signed-in person's meetings, how far each one's processing got, and
//! the review of each on its page: its final transcript with its marks, each capture as it came,
//! edits, and its history with undo. See `crate::meetings` and `docs/meetings.md`.
//!
//! A meeting belongs to whoever owns the account it was read from, and every route answers
//! them alone, until meetings are public by default (`docs/meeting-review.md`, Who sees a
//! meeting). What a meeting said reaches everyone else as its segments on the timeline.

mod approve_correction;
mod decide_duplicate;
mod edit_words;
mod get_capture_transcript;
mod get_duplicate;
mod get_meeting;
mod get_words;
mod history;
mod hold_meeting;
mod list_meetings;
mod retry_meeting;
mod set_capture_offset;
mod split_capture;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use axum::Router;
use axum::routing::{get, post, put};
use chrono::{DateTime, Utc};
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::meetings::duplicates::Moved;
use crate::meetings::workflow;
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_capture::{self, MeetingCapture};
use crate::models::meeting_correction::{self, MeetingCorrection};
use crate::models::meeting_event::{EventKind, MeetingEvent};
use crate::models::meeting_mark::{self, Mark};
use crate::models::meeting_word::{self, Word, WordChange};
use crate::models::workflow_run::{self, WorkflowRun, WorkflowRunState};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::learnings::AgentResponse;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_meetings::handle))
        .route("/{id}", get(get_meeting::handle))
        .route("/{id}/retry", post(retry_meeting::handle))
        .route("/{id}/words", get(get_words::handle))
        .route("/{id}/edits", post(edit_words::handle))
        .route(
            "/{id}/captures/{capture_id}/transcript",
            get(get_capture_transcript::handle),
        )
        .route("/{id}/history", get(history::list))
        .route("/{id}/history/{entry_id}/undo", post(history::undo))
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
/// and whether a recording is kept) is its first capture's, the one it was listed with.
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
    /// The meeting its captures were merged into, while they are: it is listed nowhere then, and
    /// its page points there, where they are split back out.
    merged_into: Option<Uuid>,
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
            merged_into: meeting.merged_into,
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

/// One word of a meeting's final transcript as its page reads it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordResponse {
    id: Uuid,
    /// Orders the transcript; a change's new words slot in by it.
    position: i64,
    text: String,
    speaker: String,
    /// On the meeting's clock.
    start_ms: i64,
    end_ms: i64,
    /// `source`, `whisper`, `correction`, or `edit`.
    origin: String,
    /// The capture it was heard in; `null` for an edit's words.
    capture_id: Option<Uuid>,
}

impl From<Word> for WordResponse {
    fn from(word: Word) -> Self {
        Self {
            id: word.id,
            position: word.position,
            text: word.text,
            speaker: word.speaker,
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            origin: word.origin,
            capture_id: word.capture_id,
        }
    }
}

/// One mark over a range of a meeting's final transcript.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkResponse {
    id: Uuid,
    /// `confidence`, `edit`, `excerpt`, `black_out`, `comment`, `pin`, or `task`; a kind a later
    /// version adds reaches clients as it is stored.
    kind: String,
    start_word_id: Uuid,
    end_word_id: Uuid,
    /// Shaped by `kind`; see `docs/meetings.md`, The final transcript.
    data: Value,
    /// Who made it, and the agent that acted for them, if one did.
    created_by: Uuid,
    agent: Option<AgentResponse>,
    created_at: DateTime<Utc>,
}

impl From<Mark> for MarkResponse {
    fn from(mark: Mark) -> Self {
        Self {
            id: mark.id,
            kind: mark.kind,
            start_word_id: mark.start_word_id,
            end_word_id: mark.end_word_id,
            data: mark.data,
            created_by: mark.created_by,
            agent: AgentResponse::new(mark.agent, mark.agent_name),
            created_at: mark.created_at,
        }
    }
}

/// One entry of a meeting's history as its page lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingEventResponse {
    id: Uuid,
    /// `built`, `edited`, `marked`, `unmarked`, `hold_changed`, or `correction_approved`.
    kind: String,
    created_by: Uuid,
    agent: Option<AgentResponse>,
    /// Shaped by `kind`; see `docs/meetings.md`, History.
    data: Value,
    /// The entry this one undid, when it is an undo.
    undid: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl From<MeetingEvent> for MeetingEventResponse {
    fn from(entry: MeetingEvent) -> Self {
        Self {
            undid: entry.undid(),
            id: entry.id,
            kind: entry.kind,
            created_by: entry.created_by,
            agent: AgentResponse::new(entry.agent, entry.agent_name),
            data: entry.data,
            created_at: entry.created_at,
        }
    }
}

/// What one change did to a meeting's final transcript, for the page that made it to apply at
/// once rather than read tens of thousands of words again: the words it took out and the ones it
/// stood in their place, and the meeting's marks as they now stand, since a change moves any of
/// them. A change that touched no words, such as a hold, answers neither words list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptChange {
    removed_word_ids: Vec<Uuid>,
    words: Vec<WordResponse>,
    marks: Vec<MarkResponse>,
}

/// What history entry `entry` of meeting `meeting_id` did to its transcript, read in the
/// transaction that made it or after.
///
/// # Errors
/// Propagates any database error.
pub(super) async fn transcript_change(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    entry: &MeetingEvent,
) -> QueryResult<TranscriptChange> {
    // Only an edit, or its undo, changes which words stand; it always records its change whole.
    let words_change = (entry.kind() == Some(EventKind::Edited))
        .then(|| serde_json::from_value::<WordChange>(entry.data.clone()).ok())
        .flatten();
    let (removed_word_ids, words) = match words_change {
        Some(change) => {
            // A word a later change replaced already is not the page's to stand up; that change's
            // own announcement brings the transcript as it reads.
            let words = meeting_word::find_many(connection, meeting_id, &change.words)
                .await?
                .into_iter()
                .filter(|word| word.superseded_by.is_none())
                .map(WordResponse::from)
                .collect();
            (change.replaced, words)
        }
        None => (Vec::new(), Vec::new()),
    };
    let marks = meeting_mark::standing(connection, meeting_id)
        .await?
        .into_iter()
        .map(MarkResponse::from)
        .collect();
    Ok(TranscriptChange {
        removed_word_ids,
        words,
        marks,
    })
}

/// Whether history entry `entry` moved a capture, a hand-set offset or a merge or split or the
/// undo of one, which lines the transcript up again: no change of words a page could apply comes
/// back from it, so every page reads the transcript again ([`announce_lined_up`]).
pub(super) fn lines_up_again(entry: &MeetingEvent) -> bool {
    matches!(
        entry.kind(),
        Some(EventKind::OffsetChanged | EventKind::Merged | EventKind::Split)
    )
}

/// Tells every client that history entry `entry`, which moved a capture ([`lines_up_again`]),
/// changed its meeting, and for a merge or a split the other meeting too, with no entry, so every
/// page holding either reads it again, the one that asked included. A merge or a split also
/// started runs that align the meetings again, which the engine is told of.
///
/// # Errors
/// Propagates any database error.
pub(super) async fn announce_lined_up(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    entry: &MeetingEvent,
) -> QueryResult<()> {
    let mut changed_ids = vec![entry.meeting_id];
    if matches!(entry.kind(), Some(EventKind::Merged | EventKind::Split)) {
        if let Ok(moved) = serde_json::from_value::<Moved>(entry.data.clone()) {
            changed_ids.extend([moved.from, moved.into]);
        }
        events.signal(&Signal::Workflows);
    }
    changed_ids.sort_unstable();
    changed_ids.dedup();
    for changed_id in changed_ids {
        let changed = meeting::find(connection, changed_id).await?;
        events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state.clone(),
            created_by: changed.created_by,
            entry_id: None,
        });
    }
    Ok(())
}

/// Tells every client that `meeting` changed by history entry `entry`, so a page holding its
/// transcript or history reads it again, unless it made that change itself. The event carries no
/// content, as every meeting's does.
pub(super) fn announce(events: &EventBus, meeting: &Meeting, entry: &MeetingEvent) {
    events.publish(&ServerEvent::MeetingChanged {
        id: meeting.id,
        state: meeting.state.clone(),
        created_by: meeting.created_by,
        entry_id: Some(entry.id),
    });
}
