// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/browser-sessions`: the browsers the signed-in person created.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::BrowserSessionResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::browser_session;
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
    let sessions = browser_session::list_owned(&mut connection, current.id()).await?;

    let responses: Vec<BrowserSessionResponse> = sessions
        .into_iter()
        .map(|session| {
            let status = state.browsers.status(session.id);
            BrowserSessionResponse::new(session, status)
        })
        .collect();
    Ok(Json(json!({ "browserSessions": responses })))
}
