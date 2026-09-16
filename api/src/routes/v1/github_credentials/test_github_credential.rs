// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/github-credentials/{id}/test`: ask GitHub about the stored token right
//! now, and record what it answers.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::GithubCredentialResponse;
use crate::errors::ApiError;
use crate::models::github_credential;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential = github_credential::find(&mut connection, id).await?;

    let token = credential
        .token(&state.cipher)
        .context("the stored token cannot be decrypted")?;
    // A token can be revoked, rotated, or given new scopes on GitHub, so what it answers
    // now replaces what the row remembered.
    let account = state.github.verify(&token).await?;
    let credential = github_credential::record_check(&mut connection, id, &account).await?;
    drop(connection);

    state.events.publish(&ServerEvent::GithubCredentialUpserted(
        GithubCredentialResponse::new(credential),
    ));

    Ok(Json(json!({ "result": account })))
}
