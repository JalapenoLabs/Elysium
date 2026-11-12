// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meeting-uploads/{id}/finish`: once every part of every file arrived, makes the
//! upload's meeting (or its capture of a meeting the person has) and starts the meeting's run.
//!
//! The transcript is read again here, as it was when its last part arrived (`upload_part`), for the
//! lines the meeting keeps. Then, in one transaction, the meeting and its capture are stored (a
//! capture of a meeting the person has is recorded in its history as `capture_added`), the upload
//! is marked finished with the lines it was read as, and the meeting's run is started from its
//! fetch, which stores the recording in the storage location the upload chose
//! (`crate::meetings::fetch`). Starting the run in the same transaction is what keeps the meetings
//! loop from starting one of its own for a meeting it sees pending. A capture of a meeting is
//! refused while that meeting's run is still its own, since a meeting holds one run at a time.
//!
//! Finishing a finished upload answers its meeting again, so a person whose answer was lost sees
//! where it went.
//!
//! An upload of a file attached to a meeting finishes as an attachment instead: under the
//! meeting's lock it is recorded on the meeting, as `attached` in its history, and its run of the
//! meeting attachment workflow is started, which stores the file and reads its text
//! (`crate::meetings::attachment_workflow`). That answers the attachment, and its history entry for
//! the page's Undo.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::{Value, json};
use uuid::Uuid;

use super::create_upload::{MEETING_MERGED, meeting_busy};
use super::read_transcript;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meeting_attachments;
use crate::meeting_uploads::{self, FileKind, transcript};
use crate::meetings::attachment_workflow::{self, AttachmentFile};
use crate::meetings::attachments;
use crate::meetings::source::TranscriptLine;
use crate::meetings::workflow;
use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting, NewMeeting};
use crate::models::meeting_attachment::{self, UploadedFile};
use crate::models::meeting_capture::{self, NewMeetingCapture};
use crate::models::meeting_mark::{self, MarkError};
use crate::models::meeting_event::{self, Change, EventKind};
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
        return finished_before(&mut connection, capture_id).await;
    }
    if !staged.files.iter().all(MeetingUploadFile::is_received) {
        return Err(ApiError::CodedConflict {
            code: "meeting_upload_incomplete",
            message: "some of this upload has not arrived yet",
        });
    }
    if staged.file(FileKind::Attachment).is_some() {
        drop(connection);
        return finish_attachment(&state, &current, &staged).await;
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
    let finished = connection
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
                return Err(meeting_busy(&started.settled.run));
            }
            Ok::<_, ApiError>((made, started))
        })
        .await;
    let (made, started) = match finished {
        Ok(finished) => finished,
        Err(ApiError::NotFound) => {
            // Finished by another request while this one read its transcript: answer what it made.
            let staged = meeting_upload::find_owned(&mut connection, id, owner).await?;
            let capture_id = staged.upload.capture_id.ok_or(ApiError::NotFound)?;
            return finished_before(&mut connection, capture_id).await;
        }
        Err(error) => return Err(error),
    };

    workflows::announce(&state.events, &started.settled);
    state.events.signal(&Signal::Workflows);
    state.events.publish(&ServerEvent::MeetingChanged {
        id: made.id,
        state: made.state.clone(),
        created_by: made.created_by,
        entry_id: None,
    });
    let answered = meetings::response(&mut connection, made).await?;
    Ok((StatusCode::CREATED, Json(json!({ "meeting": answered }))))
}

/// Finishes `staged`, an upload of one file attached to its meeting: see the module's docs.
async fn finish_attachment(
    state: &AppState,
    current: &CurrentUser,
    staged: &Staged,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    if let Some(attachment_id) = staged.upload.attachment_id {
        return attached_before(&mut connection, staged, attachment_id).await;
    }
    let upload = &staged.upload;
    let meeting_id = upload
        .meeting_id
        .context("an attachment's upload names its meeting")?;
    let file = staged
        .file(FileKind::Attachment)
        .context("an attachment's upload holds its file")?;
    let storage_location_id = upload
        .storage_location_id
        .context("an attachment's upload names where it is kept")?;
    let uploaded = UploadedFile {
        id: Uuid::now_v7(),
        meeting_id,
        storage_location_id,
        file_name: file.file_name.clone(),
        content_type: meeting_attachments::content_type_of(&file.file_name).to_owned(),
        size_bytes: file.size_bytes,
        has_text: meeting_attachments::reader_of(&file.file_name).is_some(),
    };
    let (id, owner) = (upload.id, current.id());
    let finished = connection
        .transaction(async move |connection| {
            meeting_upload::lock_unfinished(connection, id, owner).await?;
            let found = match meeting_mark::lock_meeting(connection, meeting_id).await {
                Ok(()) => meeting::find(connection, meeting_id).await?,
                Err(MarkError::Merged) => return Err(MEETING_MERGED),
                Err(MarkError::Database(error)) => return Err(error.into()),
                Err(_) => return Err(ApiError::NotFound),
            };
            if found.created_by != owner {
                return Err(ApiError::NotFound);
            }
            let author = Author {
                user_id: owner,
                agent: None,
                at: Utc::now(),
            };
            let attached = attachments::attach_file(connection, uploaded, author)
                .await
                .map_err(meetings::attachment_refusal)?;
            meeting_upload::record_attached(connection, id, attached.attachment.id)
                .await?
                .ok_or(ApiError::NotFound)?;
            let file = AttachmentFile {
                attachment_id: attached.attachment.id,
                upload_id: Some(id),
            };
            let started = workflow_run::start(
                connection,
                attachment_workflow::run_for(file, owner),
                Utc::now(),
            )
            .await?;
            Ok::<_, ApiError>((found, attached, started))
        })
        .await;
    let (found, attached, started) = match finished {
        Ok(finished) => finished,
        Err(ApiError::NotFound) => {
            // Finished by another request meanwhile: answer what it made.
            let staged = meeting_upload::find_owned(&mut connection, id, owner).await?;
            let attachment_id = staged.upload.attachment_id.ok_or(ApiError::NotFound)?;
            return attached_before(&mut connection, &staged, attachment_id).await;
        }
        Err(error) => return Err(error),
    };

    workflows::announce(&state.events, &started.settled);
    state.events.signal(&Signal::Workflows);
    state.events.publish(&ServerEvent::MeetingChanged {
        id: found.id,
        state: found.state.clone(),
        created_by: found.created_by,
        entry_id: Some(attached.entry.id),
    });
    let answered = meetings::attachment_response(attached.attachment, None);
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "attachment": answered,
            "entry": meetings::MeetingEventResponse::from(attached.entry),
        })),
    ))
}

/// The answer for an upload of a file finished before, as attachment `attachment_id`.
async fn attached_before(
    connection: &mut AsyncPgConnection,
    staged: &Staged,
    attachment_id: Uuid,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let meeting_id = staged.upload.meeting_id.ok_or(ApiError::NotFound)?;
    let attachment = meeting_attachment::find(connection, meeting_id, attachment_id).await?;
    let answered = meetings::attachment_response(attachment, None);
    Ok((StatusCode::OK, Json(json!({ "attachment": answered }))))
}

/// The answer for an upload finished before, as capture `capture_id`: the meeting it became.
async fn finished_before(
    connection: &mut AsyncPgConnection,
    capture_id: Uuid,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let capture = meeting_capture::find(connection, capture_id).await?;
    let made = meeting::find(connection, capture.meeting_id).await?;
    let answered = meetings::response(connection, made).await?;
    Ok((StatusCode::OK, Json(json!({ "meeting": answered }))))
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
    // Merged into another since the upload began.
    if current.merged_into.is_some() {
        return Err(MEETING_MERGED);
    }
    let captured = meeting_capture::insert_new(connection, capture(meeting_id))
        .await?
        .context("a capture with no connector never meets one already recorded")?;
    meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id,
            kind: EventKind::CaptureAdded,
            author: Author {
                user_id: upload.created_by,
                agent: None,
                at: Utc::now(),
            },
            data: json!({
                "captureId": captured.id,
                "sourceKind": meeting_uploads::SOURCE_KIND,
                "recording": staged.file(FileKind::Recording).is_some(),
                "transcript": staged.file(FileKind::Transcript).is_some(),
            }),
            undid: None,
        },
    )
    .await?;
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
