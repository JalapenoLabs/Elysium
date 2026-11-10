// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meeting-uploads/{id}/finish`: once every part of every file arrived, makes the
//! upload's meeting (or its capture of a meeting the person has) and starts the meeting's run.
//!
//! The transcript is read again here, as it was when its last part arrived (`upload_part`), for the
//! lines the meeting keeps. Then, in one
//! transaction, the meeting and its capture are stored, the upload is marked finished with the
//! lines it was read as, and the meeting's run is started from its fetch, which stores the
//! recording in the storage location the upload chose (`crate::meetings::fetch`). Starting the run
//! in the same transaction is what keeps the meetings loop from starting one of its own for a
//! meeting it sees pending. A capture of a meeting is refused while that meeting's run is still its
//! own, since a meeting holds one run at a time.
//!
//! Finishing a finished upload answers its meeting again, so a person whose answer was lost sees
//! where it went.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::{Value, json};
use uuid::Uuid;

use super::create_upload::meeting_processing;
use super::read_transcript;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meeting_uploads::{self, transcript};
use crate::meetings::source::TranscriptLine;
use crate::meetings::workflow;
use crate::models::meeting::{self, Meeting, NewMeeting};
use crate::models::meeting_capture::{self, NewMeetingCapture};
use crate::models::meeting_upload::{self, MeetingUploadFile, Staged};
use crate::models::{user, workflow_run};
use crate::realtime::{ServerEvent, Signal};
use crate::routes::v1::meetings;
use crate::state::AppState;
use crate::workflows;

/// The most people a new meeting names as taking part: the speakers its transcript names, in the
/// order they first spoke.
const MAX_PARTICIPANTS: usize = 100;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let staged = meeting_upload::find_owned(&mut connection, id, current.id()).await?;
    if let Some(capture_id) = staged.upload.capture_id {
        let capture = meeting_capture::find(&mut connection, capture_id).await?;
        let made = meeting::find(&mut connection, capture.meeting_id).await?;
        let answered = meetings::response(&mut connection, made).await?;
        return Ok((StatusCode::OK, Json(json!({ "meeting": answered }))));
    }
    if !staged.files.iter().all(MeetingUploadFile::is_received) {
        return Err(ApiError::CodedConflict {
            code: "meeting_upload_incomplete",
            message: "some of this upload has not arrived yet",
        });
    }
    // Reading the transcript may take a moment; no connection is held while it does.
    drop(connection);
    let lines = read_transcript(&state.upload_staging, &staged).await?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let organizer = user::find(&mut connection, current.id())
        .await?
        .email
        .unwrap_or_default();
    let owner = current.id();
    let (made, started) = connection
        .transaction(async move |connection| {
            let staged = meeting_upload::lock_unfinished(connection, id, owner).await?;
            let (made, capture_id) =
                store_capture(connection, &staged, lines.as_deref(), organizer).await?;
            let transcript = lines
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .context("transcript lines are plain fields that serialize")?;
            meeting_upload::record_finished(connection, id, capture_id, transcript)
                .await?
                .ok_or(ApiError::NotFound)?;
            let started = workflow_run::start(
                connection,
                workflow::run_for_upload(made.id, capture_id, owner),
                chrono::Utc::now(),
            )
            .await?;
            if !started.created {
                // Another run became the meeting's since the upload began.
                return Err(meeting_processing());
            }
            Ok::<_, ApiError>((made, started))
        })
        .await?;

    workflows::announce(&state.events, &started.settled);
    state.events.signal(&Signal::Workflows);
    state.events.publish(&ServerEvent::MeetingChanged {
        id: made.id,
        state: made.state.clone(),
        created_by: made.created_by,
    });
    let answered = meetings::response(&mut connection, made).await?;
    Ok((StatusCode::CREATED, Json(json!({ "meeting": answered }))))
}

/// Stores what `staged` becomes: a new meeting with it as its first capture, or a capture of the
/// meeting it names. Answers the meeting and the capture's id.
async fn store_capture(
    connection: &mut AsyncPgConnection,
    staged: &Staged,
    lines: Option<&[TranscriptLine]>,
    organizer: String,
) -> Result<(Meeting, Uuid), ApiError> {
    let upload = &staged.upload;
    let capture = |meeting_id| NewMeetingCapture {
        id: Uuid::now_v7(),
        meeting_id,
        connector_id: None,
        source_kind: meeting_uploads::SOURCE_KIND.to_owned(),
        external_id: None,
        url: None,
        created_by: upload.created_by,
    };
    let Some(meeting_id) = upload.meeting_id else {
        let lines = lines.unwrap_or_default();
        let new_meeting = NewMeeting {
            id: Uuid::now_v7(),
            title: upload
                .title
                .clone()
                .context("a new meeting's upload keeps its title")?,
            started_at: upload
                .started_at
                .context("a new meeting's upload keeps when it started")?,
            duration_ms: spoken_for(lines),
            organizer,
            participants: json!(speakers(lines)),
            created_by: upload.created_by,
        };
        let new_capture = capture(new_meeting.id);
        let (made, captured) = meeting::insert_new(connection, new_meeting, new_capture)
            .await?
            .context("a capture with no connector never meets one already recorded")?;
        return Ok((made, captured.id));
    };

    // Locked, so the run started below is decided against this meeting as it stands.
    let current = meeting::lock(connection, meeting_id).await?;
    if current.created_by != upload.created_by {
        return Err(ApiError::NotFound);
    }
    let captured = meeting_capture::insert_new(connection, capture(meeting_id))
        .await?
        .context("a capture with no connector never meets one already recorded")?;
    Ok((current, captured.id))
}

/// How long a transcript's speech runs, when it carries times: where its last line ends.
fn spoken_for(lines: &[TranscriptLine]) -> Option<i64> {
    if transcript::is_untimed(lines) {
        return None;
    }
    lines.iter().map(|line| line.end_ms).max()
}

/// The speakers `lines` name, in the order they first spoke.
fn speakers(lines: &[TranscriptLine]) -> Vec<&str> {
    let mut named: Vec<&str> = Vec::new();
    for line in lines {
        if named.len() == MAX_PARTICIPANTS {
            break;
        }
        if !line.speaker.is_empty() && !named.contains(&line.speaker.as_str()) {
            named.push(&line.speaker);
        }
    }
    named
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start_ms: i64, end_ms: i64, speaker: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms,
            end_ms,
            speaker: speaker.to_owned(),
            text: "said".to_owned(),
        }
    }

    #[test]
    fn a_new_meeting_runs_as_long_as_its_speech_and_names_its_speakers() {
        let lines = [
            line(0, 4_000, "Ada"),
            line(4_000, 9_500, ""),
            line(9_500, 12_000, "Grace"),
            line(12_000, 13_000, "Ada"),
        ];

        assert_eq!(spoken_for(&lines), Some(13_000));
        assert_eq!(speakers(&lines), ["Ada", "Grace"]);
        // Words alone have no length.
        assert_eq!(spoken_for(&[line(0, 0, "Ada")]), None);
    }
}
