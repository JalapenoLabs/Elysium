// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/model-settings`: every kind, then every step and agent in the order the catalog
//! lists them, each with what was chosen and what it runs on, what each kind's turns spent over
//! the last `agent_models::USAGE_WINDOW`, and what the page offers to choose from. Admins only.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use chrono::Utc;
use serde_json::{Value, json};

use super::{AgentModelResponse, ChoicesResponse, KindModelResponse};
use crate::agent_models::{self, USAGE_WINDOW};
use crate::auth::AdminUser;
use crate::errors::ApiError;
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
    let since = Utc::now() - USAGE_WINDOW;
    let (entries, kinds) = agent_models::workspace_view(&mut connection, since).await?;
    let agents: Vec<AgentModelResponse> = entries.into_iter().map(Into::into).collect();
    let kinds: Vec<KindModelResponse> = kinds.into_iter().map(Into::into).collect();

    Ok(Json(json!({
        "kinds": kinds,
        "agents": agents,
        "usageSince": since,
        "choices": ChoicesResponse::new(),
    })))
}
