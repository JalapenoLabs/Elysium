// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/github-credentials/{id}`: forget a token.
//!
//! The token itself stays valid on GitHub; revoke it there if it should stop working.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::github_credential;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    github_credential::delete(&mut connection, id).await?;

    state
        .events
        .publish(&ServerEvent::GithubCredentialDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
