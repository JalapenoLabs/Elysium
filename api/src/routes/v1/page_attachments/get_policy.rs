// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/page-attachments`: where attachments go and what a page takes, for the company's
//! members, who are the ones attaching.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::PolicyResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::user;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let settings = user::workspace_settings(&mut connection).await?;
    Ok(Json(json!({ "policy": PolicyResponse::from(settings) })))
}
