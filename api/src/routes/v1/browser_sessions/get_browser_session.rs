// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/browser-sessions/{id}`: one of the signed-in person's browsers.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::BrowserSessionResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::browser_session;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = browser_session::find_owned(&mut connection, id, current.id()).await?;

    let status = state.browsers.status(session.id);
    Ok(Json(
        json!({ "browserSession": BrowserSessionResponse::new(session, status) }),
    ))
}
