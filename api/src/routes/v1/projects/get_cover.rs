// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/projects/{id}/cover`: the cover image, as WebP.
//!
//! Clients request it with the project's `coverUpdatedAt` in the query string, so each
//! version has its own URL and can be cached for good. `404` when the project has no
//! cover.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::project;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Some((image, _updated_at)) = project::find_cover(&mut connection, id).await? else {
        return Err(ApiError::NotFound);
    };

    Ok((
        [
            (header::CONTENT_TYPE, "image/webp"),
            // Private: covers are workspace data, not for shared caches.
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        image,
    ))
}
