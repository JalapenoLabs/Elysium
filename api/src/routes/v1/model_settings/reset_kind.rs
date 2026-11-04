// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/model-settings/kinds/{kind}`: put a kind of agent back on its default across
//! the workspace. Admins only. Answers it as it now stands; one already on its default changes
//! nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};

use super::{KindModelResponse, kind_named};
use crate::agent_models::{self, USAGE_WINDOW};
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::kind_model_setting::Scope;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(name) = path?;
    let kind = kind_named(&name)?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changed =
        agent_models::reset_kind(&mut connection, Scope::Workspace, kind, admin.id()).await?;
    let view = agent_models::kind_view(&mut connection, kind, Utc::now() - USAGE_WINDOW).await?;
    if changed {
        state.events.publish(&ServerEvent::KindModelSettingChanged {
            kind,
            project_id: None,
        });
    }
    Ok(Json(json!({ "kind": KindModelResponse::from(view) })))
}
