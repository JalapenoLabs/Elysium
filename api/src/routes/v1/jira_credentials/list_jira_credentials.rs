// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials`: every Jira credential, with what each may touch.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::JiraCredentialResponse;
use crate::errors::ApiError;
use crate::models::jira_credential;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let credentials: Vec<JiraCredentialResponse> = jira_credential::list(&mut connection)
        .await?
        .into_iter()
        .map(|(credential, allowed)| JiraCredentialResponse::new(credential, allowed))
        .collect();

    Ok(Json(json!({ "credentials": credentials })))
}
