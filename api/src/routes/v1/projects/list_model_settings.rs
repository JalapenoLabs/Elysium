// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/projects/{id}/model-settings`: the kinds of agent that work for one project,
//! each with the project's own choice, what it follows without one, and what its turns for the
//! project spent over the last `agent_models::USAGE_WINDOW`, with what the page offers to choose
//! from. Anyone who may see the project.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::agent_models::{self, AgentKind, ProjectKindView, USAGE_WINDOW};
use crate::errors::ApiError;
use crate::models::project;
use crate::models::session_event::RecordedUsage;
use crate::routes::v1::model_settings::{ChoicesResponse, ResolvedResponse, SettingResponse};
use crate::state::AppState;

/// One kind of agent as a project's model settings show it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProjectKindModelResponse {
    name: AgentKind,
    /// The project's own choice, or `null` while it follows the workspace.
    setting: Option<SettingResponse>,
    /// What the kind runs on without the project's choice: the workspace's setting of the
    /// kind, or its default.
    workspace: ResolvedResponse,
    /// What the kind's turns for the project spent since the list's `usageSince`, or `null`
    /// where its turns record no cost.
    usage: Option<RecordedUsage>,
}

impl From<ProjectKindView> for ProjectKindModelResponse {
    fn from(view: ProjectKindView) -> Self {
        Self {
            name: view.kind,
            setting: view.setting.as_ref().map(SettingResponse::from),
            workspace: ResolvedResponse::from(view.workspace),
            usage: view.usage,
        }
    }
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    project::find(&mut connection, id).await?;
    let since = Utc::now() - USAGE_WINDOW;
    let kinds: Vec<ProjectKindModelResponse> =
        agent_models::project_view(&mut connection, id, since)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();

    Ok(Json(json!({
        "kinds": kinds,
        "usageSince": since,
        "choices": ChoicesResponse::new(),
    })))
}
