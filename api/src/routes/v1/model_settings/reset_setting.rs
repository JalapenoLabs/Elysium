// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/model-settings/{agent}`: let a step or agent follow its kind again. Admins
//! only. Answers it as it now stands; one with no setting of its own changes nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};

use super::set_setting::answer;
use crate::agent_models;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(agent) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changed = agent_models::reset(&mut connection, &agent, admin.id()).await?;
    let response = answer(&mut connection, &agent).await?;
    if changed {
        state
            .events
            .publish(&ServerEvent::ModelSettingChanged { agent });
    }
    Ok(Json(json!({ "agent": response })))
}
