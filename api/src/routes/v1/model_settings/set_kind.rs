// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/model-settings/kinds/{kind}`: set the model and effort a kind of agent runs on
//! across the workspace, for each of its agents without a setting of its own. Admins only.
//!
//! A model that could read as a flag is a `400` saying why, and a name no kind has `404`. A
//! choice equal to the kind's default removes the setting. Answers the kind as it now stands;
//! the steps and agents it moved are read again from the list, which the event prompts.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};

use super::{ChoiceRequest, KindModelResponse, kind_named};
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
    body: Result<Json<ChoiceRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(name) = path?;
    let kind = kind_named(&name)?;
    let Json(body) = body?;
    let choice = body.into_choice()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changed =
        agent_models::change_kind(&mut connection, Scope::Workspace, kind, &choice, admin.id())
            .await?;
    let view = agent_models::kind_view(&mut connection, kind, Utc::now() - USAGE_WINDOW).await?;
    if changed {
        state.events.publish(&ServerEvent::KindModelSettingChanged {
            kind,
            project_id: None,
        });
    }
    Ok(Json(json!({ "kind": KindModelResponse::from(view) })))
}
