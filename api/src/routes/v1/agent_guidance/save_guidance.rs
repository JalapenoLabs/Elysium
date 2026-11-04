// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/agent-guidance`: an admin's new text, saved as the next version, which every
//! agent reads from its next session on. Text the current version already holds records nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;

use crate::agent_guidance::AgentGuidanceResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// The guidance, Markdown, which is trimmed.
    content: String,
    /// The version the text was edited from. A save against one that is no longer current is
    /// refused, so an admin never replaces a version they have not seen.
    base_version: i32,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<AgentGuidanceResponse>, ApiError> {
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let saved = state
        .agent_guidance
        .save(
            &mut connection,
            &body.content,
            body.base_version,
            admin.id(),
        )
        .await?;
    if saved.is_some() {
        state.events.publish(&ServerEvent::AgentGuidanceChanged);
    }

    Ok(Json(state.agent_guidance.read(&mut connection).await?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_save_names_the_version_it_was_made_against() {
        let body: RequestBody = serde_json::from_value(json!({
            "content": "Search before asking.",
            "baseVersion": 3,
        }))
        .expect("parses");
        assert_eq!(body.base_version, 3);

        serde_json::from_value::<RequestBody>(json!({ "content": "Search before asking." }))
            .expect_err("a save without its version could replace one nobody saw");
        serde_json::from_value::<RequestBody>(json!({
            "content": "x",
            "baseVersion": 1,
            "rules": "none",
        }))
        .expect_err("a stray field is refused");
    }
}
