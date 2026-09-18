// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/jira-credentials/{id}`: forget a credential and what it was allowed to
//! touch.
//!
//! The token itself stays valid on Atlassian; revoke it there if it should stop working.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::jira_credential;
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

    jira_credential::delete(&mut connection, id).await?;

    state
        .events
        .publish(&ServerEvent::JiraCredentialDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
