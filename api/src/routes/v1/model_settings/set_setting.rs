// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/model-settings/{agent}`: set the model and effort a step or agent runs on.
//! Admins only.
//!
//! The model is checked here before Postgres checks it again, so one that could read as a flag
//! is a `400` saying why. A name no step or agent has answers `404`, and a step that runs no
//! model `409`. A choice equal to the default removes the setting, so the agent follows its
//! default from then on.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AgentModelResponse;
use crate::agent_models;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::model_setting::{self, Effort, ModelChoice};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// An id such as `claude-opus-5-5`, or an alias the CLI takes, such as `opus[1m]`.
    model: String,
    effort: Effort,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(agent) = path?;
    let Json(body) = body?;
    let choice = ModelChoice {
        model: body.model.trim().to_owned(),
        effort: body.effort,
    };
    if !agent_models::is_model_name(&choice.model) {
        return Err(ApiError::BadRequest(
            "a model is a letter or digit, then up to 99 letters, digits, dots, underscores, \
             hyphens, or brackets, such as claude-opus-5-5 or opus[1m]"
                .to_owned(),
        ));
    }

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
    connection: &mut diesel_async::AsyncPgConnection,
    agent: &str,
) -> Result<AgentModelResponse, ApiError> {
    let entry = agent_models::catalog()
        .into_iter()
        .find(|entry| entry.name == agent)
        .ok_or(ApiError::NotFound)?;
    let stored = model_setting::find(connection, agent).await?;
    Ok(AgentModelResponse::new(entry, stored.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_names_a_model_and_an_effort_the_cli_takes() {
        let body: RequestBody =
            serde_json::from_value(json!({ "model": "claude-haiku-5-5", "effort": "xhigh" }))
                .expect("parses");
        assert_eq!(
            (body.model.as_str(), body.effort),
            ("claude-haiku-5-5", Effort::Xhigh)
        );

        serde_json::from_value::<RequestBody>(json!({ "model": "opus", "effort": "extreme" }))
            .expect_err("an effort the CLI does not take");
        serde_json::from_value::<RequestBody>(json!({ "model": "opus" }))
            .expect_err("both are named");
        serde_json::from_value::<RequestBody>(
            json!({ "model": "opus", "effort": "low", "agent": "elysia" }),
        )
        .expect_err("the agent is the path's");
    }
}
