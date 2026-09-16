// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/github-credentials`: every GitHub credential.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::GithubCredentialResponse;
use crate::errors::ApiError;
use crate::models::github_credential;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let credentials: Vec<GithubCredentialResponse> = github_credential::list(&mut connection)
        .await?
        .into_iter()
        .map(GithubCredentialResponse::new)
        .collect();

    Ok(Json(json!({ "credentials": credentials })))
}
