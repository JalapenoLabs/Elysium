// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages/{id}/attachments/{attachmentId}/{name}`: one of a page's files, streamed from
//! its storage location to the company's members. The last segment is there for whoever reads the
//! address; the attachment's id finds the file.
//!
//! Anyone a page lets in can upload to it, and this is served on Elysium's own origin, so it answers
//! with the type its extension allows, never what the provider or uploader said, and displays only
//! a proven image: anything else is a download, which no browser renders here
//! (`crate::page_attachments`). A deleted page's files stay served, as it reads on at its address.

use anyhow::Context;
use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::{page_attachment, storage_location};
use crate::page_attachments;
use crate::state::AppState;

/// A file's bytes never change, since a new upload is a new attachment. Only removing one could
/// change what its address answers, so a browser may keep it for an hour, which spares a page of
/// images its requests, and private, so nothing shared keeps it.
const CACHE_CONTROL: HeaderValue = HeaderValue::from_static("private, max-age=3600");

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<(Uuid, Uuid, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path((id, attachment_id, _name)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let attachment = page_attachment::find(&mut connection, id, attachment_id).await?;
    let location = storage_location::find(&mut connection, attachment.storage_location_id).await?;
    drop(connection);

    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;
    let download = state
        .storage
        .download(&location, &access_key, &attachment.storage_path)
        .await?;

    let content_type = HeaderValue::from_str(&attachment.content_type)
        .context("a stored content type fits a header")?;
    let is_inline = page_attachments::displays_inline(&attachment.content_type);
    let mut response = Body::from_stream(download.body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(
        header::CONTENT_DISPOSITION,
        page_attachments::content_disposition(&attachment.file_name, is_inline),
    );
    headers.insert(header::CACHE_CONTROL, CACHE_CONTROL);
    if let Some(length) = download.content_length {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    }
    Ok(response)
}
