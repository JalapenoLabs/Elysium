// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/model-settings`: every step and agent, in the order the catalog lists them,
//! with its default and setting, and what the page offers to choose from. Admins only.

use std::collections::HashMap;

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::AgentModelResponse;
use crate::agent_models::{self, SUGGESTED_MODELS};
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::model_setting::{self, Effort};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let stored = model_setting::list(&mut connection).await?;
    let by_agent: HashMap<&str, _> = stored
        .iter()
        .map(|setting| (setting.agent.as_str(), setting))
        .collect();
    let agents: Vec<AgentModelResponse> = agent_models::catalog()
        .into_iter()
        .map(|entry| {
            let setting = by_agent.get(entry.name).copied();
            AgentModelResponse::new(entry, setting)
        })
        .collect();

    Ok(Json(json!({
        "agents": agents,
        "suggestedModels": SUGGESTED_MODELS,
        "efforts": Effort::ALL,
    })))
}
