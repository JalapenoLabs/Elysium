// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/model-settings/{agent}`: set the model and effort a step or agent runs on, over
//! its kind's. Admins only.
//!
//! A model that could read as a flag is a `400` saying why. A name no step or agent has answers
//! `404`, and a step that runs no model `409`. A choice equal to what it follows without one
//! (its kind's setting or default) removes the setting, so it keeps following its kind.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use diesel_async::AsyncPgConnection;
use serde_json::{Value, json};

use super::{AgentModelResponse, ChoiceRequest};
use crate::agent_models;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<ChoiceRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(agent) = path?;
    let Json(body) = body?;
    let choice = body.into_choice()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changed = agent_models::change(&mut connection, &agent, &choice, admin.id()).await?;
    let response = answer(&mut connection, &agent).await?;
    if changed {
        state
            .events
            .publish(&ServerEvent::ModelSettingChanged { agent });
    }
    Ok(Json(json!({ "agent": response })))
}

/// The step or agent named `agent` as it stands now.
pub(super) async fn answer(
    connection: &mut AsyncPgConnection,
    agent: &str,
) -> Result<AgentModelResponse, ApiError> {
    let view = agent_models::entry_view(connection, agent)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(view.into())
}
