// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/learnings`: save a learning. Saving a title a learning already has revises
//! that learning, bringing it back if it was deleted, so a takeaway is one learning however
//! often it is saved.
//!
//! Answers `201` with `outcome: "created"`, or `200` with `revised`, or with `unchanged` when the
//! learning already said exactly this.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use validator::Validate;

use super::{LearningResponse, announce, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::learning::{self, Agent, HistoryKind, LearningContent};
use crate::routes::v1::action_items::validate_not_blank;
use crate::state::AppState;

/// A learning to save. Saving a title that a learning already has, in any case, revises that
/// learning rather than adding another.
#[derive(Debug, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveLearningRequest {
    /// The takeaway, on one line of up to 300 characters, so the next agent knows the rule from
    /// the title alone: "Diesel's migration revert rewrites schema.rs", not "Diesel notes". Save
    /// the same title again to revise it.
    #[validate(length(min = 1, max = 300))]
    pub title: String,
    /// Leads with the rule or decision, then when it applies and why. Markdown, up to 20,000
    /// characters.
    #[validate(length(min = 1, max = 20000), custom(function = "validate_not_blank"))]
    pub body: String,
    /// The key of the category it is filed under: one of Elysium's built-in categories or the
    /// workspace's own (`GET /api/v1/learning-categories`).
    #[validate(length(min = 1, max = 50))]
    pub category: String,
    /// What shows it is true: the command and what it printed, a link, a quote and who said it,
    /// or the meeting it was decided in. Markdown, up to 20,000 characters.
    #[validate(length(min = 1, max = 20000), custom(function = "validate_not_blank"))]
    pub evidence: String,
}

/// What a save did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveOutcome {
    Created,
    Revised,
    /// The learning already said exactly this; nothing was recorded.
    Unchanged,
}

/// A saved learning and what saving it did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedLearning {
    pub learning: LearningResponse,
    pub outcome: SaveOutcome,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    body: Result<Json<SaveLearningRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<SavedLearning>), ApiError> {
    let Json(body) = body?;
    let saved = save(&state, &member, None, body).await?;
    let status = if saved.outcome == SaveOutcome::Created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(saved)))
}

/// Saves `request` as `member`, with `agent` acting for them: what both this route and the MCP
/// server's `learning_save` do.
///
/// The title is trimmed and its whitespace collapsed, and the body and evidence trimmed, before
/// they are checked, so a title differing only in spacing names the same learning.
///
/// # Errors
/// The request's validation errors.
pub async fn save(
    state: &AppState,
    member: &MemberUser,
    agent: Option<&Agent>,
    mut request: SaveLearningRequest,
) -> Result<SavedLearning, ApiError> {
    request.title = request
        .title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    request.body = request.body.trim().to_owned();
    request.category = request.category.trim().to_owned();
    request.evidence = request.evidence.trim().to_owned();
    request.validate()?;

    let content = LearningContent {
        title: request.title,
        body: request.body,
        category: request.category,
        evidence: request.evidence,
    };
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = learning::save(&mut connection, content, author(member, agent)).await?;

    let outcome = match recorded.history.first() {
        None => SaveOutcome::Unchanged,
        Some(entry) if entry.kind == HistoryKind::Created.as_str() => SaveOutcome::Created,
        Some(_) => SaveOutcome::Revised,
    };
    Ok(SavedLearning {
        learning: announce(state, recorded),
        outcome,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(body: serde_json::Value) -> Result<SaveLearningRequest, serde_json::Error> {
        serde_json::from_value(body)
    }

    fn valid() -> serde_json::Value {
        json!({
            "title": "Diesel's migration revert rewrites schema.rs",
            "body": "Use redo --all --locked-schema instead.",
            "category": "pitfall",
            "evidence": "schema.rs was emptied by revert --all.",
        })
    }

    #[test]
    fn a_learning_needs_every_field() {
        parse(valid()).expect("valid").validate().expect("passes");

        for field in ["title", "body", "category", "evidence"] {
            let mut missing = valid();
            missing.as_object_mut().expect("an object").remove(field);
            parse(missing).expect_err("every field is required");
        }

        let mut stray = valid();
        stray["tags"] = json!(["diesel"]);
        parse(stray).expect_err("unknown fields are refused");
    }

    #[test]
    fn oversized_or_blank_fields_are_refused() {
        for (field, value) in [
            ("title", "t".repeat(301)),
            ("body", "   ".to_owned()),
            ("category", String::new()),
            ("evidence", "e".repeat(20_001)),
        ] {
            let mut body = valid();
            body[field] = json!(value);
            let errors = parse(body)
                .expect("parses")
                .validate()
                .expect_err("refused");
            assert!(errors.field_errors().contains_key(field), "{field}");
        }
    }
}
