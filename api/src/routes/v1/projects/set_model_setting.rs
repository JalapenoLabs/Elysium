// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/projects/{id}/model-settings/{kind}`: set the model and effort a kind of agent
//! runs on for this project's own agents, over everything the workspace set for it. Anyone who
//! may edit the project, as `PATCH /api/v1/projects/{id}` allows.
//!
//! A model that could read as a flag is a `400` saying why; a project or kind that does not
//! exist `404`; a kind that works for the whole workspace rather than one project
//! `409 kind_not_in_projects`. A choice equal to what the kind follows without one removes the
//! project's setting, so it keeps following the workspace.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
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
use crate::routes::v1::model_settings::{ChoiceRequest, kind_named};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, String)>, PathRejection>,
    body: Result<Json<ChoiceRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, name)) = path?;
    let kind = kind_named(&name)?;
    let Json(body) = body?;
    let choice = body.into_choice()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    project::find(&mut connection, id).await?;
    let changed = agent_models::change_kind(
        &mut connection,
        Scope::Project(id),
        kind,
        &choice,
        current.id(),
    )
    .await?;
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
