// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/meeting-uploads/{id}/{kind}/parts/{index}`: one part of an upload's recording,
//! transcript, or attached file. The body is the part itself, sent with its `Content-Length`, and streams to the
//! upload's staging directory (`crate::meeting_uploads::staging`), so nothing holds it in memory.
//!
//! Parts arrive in order, each [`MeetingUpload::part_bytes`] long but the last. The part the API
//! needs next is written under a name of its own, then put in place and counted under the lock on
//! its file's row, so of two sends of one part at once only the one counted is kept; one it has
//! already is acknowledged without being written, so a part resent after its answer was lost
//! changes nothing; one past the next is refused with how far the file got, for the page to resume
//! from. A recording's first part, and an attached image's, must open as its format does before
//! any of it is kept (`crate::meeting_uploads::require_signature`), and a
//! transcript's last part is counted only once the whole transcript reads as one
//! (`super::read_transcript`).
//!
//! Only this request knows its send's name, so every send it does not put in place is let go here,
//! whatever stopped it: another send counted first, counting failed, or the upload was cancelled or
//! swept while the part arrived. That last one takes the upload's whole directory, which the send
//! may have made again after the cancel removed it.
//!
//! [`MeetingUpload::part_bytes`]: crate::models::meeting_upload::MeetingUpload::part_bytes

use std::error::Error;

use anyhow::Context;
use axum::Json;
use axum::RequestExt as _;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, Request, State};
use axum::http::header;
use bytes::{Bytes, BytesMut};
use diesel_async::AsyncConnection;
use futures_util::stream::{self, Stream, StreamExt as _};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{UploadResponse, read_transcript};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meeting_uploads::staging::StagedPart;
use crate::meeting_uploads::{self, FileKind, SIGNATURE_BYTES};
use crate::models::meeting_upload::{self, Staged};
use crate::state::AppState;

/// What became of a whole send once its file's row decided.
enum Counted {
    /// It was put under its part's name and counted.
    Placed,
    /// Another send of its part was counted first, so it is not needed.
    Superseded,
    /// Its upload was cancelled, or swept, while it arrived.
    Gone,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, String, u64)>, PathRejection>,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let Path((id, kind, index)) = path?;
    let kind = FileKind::parse(&kind).ok_or(ApiError::NotFound)?;
    let length: u64 = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| ApiError::BadRequest("a part must be sent with its length".to_owned()))?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let staged = meeting_upload::find_owned(&mut connection, id, current.id()).await?;
    if staged.upload.finished_at.is_some() {
        return Err(ApiError::CodedConflict {
            code: "meeting_upload_finished",
            message: "this upload is finished, so it takes no more parts",
        });
    }
    let file = staged.file(kind).ok_or(ApiError::NotFound)?;
    let part_bytes = staged.upload.part_bytes();
    let received = file.received_bytes();
    let next = received / u64::from(part_bytes);
    if index < next || file.is_received() {
        // Here already: the answer to an earlier send of it was lost.
        return Ok(Json(json!({ "upload": UploadResponse::from(staged) })));
    }
    if index > next {
        return Err(ApiError::CodedConflict {
            code: "meeting_upload_part_ahead",
            message: "parts arrive in order; this upload says which one it needs next",
        });
    }
    let expected = meeting_uploads::part_length(file.size_bytes(), part_bytes, index)
        .expect("the next part of a file not yet received exists");
    if length != expected {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_part_length",
            message: format!("part {index} carries {expected} bytes, not {length}"),
        });
    }
    let file_name = file.file_name.clone();
    let from = i64::try_from(received).context("a file's size fits a bigint")?;
    let to = i64::try_from(received + expected).context("a file's size fits a bigint")?;
    let is_last_of_transcript = kind == FileKind::Transcript && to == file.size_bytes;
    // The part may take a while to arrive; no connection is held while it does.
    drop(connection);

    // Limited to the route's `DefaultBodyLimit`, which a body read as a stream must ask for.
    let mut chunks = Box::pin(request.into_limited_body().into_data_stream());
    let head = if kind != FileKind::Transcript && index == 0 {
        let head = read_head(&mut chunks).await?;
        meeting_uploads::require_signature(kind, &file_name, &head)?;
        head.freeze()
    } else {
        Bytes::new()
    };
    let whole = stream::once(async move { Ok(head) }).chain(chunks);
    let sent = state
        .upload_staging
        .write_part((id, kind), index, expected, whole)
        .await?;

    let counted = count(
        &state,
        &staged,
        &sent,
        kind,
        (from, to),
        is_last_of_transcript,
    )
    .await;
    match counted {
        Ok(Counted::Placed) => {}
        Ok(Counted::Superseded) => state.upload_staging.discard(sent).await,
        Ok(Counted::Gone) => {
            if let Err(error) = state.upload_staging.remove(id).await {
                tracing::event!(
                    name: "meeting_uploads.send.unremoved",
                    tracing::Level::WARN,
                    meeting_upload.id = %id,
                    error.message = %error,
                    "a part that arrived for an upload let go meanwhile could not be removed",
                );
            }
            return Err(ApiError::NotFound);
        }
        Err(error) => {
            state.upload_staging.discard(sent).await;
            return Err(error);
        }
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let staged = meeting_upload::find_owned(&mut connection, id, current.id()).await?;
    Ok(Json(json!({ "upload": UploadResponse::from(staged) })))
}

/// Decides, under the lock on the row of `staged`'s file of `kind`, whether `sent` is the send of
/// its part that counts, moving what the file received `from` → `to`, and if it is, puts it in
/// place and counts it. One request at a time decides, so the send counted is the send kept. A
/// send that is put in place and then not counted, because the transcript it finishes cannot be
/// read, stays under its part's name uncounted, for the next send of it to replace.
///
/// # Errors
/// The workspace's refusal, the transcript's, and any database error; the send is then the
/// caller's to let go, as it is when it was not counted.
async fn count(
    state: &AppState,
    staged: &Staged,
    sent: &StagedPart,
    kind: FileKind,
    (from, to): (i64, i64),
    is_last_of_transcript: bool,
) -> Result<Counted, ApiError> {
    let id = staged.upload.id;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    connection
        .transaction(async |connection| {
            let locked = match meeting_upload::lock_file(connection, (id, kind)).await {
                Ok(locked) => locked,
                Err(diesel::result::Error::NotFound) => return Ok(Counted::Gone),
                Err(error) => return Err(ApiError::from(error)),
            };
            if locked.received_bytes != from {
                return Ok(Counted::Superseded);
            }
            state.upload_staging.place(sent).await?;
            if is_last_of_transcript {
                // The transcript's last part: one that cannot be read is refused now, before a
                // recording sent after it, rather than when the upload finishes.
                read_transcript(&state.upload_staging, staged).await?;
            }
            meeting_upload::record_part(connection, (id, kind), from, to).await?;
            Ok(Counted::Placed)
        })
        .await
}

/// Reads up to [`SIGNATURE_BYTES`] of what is arriving, however it is split into chunks. All of it
/// is kept and sent on, so the part is staged whole.
async fn read_head<Chunks, ChunkError>(
    chunks: &mut std::pin::Pin<Box<Chunks>>,
) -> Result<BytesMut, ApiError>
where
    Chunks: Stream<Item = Result<Bytes, ChunkError>>,
    ChunkError: Into<Box<dyn Error + Send + Sync>>,
{
    let mut head = BytesMut::with_capacity(SIGNATURE_BYTES);
    while head.len() < SIGNATURE_BYTES {
        match chunks.next().await {
            Some(Ok(chunk)) => head.extend_from_slice(&chunk),
            Some(Err(error)) => {
                return Err(ApiError::BadRequest(format!(
                    "the part stopped arriving: {}",
                    error.into()
                )));
            }
            None => break,
        }
    }
    Ok(head)
}
