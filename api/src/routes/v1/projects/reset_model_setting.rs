// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/projects/{id}/model-settings/{kind}`: let a kind of agent follow the
//! workspace again for this project. Anyone who may edit the project. Answers it as it now
//! stands; one with no setting of the project's own changes nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::list_model_settings::ProjectKindModelResponse;
use crate::agent_models::{self, USAGE_WINDOW};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::kind_model_setting::Scope;
use crate::models::project;
use crate::realtime::ServerEvent;
use crate::routes::v1::model_settings::kind_named;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, name)) = path?;
    let kind = kind_named(&name)?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    project::find(&mut connection, id).await?;
    let changed =
        agent_models::reset_kind(&mut connection, Scope::Project(id), kind, current.id()).await?;
    let view =
        agent_models::project_kind_view(&mut connection, id, kind, Utc::now() - USAGE_WINDOW)
            .await?;
    if changed {
        state.events.publish(&ServerEvent::KindModelSettingChanged {
            kind,
            project_id: Some(id),
        });
    }
    Ok(Json(
        json!({ "kind": ProjectKindModelResponse::from(view) }),
    ))
}
