// Copyright © 2026 Jalapeno Labs

//! `/api/v1/model-settings`: the model and effort each kind of agent, workflow step, and agent
//! runs on across the workspace (`docs/engine.md`, Models). Admins only, reading and writing,
//! since it is a workspace setting that decides what every turn costs.
//!
//! - `GET /` lists every kind, with what it runs on and what its turns spent, and every step and
//!   agent `crate::agent_models::catalog` knows, each with its setting and what it resolves to
//!   and why, and the models, prices, and efforts the page offers.
//! - `PUT /kinds/{kind}` sets a kind; `DELETE /kinds/{kind}` puts it back on its default.
//! - `PUT /{agent}` sets one step or agent; `DELETE /{agent}` lets it follow its kind again.
//!
//! A choice equal to what it would follow anyway removes the setting instead. Each change is
//! recorded in the workspace's history and announced by name alone, `modelSetting.changed` for
//! an agent and `kindModelSetting.changed` for a kind; admins' pages load the list again.
//!
//! A project's own overrides of its kinds are under `/api/v1/projects/{id}/model-settings`.

mod list_settings;
mod reset_kind;
mod reset_setting;
mod set_kind;
mod set_setting;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::{get, put};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::agent_models::{
    self, AgentKind, EntryView, KindView, MODEL_PRICES, ModelPrice, Resolved, SUGGESTED_MODELS,
    Source,
};
use crate::errors::ApiError;
use crate::models::kind_model_setting::StoredKindSetting;
use crate::models::model_setting::{Effort, ModelChoice, StoredSetting};
use crate::models::session_event::RecordedUsage;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_settings::handle))
        .route(
            "/kinds/{kind}",
            put(set_kind::handle).delete(reset_kind::handle),
        )
        .route(
            "/{agent}",
            put(set_setting::handle).delete(reset_setting::handle),
        )
}

/// One step or agent as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelResponse {
    /// Its name in code: a step's, as the Runs page names it, or an agent's own.
    name: &'static str,
    is_step: bool,
    /// The kind of agent it is; `null` for a step that runs no model.
    kind: Option<AgentKind>,
    /// What an admin chose for it alone, or `null` while it follows its kind.
    setting: Option<SettingResponse>,
    /// What its turns run on outside any project, and why; `null` for a step that runs no model.
    resolved: Option<ResolvedResponse>,
}

impl From<EntryView> for AgentModelResponse {
    fn from(view: EntryView) -> Self {
        Self {
            name: view.entry.name,
            is_step: view.entry.is_step,
            kind: view.entry.kind,
            setting: view.setting.as_ref().map(SettingResponse::from),
            resolved: view.resolved.map(ResolvedResponse::from),
        }
    }
}

/// One kind of agent as the workspace's settings show it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindModelResponse {
    name: AgentKind,
    /// What its agents run on until someone chooses otherwise.
    default: ChoiceResponse,
    /// What an admin chose for the kind, or `null` while it runs on its default.
    setting: Option<SettingResponse>,
    /// Whether a project may override it for its own agents.
    runs_in_projects: bool,
    /// What its turns spent since the list's `usageSince`, or `null` where its turns record no
    /// cost.
    usage: Option<RecordedUsage>,
}

impl From<KindView> for KindModelResponse {
    fn from(view: KindView) -> Self {
        Self {
            name: view.kind,
            default: ChoiceResponse::from(&view.kind.default_choice()),
            setting: view.setting.as_ref().map(SettingResponse::from),
            runs_in_projects: view.kind.runs_in_projects(),
            usage: view.usage,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChoiceResponse {
    model: String,
    effort: Effort,
}

impl From<&ModelChoice> for ChoiceResponse {
    fn from(choice: &ModelChoice) -> Self {
        Self {
            model: choice.model.clone(),
            effort: choice.effort,
        }
    }
}

/// What someone chose, and when they last changed it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingResponse {
    model: String,
    effort: Effort,
    updated_at: DateTime<Utc>,
}

impl From<&StoredSetting> for SettingResponse {
    fn from(stored: &StoredSetting) -> Self {
        Self {
            model: stored.choice.model.clone(),
            effort: stored.choice.effort,
            updated_at: stored.updated_at,
        }
    }
}

impl From<&StoredKindSetting> for SettingResponse {
    fn from(stored: &StoredKindSetting) -> Self {
        Self {
            model: stored.choice.model.clone(),
            effort: stored.choice.effort,
            updated_at: stored.updated_at,
        }
    }
}

/// What a turn runs on, and which level it came from.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedResponse {
    model: String,
    effort: Effort,
    source: Source,
}

impl From<Resolved> for ResolvedResponse {
    fn from(resolved: Resolved) -> Self {
        Self {
            model: resolved.choice.model,
            effort: resolved.choice.effort,
            source: resolved.source,
        }
    }
}

/// What every page that sets a model offers to choose from: the current Claude models, their
/// prices, and the CLI's efforts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChoicesResponse {
    suggested_models: [&'static str; 4],
    model_prices: [ModelPrice; 4],
    efforts: [Effort; 5],
}

impl ChoicesResponse {
    pub(crate) const fn new() -> Self {
        Self {
            suggested_models: SUGGESTED_MODELS,
            model_prices: MODEL_PRICES,
            efforts: Effort::ALL,
        }
    }
}

/// A setting as `PUT` takes it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChoiceRequest {
    /// An id such as `claude-opus-5-5`, or an alias the CLI takes, such as `opus[1m]`.
    model: String,
    effort: Effort,
}

impl ChoiceRequest {
    /// The choice it names, its model trimmed. A model that could read as a flag is refused here,
    /// before Postgres refuses it again, so the answer says why.
    ///
    /// # Errors
    /// [`ApiError::BadRequest`] for a model the CLI's `--model` must not be given.
    pub(crate) fn into_choice(self) -> Result<ModelChoice, ApiError> {
        let choice = ModelChoice {
            model: self.model.trim().to_owned(),
            effort: self.effort,
        };
        if !agent_models::is_model_name(&choice.model) {
            return Err(ApiError::BadRequest(
                "a model is a letter or digit, then up to 99 letters, digits, dots, \
                 underscores, hyphens, or brackets, such as claude-opus-5-5 or opus[1m]"
                    .to_owned(),
            ));
        }
        Ok(choice)
    }
}

/// The kind named `name` in a path; a name no kind has answers `404`.
///
/// # Errors
/// [`ApiError::NotFound`] for a name this build has no kind by.
pub(crate) fn kind_named(name: &str) -> Result<AgentKind, ApiError> {
    AgentKind::from_name(name).ok_or(ApiError::NotFound)
}

#[cfg(test)]
mod request_tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_setting_names_a_model_and_an_effort_the_cli_takes() {
        let body: ChoiceRequest =
            serde_json::from_value(json!({ "model": " claude-haiku-5-5 ", "effort": "xhigh" }))
                .expect("parses");
        let choice = body.into_choice().expect("a model");
        assert_eq!(
            (choice.model.as_str(), choice.effort),
            ("claude-haiku-5-5", Effort::Xhigh)
        );

        serde_json::from_value::<ChoiceRequest>(json!({ "model": "opus", "effort": "extreme" }))
            .expect_err("an effort the CLI does not take");
        serde_json::from_value::<ChoiceRequest>(json!({ "model": "opus" }))
            .expect_err("both are named");
        serde_json::from_value::<ChoiceRequest>(
            json!({ "model": "opus", "effort": "low", "agent": "elysia" }),
        )
        .expect_err("the agent is the path's");
        let flag: ChoiceRequest = serde_json::from_value(
            json!({ "model": "--dangerously-skip-permissions", "effort": "low" }),
        )
        .expect("parses");
        assert!(flag.into_choice().is_err(), "a flag is no model");
    }
}
