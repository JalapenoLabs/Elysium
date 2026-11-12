// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meeting-uploads`: starts an upload of a recording, a transcript, or both, as a new
//! meeting (with its title and when it started) or as a capture of a meeting the person has; or of
//! one file attached to a meeting (`attachment`), of any kind, up to the admin's largest
//! (`crate::meeting_attachments`), which may arrive whenever the meeting is, its run or not.
//!
//! Everything that can be refused before a byte is sent is refused here: what the files are named,
//! their kinds and sizes, the meeting and the storage location they name. The files then arrive in
//! parts of the size this answers (`upload_part`).

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::UploadResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meeting_attachments::{FileName, MAX_PER_MEETING};
use crate::meeting_uploads::{FileKind, PART_BYTES, upload_name};
use crate::meetings::workflow;
use crate::models::meeting_upload::{self, NewMeetingUpload, NewUploadFile};
use crate::models::workflow_run::{WorkflowRun, WorkflowRunState};
use crate::models::{meeting, meeting_attachment, storage_location, user};
use crate::state::AppState;

/// The longest a meeting's title may be, as `meetings.title` allows.
const MAX_TITLE_CHARACTERS: usize = 2048;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateUpload {
    /// The meeting to add a capture to; absent for a new meeting.
    meeting_id: Option<Uuid>,
    /// A new meeting's title, and when it started.
    title: Option<String>,
    started_at: Option<DateTime<Utc>>,
    /// Where the recording is kept; required with one.
    storage_location_id: Option<Uuid>,
    recording: Option<FileToUpload>,
    transcript: Option<FileToUpload>,
    /// A file to attach to the meeting, alone.
    attachment: Option<FileToUpload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileToUpload {
    name: String,
    size_bytes: u64,
    /// When the file last changed, as the browser read it.
    modified_at: Option<DateTime<Utc>>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<CreateUpload>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(request) = body?;
    if let Some(attachment) = &request.attachment {
        return create_attachment(&state, &current, &request, attachment).await;
    }
    let mut files = Vec::new();
    for (kind, file) in [
        (FileKind::Recording, request.recording.as_ref()),
        (FileKind::Transcript, request.transcript.as_ref()),
    ] {
        if let Some(file) = file {
            files.push(new_file(kind, file)?);
        }
    }
    if files.is_empty() {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_nothing",
            message: "an upload needs a recording, a transcript, or both".to_owned(),
        });
    }
    let (title, started_at) = new_meeting_fields(&request)?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    if let Some(meeting_id) = request.meeting_id {
        // Its own, not merged away, and done with its run, as a capture is only added once it is
        // (`finish_upload`).
        let found = meeting::find_owned(&mut connection, meeting_id, current.id()).await?;
        if found.merged_into.is_some() {
            return Err(MEETING_MERGED);
        }
        if let Some(run) = workflow::live_run(&mut connection, meeting_id).await? {
            return Err(meeting_busy(&run));
        }
    }
    let storage_location_id = if request.recording.is_some() {
        let Some(location_id) = request.storage_location_id else {
            return Err(ApiError::CodedBadRequest {
                code: "meeting_upload_location",
                message: "a recording needs a storage location to be kept in".to_owned(),
            });
        };
        storage_location::find(&mut connection, location_id)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => ApiError::CodedBadRequest {
                    code: "meeting_upload_location",
                    message: "that storage location no longer exists".to_owned(),
                },
                other => ApiError::from(other),
            })?;
        Some(location_id)
    } else {
        None
    };

    let staged = meeting_upload::create(
        &mut connection,
        NewMeetingUpload {
            id: Uuid::now_v7(),
            meeting_id: request.meeting_id,
            title,
            started_at,
            storage_location_id,
            part_bytes: i32::try_from(PART_BYTES).expect("a part's size fits an integer"),
            created_by: current.id(),
        },
        files,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "upload": UploadResponse::from(staged) })),
    ))
}

/// Starts an upload of `attachment`, a file attached to the meeting `request` names, alone.
async fn create_attachment(
    state: &AppState,
    current: &CurrentUser,
    request: &CreateUpload,
    attachment: &FileToUpload,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Some(meeting_id) = request.meeting_id else {
        return Err(ApiError::BadRequest(
            "a file is attached to a meeting, so it names one".to_owned(),
        ));
    };
    let alone = request.recording.is_none()
        && request.transcript.is_none()
        && request.title.is_none()
        && request.started_at.is_none()
        && request.storage_location_id.is_none();
    if !alone {
        return Err(ApiError::BadRequest(
            "an attached file is uploaded alone, kept where page attachments are".to_owned(),
        ));
    }
    let file_name = FileName::parse(&attachment.name)?;
    if attachment.size_bytes == 0 {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_empty",
            message: format!("{} is empty", file_name.as_str()),
        });
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, meeting_id, current.id()).await?;
    if found.merged_into.is_some() {
        return Err(MEETING_MERGED);
    }
    let settings = user::workspace_settings(&mut connection).await?;
    let max_bytes = u64::try_from(settings.meeting_attachment_max_bytes).unwrap_or_default();
    if attachment.size_bytes > max_bytes {
        return Err(ApiError::PayloadTooLarge {
            code: "meeting_attachment_too_large",
            message: format!("a meeting's file may be at most {max_bytes} bytes"),
        });
    }
    let Some(location_id) = settings.page_attachment_storage_location_id else {
        return Err(ApiError::CodedConflict {
            code: "meeting_attachment_no_location",
            message: "No storage location takes attachments yet. An admin chooses one in \
                      Settings, Storage.",
        });
    };
    // Asked before a byte moves, and again as the upload finishes under the meeting's lock.
    if meeting_attachment::count_standing(&mut connection, found.id).await? >= MAX_PER_MEETING {
        return Err(ApiError::CodedConflict {
            code: "meeting_attachment_too_many",
            message: "This meeting holds as many files and frames as one may. Take one off first.",
        });
    }
    let staged = meeting_upload::create(
        &mut connection,
        NewMeetingUpload {
            id: Uuid::now_v7(),
            meeting_id: Some(found.id),
            title: None,
            started_at: None,
            storage_location_id: Some(location_id),
            part_bytes: i32::try_from(PART_BYTES).expect("a part's size fits an integer"),
            created_by: current.id(),
        },
        vec![NewUploadFile {
            kind: FileKind::Attachment,
            file_name: file_name.as_str().to_owned(),
            size_bytes: i64::try_from(attachment.size_bytes)
                .context("a file's size fits a bigint")?,
            modified_at: attachment.modified_at,
        }],
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "upload": UploadResponse::from(staged) })),
    ))
}

/// The refusal for a capture of a meeting merged into another, which holds no captures while it
/// is: the person adds to the meeting it went into.
pub const MEETING_MERGED: ApiError = ApiError::CodedConflict {
    code: "meeting_merged",
    message: "this meeting was merged into another; add to that one",
};

/// The refusal for a capture of a meeting whose run is still its `run`: one that failed waits for
/// its person to retry or skip it, and any other is still going or held.
pub fn meeting_busy(run: &WorkflowRun) -> ApiError {
    if run.state == WorkflowRunState::Failed {
        return ApiError::CodedConflict {
            code: "meeting_run_failed",
            message: "this meeting's run failed; retry or skip it, then add to the meeting",
        };
    }
    ApiError::CodedConflict {
        code: "meeting_processing",
        message: "this meeting is still being processed; add to it once its run is done",
    }
}

/// `file` as a file of `kind`, refused when its name, type, or size is not one a meeting takes.
fn new_file(kind: FileKind, file: &FileToUpload) -> Result<NewUploadFile, ApiError> {
    let file_name = upload_name(&file.name, kind)?;
    if file.size_bytes == 0 {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_empty",
            message: format!("{file_name} is empty"),
        });
    }
    let max_bytes = kind.max_bytes();
    if file.size_bytes > max_bytes {
        return Err(ApiError::PayloadTooLarge {
            code: "meeting_upload_too_large",
            message: format!(
                "a meeting's {} may be at most {max_bytes} bytes",
                kind.as_str()
            ),
        });
    }
    Ok(NewUploadFile {
        kind,
        file_name,
        size_bytes: i64::try_from(file.size_bytes).context("a file's size fits a bigint")?,
        modified_at: file.modified_at,
    })
}

/// A new meeting's title and start, which a new meeting needs and a capture takes from its own.
fn new_meeting_fields(
    request: &CreateUpload,
) -> Result<(Option<String>, Option<DateTime<Utc>>), ApiError> {
    if request.meeting_id.is_some() {
        if request.title.is_some() || request.started_at.is_some() {
            return Err(ApiError::BadRequest(
                "a capture takes its meeting's title and start, so it names neither".to_owned(),
            ));
        }
        return Ok((None, None));
    }
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    let Some(title) = title else {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_title",
            message: "a new meeting needs a title".to_owned(),
        });
    };
    if title.chars().count() > MAX_TITLE_CHARACTERS {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_title",
            message: format!("a meeting's title may be at most {MAX_TITLE_CHARACTERS} characters"),
        });
    }
    let Some(started_at) = request.started_at else {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_started_at",
            message: "a new meeting needs when it started".to_owned(),
        });
    };
    Ok((Some(title.to_owned()), Some(started_at)))
}
