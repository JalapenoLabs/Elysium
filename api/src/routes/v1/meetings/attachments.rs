// Copyright © 2026 Jalapeno Labs

//! A meeting's files and frames (`docs/meetings.md`, Attachments and frames):
//!
//! - `GET /api/v1/meetings/{id}/attachments` lists them, in the order they arrived;
//! - `GET .../attachments/{attachmentId}/{name}` serves one from its storage location;
//! - `DELETE .../attachments/{attachmentId}` takes one off with its pins;
//! - `POST .../frames` takes a still a person took from the player, with its caption, as a form;
//! - `POST .../pins` pins one over a range of the words, which is how one is pinned to an
//!   excerpt, and a pin is removed as any mark is (`DELETE .../marks/{markId}`).
//!
//! A file arrives as an upload's parts (`/api/v1/meeting-uploads`), as a recording does, since it
//! can be a gigabyte. Every change is in the meeting's history, so it answers its entry and the
//! marks as they now stand, for the page to apply and offer Undo (`history/{entryId}/undo`).
//!
//! Whoever reads the meeting lists and opens its files and frames, and whoever may change it
//! attaches, pins, and takes them off, as the meeting's other routes ask (`crate::meetings::access`,
//! `docs/meeting-review.md`, Who sees a meeting). `404` for a meeting they may not read, and
//! `409 meeting_merged` for every change to one merged away.

use std::fmt::Display;
use std::str::FromStr;

use anyhow::Context;
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::multipart::{Field, Multipart, MultipartRejection};
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::{MeetingEventResponse, announce, merged_refusal, open, transcript_change};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::images::{self, ImageError, MAX_FRAME_UPLOAD_BYTES};
use crate::meeting_attachments;
use crate::meetings::access::Need;
use crate::meetings::attachments::{self as meeting_attachments_change, AttachmentError};
use crate::models::agent::Author;
use crate::models::meeting::Meeting;
use crate::models::meeting_attachment::{self, MeetingAttachment, StoredFrame};
use crate::models::meeting_capture;
use crate::models::meeting_mark::{MarkError, Range};
use crate::models::storage_location::{self, StorageLocation};
use crate::models::user;
use crate::routes::v1::learnings::AgentResponse;
use crate::state::AppState;

/// The most a frame's form carries beside its still: the capture, the moment, the words, and a
/// caption of [`meeting_attachments::MAX_CAPTION_CHARACTERS`] in any script, with the form's own
/// boundaries, rounded up generously.
const FRAME_FIELDS_MAX_BYTES: usize = 64 * 1024;

/// The most `POST .../frames` takes: the largest still and the fields beside it.
pub(crate) const MAX_FRAME_FORM_BYTES: usize = MAX_FRAME_UPLOAD_BYTES + FRAME_FIELDS_MAX_BYTES;

/// The most bytes an id or a moment is written in.
const SHORT_FIELD_MAX_BYTES: usize = 64;

/// A file's bytes never change, since a new upload is a new attachment. Only taking one off could
/// change what its address answers, so a browser may keep it for an hour, privately, as a page's.
const CACHE_CONTROL: HeaderValue = HeaderValue::from_static("private, max-age=3600");

/// One file or frame of a meeting, as its page lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentResponse {
    id: Uuid,
    /// `file` or `frame`.
    kind: String,
    file_name: String,
    content_type: String,
    size_bytes: i64,
    /// Where it is served, once it is stored; `null` while an upload's parts are being stored.
    address: Option<String>,
    /// Whether a page shows it: a proven image or a frame. Anything else downloads.
    displays_inline: bool,
    /// A frame's caption, capture, and moment: on that capture's clock, and on the meeting's.
    caption: Option<String>,
    capture_id: Option<Uuid>,
    capture_ms: Option<i64>,
    meeting_ms: Option<i64>,
    /// How far reading its text got: `pending`, `read`, `none`, or `unreadable`.
    text_state: String,
    created_by: Uuid,
    agent: Option<AgentResponse>,
    created_at: DateTime<Utc>,
}

impl AttachmentResponse {
    /// `attachment`, its moment placed on the meeting's clock by its capture's `offset_ms`.
    pub fn new(attachment: MeetingAttachment, offset_ms: Option<i64>) -> Self {
        let address = attachment.stored_at.map(|_| {
            meeting_attachments::address(
                attachment.meeting_id,
                attachment.id,
                &attachment.file_name,
            )
        });
        Self {
            displays_inline: meeting_attachments::displays_inline(&attachment.content_type),
            meeting_ms: attachment
                .capture_ms
                .zip(offset_ms)
                .map(|(capture_ms, offset_ms)| capture_ms + offset_ms),
            agent: AgentResponse::new(attachment.agent, attachment.agent_name),
            id: attachment.id,
            kind: attachment.kind,
            file_name: attachment.file_name,
            content_type: attachment.content_type,
            size_bytes: attachment.size_bytes,
            address,
            caption: attachment.caption,
            capture_id: attachment.capture_id,
            capture_ms: attachment.capture_ms,
            text_state: attachment.text_state,
            created_by: attachment.created_by,
            created_at: attachment.created_at,
        }
    }
}

/// `attachment` as answered, its moment read from its capture.
///
/// # Errors
/// Propagates any database error.
async fn response(
    connection: &mut AsyncPgConnection,
    attachment: MeetingAttachment,
) -> Result<AttachmentResponse, ApiError> {
    let offset_ms = match attachment.capture_id {
        Some(capture_id) => Some(
            meeting_capture::find(connection, capture_id)
                .await?
                .offset_ms,
        ),
        None => None,
    };
    Ok(AttachmentResponse::new(attachment, offset_ms))
}

pub async fn list(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
    let offsets: std::collections::HashMap<Uuid, i64> =
        meeting_capture::list_of(&mut connection, found.id)
            .await?
            .into_iter()
            .map(|capture| (capture.id, capture.offset_ms))
            .collect();
    let attachments: Vec<AttachmentResponse> =
        meeting_attachment::of_meeting(&mut connection, found.id)
            .await?
            .into_iter()
            .map(|attachment| {
                let offset_ms = attachment
                    .capture_id
                    .and_then(|capture_id| offsets.get(&capture_id).copied());
                AttachmentResponse::new(attachment, offset_ms)
            })
            .collect();
    Ok(Json(json!({ "attachments": attachments })))
}

pub async fn read(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path((id, attachment_id, _name)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
    let attachment = meeting_attachment::find(&mut connection, found.id, attachment_id).await?;
    let Some(storage_path) = attachment
        .storage_path
        .as_deref()
        .filter(|_| attachment.removed_by.is_none())
    else {
        return Err(ApiError::NotFound);
    };
    let location = storage_location::find(&mut connection, attachment.storage_location_id).await?;
    drop(connection);

    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;
    let download = state
        .storage
        .download(&location, &access_key, storage_path)
        .await?;
    let content_type = HeaderValue::from_str(&attachment.content_type)
        .context("a stored content type fits a header")?;
    let is_inline = meeting_attachments::displays_inline(&attachment.content_type);
    let mut response = Body::from_stream(download.body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(
        header::CONTENT_DISPOSITION,
        crate::page_attachments::content_disposition(&attachment.file_name, is_inline),
    );
    headers.insert(header::CACHE_CONTROL, CACHE_CONTROL);
    if let Some(length) = download.content_length {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    }
    Ok(response)
}

pub async fn remove(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, attachment_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (found, _access) = open(&mut connection, id, &current, Need::Edit).await?;
    let entry = meeting_attachments_change::detach(
        &mut connection,
        &found,
        attachment_id,
        author_of(&current),
    )
    .await
    .map_err(refusal)?;
    let change = transcript_change(&mut connection, found.id, &entry).await?;
    announce(&state.events, &found, &entry);
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(entry),
        "change": change,
    })))
}

/// A frame as `POST .../frames` takes it: a `multipart/form-data` form rather than a query
/// string, since a caption of a thousand characters in a script of three bytes a character is
/// longer than a request line may be.
#[derive(Debug)]
struct FrameForm {
    /// The capture whose video it is a still of (`captureId`), and its moment on that capture's
    /// clock (`captureMs`).
    capture_id: Uuid,
    capture_ms: i64,
    /// What it shows (`caption`), checked.
    caption: String,
    /// The words it is pinned over (`startWordId` and `endWordId`, both or neither); none pins it
    /// over the word being spoken at its moment.
    range: Option<Range>,
    /// The picture itself (`still`), as the browser's canvas encoded it.
    still: Bytes,
}

pub async fn take_frame(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // Who may send a frame is decided before a byte of the still is read.
    let (found, _access) = open(&mut connection, id, &current, Need::Edit).await?;
    if found.merged_into.is_some() {
        return Err(merged_refusal());
    }
    let location_id = user::workspace_settings(&mut connection)
        .await?
        .page_attachment_storage_location_id
        .ok_or(meeting_attachments::NO_LOCATION)?;
    let location = storage_location::find(&mut connection, location_id).await?;
    // Reading, compressing, and storing the still take a moment; no connection is held meanwhile.
    drop(connection);
    let form = read_frame_form(multipart.map_err(|error| ApiError::BadRequest(error.body_text()))?)
        .await?;

    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;
    let range = form.range;
    let stored = store_frame(&state, (&location, &access_key), &found, form).await?;
    let storage_path = stored.storage_path.clone();

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let attached = match meeting_attachments_change::attach_frame(
        &mut connection,
        &found,
        stored,
        range,
        author_of(&current),
    )
    .await
    {
        Ok(attached) => attached,
        Err(error) => {
            // Nothing records this file, so nothing would ever find it again.
            if let Err(cleanup) = state
                .storage
                .delete(&location, &access_key, &storage_path)
                .await
            {
                event!(
                    name: "meetings.frame.orphaned",
                    Level::WARN,
                    file.path = storage_path,
                    error.message = %cleanup,
                    "a refused frame could not be deleted from its location",
                );
            }
            return Err(refusal(error));
        }
    };
    let change = transcript_change(&mut connection, found.id, &attached.entry).await?;
    announce(&state.events, &found, &attached.entry);
    let answered = response(&mut connection, attached.attachment).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "attachment": answered,
            "entry": MeetingEventResponse::from(attached.entry),
            "change": change,
        })),
    ))
}

/// Reads a frame's form, each part checked against its own limit as it arrives.
///
/// # Errors
/// `400` for an unknown or missing field, one that does not parse, a moment before 0, a range
/// with one end alone, or a caption [`meeting_attachments::caption`] refuses; `413` for a still
/// past [`MAX_FRAME_UPLOAD_BYTES`].
async fn read_frame_form(mut multipart: Multipart) -> Result<FrameForm, ApiError> {
    let (mut capture_id, mut capture_ms, mut caption) = (None, None, None);
    let (mut start_word_id, mut end_word_id, mut still) = (None, None, None);
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::BadRequest(error.body_text()))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "captureId" => capture_id = Some(parsed::<Uuid>(field, &name).await?),
            "captureMs" => capture_ms = Some(parsed::<i64>(field, &name).await?),
            "caption" => caption = Some(text_of(field, &name, FRAME_FIELDS_MAX_BYTES).await?),
            "startWordId" => start_word_id = Some(parsed::<Uuid>(field, &name).await?),
            "endWordId" => end_word_id = Some(parsed::<Uuid>(field, &name).await?),
            "still" => {
                let bytes = bytes_of(field, MAX_FRAME_UPLOAD_BYTES)
                    .await?
                    .ok_or_else(|| frame_refusal(ImageError::TooLarge))?;
                still = Some(Bytes::from(bytes));
            }
            other => return Err(ApiError::BadRequest(format!("unknown field `{other}`"))),
        }
    }
    let missing = |name: &str| ApiError::BadRequest(format!("a frame needs its `{name}`"));
    let capture_ms = capture_ms.ok_or_else(|| missing("captureMs"))?;
    if capture_ms < 0 {
        return Err(ApiError::BadRequest(
            "a frame's moment is on its capture's clock, from 0".to_owned(),
        ));
    }
    let range = match (start_word_id, end_word_id) {
        (Some(start_word_id), Some(end_word_id)) => Some(Range {
            start_word_id,
            end_word_id,
        }),
        (None, None) => None,
        _ => {
            return Err(ApiError::BadRequest(
                "a frame is pinned over a range with both its ends, or over its moment".to_owned(),
            ));
        }
    };
    Ok(FrameForm {
        capture_id: capture_id.ok_or_else(|| missing("captureId"))?,
        capture_ms,
        caption: meeting_attachments::caption(&caption.ok_or_else(|| missing("caption"))?)?,
        range,
        still: still.ok_or_else(|| missing("still"))?,
    })
}

/// A part's bytes, or `None` as soon as they pass `limit`, leaving the rest unread.
async fn bytes_of(mut field: Field<'_>, limit: usize) -> Result<Option<Vec<u8>>, ApiError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| ApiError::BadRequest(error.body_text()))?
    {
        if bytes.len() + chunk.len() > limit {
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Some(bytes))
}

/// A text part of at most `limit` bytes of UTF-8.
async fn text_of(field: Field<'_>, name: &str, limit: usize) -> Result<String, ApiError> {
    let bytes = bytes_of(field, limit)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("`{name}` is too long")))?;
    String::from_utf8(bytes)
        .map_err(|error| ApiError::BadRequest(format!("`{name}` is not UTF-8 text: {error}")))
}

/// A short text part read as what it names: an id or a moment.
async fn parsed<Parsed>(field: Field<'_>, name: &str) -> Result<Parsed, ApiError>
where
    Parsed: FromStr,
    Parsed::Err: Display,
{
    text_of(field, name, SHORT_FIELD_MAX_BYTES)
        .await?
        .trim()
        .parse()
        .map_err(|error| ApiError::BadRequest(format!("`{name}` does not read: {error}")))
}

/// Keeps `form`'s still, of a capture of `found`, in `location`: re-encoded from its pixels as a
/// frame (`crate::images::compress_frame`), and named for its moment.
async fn store_frame(
    state: &AppState,
    (location, access_key): (&StorageLocation, &SecretString),
    found: &Meeting,
    form: FrameForm,
) -> Result<StoredFrame, ApiError> {
    let FrameForm {
        capture_id,
        capture_ms,
        caption,
        still,
        ..
    } = form;
    let frame = tokio::task::spawn_blocking(move || images::compress_frame(&still))
        .await
        .context("compressing a frame panicked")?
        .map_err(frame_refusal)?;
    let attachment_id = Uuid::now_v7();
    let file_name = meeting_attachments::frame_name(capture_ms);
    let storage_path = meeting_attachments::storage_path(found.id, attachment_id, &file_name);
    let size_bytes = i64::try_from(frame.webp.len()).context("a frame's size fits a bigint")?;
    let webp = Bytes::from(frame.webp);
    state
        .storage
        .upload(
            location,
            access_key,
            &storage_path,
            futures_util::stream::once(async move { Ok::<_, std::io::Error>(webp) }),
            u64::try_from(size_bytes).context("a frame's size is positive")?,
            Some("image/webp"),
        )
        .await?;
    Ok(StoredFrame {
        id: attachment_id,
        meeting_id: found.id,
        storage_location_id: location.id,
        storage_path,
        file_name,
        size_bytes,
        capture_id,
        capture_ms,
        caption,
        fingerprint: frame.fingerprint,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "the fields are the request's wire names, which every range route shares"
)]
pub struct PinRequest {
    attachment_id: Uuid,
    start_word_id: Uuid,
    end_word_id: Uuid,
}

pub async fn pin(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<PinRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (found, _access) = open(&mut connection, id, &current, Need::Edit).await?;
    let range = Range {
        start_word_id: body.start_word_id,
        end_word_id: body.end_word_id,
    };
    let (_mark, entry) = meeting_attachments_change::pin(
        &mut connection,
        &found,
        body.attachment_id,
        range,
        author_of(&current),
    )
    .await
    .map_err(refusal)?;
    let change = transcript_change(&mut connection, found.id, &entry).await?;
    announce(&state.events, &found, &entry);
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(entry),
        "change": change,
    })))
}

fn author_of(current: &CurrentUser) -> Author<'static> {
    Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    }
}

fn frame_refusal(error: ImageError) -> ApiError {
    match error {
        ImageError::TooLarge => ApiError::PayloadTooLarge {
            code: "meeting_frame_too_large",
            message: format!("a frame may be at most {MAX_FRAME_UPLOAD_BYTES} bytes"),
        },
        ImageError::Unsupported | ImageError::Undecodable(_) => ApiError::CodedBadRequest {
            code: "meeting_frame_not_an_image",
            message: "a frame is a PNG, JPEG, or WebP still of the meeting's video".to_owned(),
        },
        ImageError::Encoding(reason) => {
            ApiError::Internal(anyhow::anyhow!("a frame could not be encoded: {reason}"))
        }
    }
}

pub(crate) fn refusal(error: AttachmentError) -> ApiError {
    match error {
        AttachmentError::Mark(MarkError::NotFound) | AttachmentError::NotFound => {
            ApiError::NotFound
        }
        AttachmentError::Mark(MarkError::NotStanding) => ApiError::CodedConflict {
            code: "meeting_words_changed",
            message: "A word of that range was changed meanwhile.",
        },
        AttachmentError::Mark(MarkError::OutOfOrder) => {
            ApiError::BadRequest("The range ends before it starts.".to_owned())
        }
        AttachmentError::Mark(MarkError::Merged) => merged_refusal(),
        AttachmentError::Mark(
            MarkError::EditsOwn | MarkError::Removed | MarkError::NotAnObject,
        )
        | AttachmentError::Removed => ApiError::CodedConflict {
            code: "meeting_attachment_removed",
            message: "It was taken off meanwhile.",
        },
        AttachmentError::TooMany => ApiError::CodedConflict {
            code: "meeting_attachment_too_many",
            message: "This meeting holds as many files and frames as one may. Take one off first.",
        },
        AttachmentError::NoRecording => ApiError::CodedBadRequest {
            code: "meeting_frame_no_recording",
            message: "That capture keeps no recording to take a frame from.".to_owned(),
        },
        AttachmentError::Mark(MarkError::Database(error)) | AttachmentError::Database(error) => {
            error.into()
        }
    }
}
