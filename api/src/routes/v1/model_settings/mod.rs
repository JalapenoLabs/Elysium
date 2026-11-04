// Copyright © 2026 Jalapeno Labs

//! `/api/v1/model-settings`: the model and effort each workflow step and agent runs on
//! (`docs/engine.md`, Models). Admins only, reading and writing, since it is a workspace
//! setting that decides what every turn costs.
//!
//! - `GET /` lists every step and agent `crate::agent_models::catalog` knows, each with its
//!   default and its setting, and the models and efforts the page offers.
//! - `PUT /{agent}` sets one; a choice equal to its default removes its setting instead.
//! - `DELETE /{agent}` puts one back on its default.
//!
//! Each change is recorded in the workspace's history and announced as `modelSetting.changed`
//! with the agent's name alone; admins' pages load the list again.

mod list_settings;
mod reset_setting;
mod set_setting;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::{get, put};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::agent_models::{Entry, EntryKind};
use crate::models::model_setting::{Effort, ModelChoice, StoredSetting};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_settings::handle)).route(
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
    kind: EntryKindResponse,
    /// What it runs on until an admin chooses otherwise; `null` for a step that runs no model.
    default: Option<ChoiceResponse>,
    /// What an admin chose, or `null` while it runs on its default.
    setting: Option<SettingResponse>,
}

impl AgentModelResponse {
    fn new(entry: Entry, stored: Option<&StoredSetting>) -> Self {
        Self {
            name: entry.name,
            kind: match entry.kind {
                EntryKind::Step => EntryKindResponse::Step,
                EntryKind::Agent => EntryKindResponse::Agent,
            },
            default: entry.default.as_ref().map(ChoiceResponse::from),
            // A step that runs no model has nothing to set, whatever a row says.
            setting: entry.default.and(stored).map(|stored| SettingResponse {
                model: stored.choice.model.clone(),
                effort: stored.choice.effort,
                updated_at: stored.updated_at,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum EntryKindResponse {
    Step,
    Agent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChoiceResponse {
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingResponse {
    model: String,
    effort: Effort,
    updated_at: DateTime<Utc>,
}
