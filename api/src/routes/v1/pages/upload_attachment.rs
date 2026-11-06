// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/attachments?name=<file name>`: attach a file to a page. The body is the
//! file itself, sent with its `Content-Length`, and streams straight to the storage location an
//! admin chose, so nothing holds it in memory and no form needs parsing to learn its size.
//!
//! Everything that can be refused without the file is refused before a byte of it moves. It makes
//! no version: the page's history records it, and the page's text refers to it by the address
//! answered here. A file written to a provider and then refused, by a page deleted in the meantime,
//! is deleted again.

use std::error::Error;

use anyhow::Context;
use axum::Json;
use axum::RequestExt as _;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use bytes::{Bytes, BytesMut};
use futures_util::stream::{self, Stream, StreamExt as _};
use serde::{Deserialize, Serialize};
use tracing::{Level, event};
use uuid::Uuid;

use super::{AttachmentResponse, PageResponse, answer, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page_attachment::{self, StoredFile};
use crate::models::storage_location;
use crate::page_attachments::{self, AttachmentName, MAX_ATTACHMENT_BYTES, SIGNATURE_BYTES};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UploadQuery {
    /// What the file is called. Its extension decides what it is.
    name: String,
}

/// A file arriving: what it is called, the bytes it says it holds, and those bytes.
#[derive(Debug)]
pub struct Upload<Chunks> {
    pub name: String,
    pub size_bytes: u64,
    pub chunks: Chunks,
}

/// What attaching a file left: the attachment, and its page, which now lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attached {
    pub attachment: AttachmentResponse,
    pub page: PageResponse,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<UploadQuery>, QueryRejection>,
    request: Request,
) -> Result<(StatusCode, Json<Attached>), ApiError> {
    let Path(id) = path?;
    let Query(query) = query?;
    let size_bytes = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| ApiError::BadRequest("a file must be sent with its length".to_owned()))?;

    let upload = Upload {
        name: query.name,
        size_bytes,
        // Limited to the route's `DefaultBodyLimit`, which a body read as a stream must ask for.
        chunks: request.into_limited_body().into_data_stream(),
    };
    let attached = upload_file(&state, author(&member), id, upload).await?;
    Ok((StatusCode::CREATED, Json(attached)))
}

/// Attaches `upload` to page `id` as `author`: what this route does, and what an agent's tool would.
///
/// # Errors
/// `400` with a stable code for a name, type, or file a page does not take, `413` with
/// `attachment_too_large` for a file over [`MAX_ATTACHMENT_BYTES`], `404` for an unknown page, `409`
/// for one that refuses it or with `attachment_no_location` until an admin chooses a location, and
/// `502` when the provider refuses.
pub async fn upload_file<Chunks, ChunkError>(
    state: &AppState,
    author: Author<'_>,
    id: Uuid,
    upload: Upload<Chunks>,
) -> Result<Attached, ApiError>
where
    Chunks: Stream<Item = Result<Bytes, ChunkError>> + Send + 'static,
    ChunkError: Into<Box<dyn Error + Send + Sync>> + 'static,
{
    let name = AttachmentName::parse(&upload.name)?;
    if upload.size_bytes == 0 {
        return Err(ApiError::CodedBadRequest {
            code: "attachment_empty",
            message: "this file is empty".to_owned(),
        });
    }
    if upload.size_bytes > MAX_ATTACHMENT_BYTES {
        return Err(ApiError::PayloadTooLarge {
            code: "attachment_too_large",
            message: format!("a file may be at most {MAX_ATTACHMENT_BYTES} bytes"),
        });
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let location_id = page_attachment::destination_for(&mut connection, id, author).await?;
    let location = storage_location::find(&mut connection, location_id).await?;
    // The file may take a while to arrive; no connection is held while it does.
    drop(connection);
    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;

    let mut chunks = Box::pin(upload.chunks);
    let head = read_head(&mut chunks).await?;
    name.file_type().require_signature(&head)?;

    let attachment_id = Uuid::now_v7();
    let storage_path = page_attachments::storage_path(id, attachment_id, name.as_str());
    let whole = stream::once(async move { Ok(head.freeze()) }).chain(chunks);
    state
        .storage
        .upload(
            &location,
            &access_key,
            &storage_path,
            whole,
            upload.size_bytes,
            Some(name.file_type().content_type()),
        )
        .await?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let file = StoredFile {
        id: attachment_id,
        page_id: id,
        storage_location_id: location_id,
        storage_path: &storage_path,
        file_name: name.as_str(),
        content_type: name.file_type().content_type(),
        size_bytes: i64::try_from(upload.size_bytes).context("a file's size fits a bigint")?,
    };
    let (attachment, recorded) = match page_attachment::add(&mut connection, file, author).await {
        Ok(added) => added,
        Err(error) => {
            // Nothing records this file, so nothing would ever find it again.
            if let Err(cleanup) = state
                .storage
                .delete(&location, &access_key, &storage_path)
                .await
            {
                event!(
                    name: "pages.attachment.orphaned",
                    Level::WARN,
                    file.path = storage_path,
                    error.message = %cleanup,
                    "a refused attachment could not be deleted from its location",
                );
            }
            return Err(error.into());
        }
    };

    Ok(Attached {
        attachment: AttachmentResponse::from(attachment),
        page: answer(state, &mut connection, recorded).await?,
    })
}

/// Reads up to [`SIGNATURE_BYTES`] of what is arriving, however it is split into chunks. All of it
/// is kept and sent on, so the file reaches the provider whole.
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
                    "the file stopped arriving: {}",
                    error.into()
                )));
            }
            None => break,
        }
    }
    Ok(head)
}
