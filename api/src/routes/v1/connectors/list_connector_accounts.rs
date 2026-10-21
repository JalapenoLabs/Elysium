// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/connectors`: the signed-in person's connector accounts.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::ConnectorAccountResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::connector_account;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let accounts = connector_account::list_owned(&mut connection, current.id()).await?;
    let responses: Vec<ConnectorAccountResponse> = accounts
        .into_iter()
        .map(ConnectorAccountResponse::new)
        .collect();
    Ok(Json(json!({ "connectors": responses })))
}
