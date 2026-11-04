// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/learnings/{id}`: correct a learning. Anyone may correct any learning,
//! including its title, which must stay another learning's own. The correction is a revision in
//! its history, so it can be undone.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{announce, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::learning::{self, LearningChanges};
use crate::routes::v1::action_items::validate_not_blank;
use crate::state::AppState;

/// The fields to change; those left out keep their value. Shaped and bounded as a save is.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 300))]
    title: Option<String>,
    #[validate(length(min = 1, max = 20000), custom(function = "validate_not_blank"))]
    body: Option<String>,
    #[validate(length(min = 1, max = 50))]
    category: Option<String>,
    #[validate(length(min = 1, max = 20000), custom(function = "validate_not_blank"))]
    evidence: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(mut body) = body?;
    // Spaced and trimmed as a save is, so a correction cannot store what a save would not.
    body.title = body
        .title
        .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "));
    body.body = body.body.map(|text| text.trim().to_owned());
    body.category = body.category.map(|key| key.trim().to_owned());
    body.evidence = body.evidence.map(|text| text.trim().to_owned());
    body.validate()?;

    let changes = LearningChanges {
        title: body.title,
        body: body.body,
        category: body.category,
        evidence: body.evidence,
    };
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = learning::correct(&mut connection, id, changes, author(&member, None)).await?;

    Ok(Json(json!({ "learning": announce(&state, recorded) })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_correction_names_only_what_it_changes_and_is_bounded_like_a_save() {
        let category: RequestBody =
            serde_json::from_value(json!({ "category": "decision" })).expect("parses");
        category.validate().expect("one field is enough");
        assert!(category.title.is_none() && category.body.is_none());

        let blank: RequestBody =
            serde_json::from_value(json!({ "evidence": "  " })).expect("parses");
        assert!(
            blank
                .validate()
                .expect_err("refused")
                .field_errors()
                .contains_key("evidence")
        );

        serde_json::from_value::<RequestBody>(json!({ "createdBy": "someone" }))
            .expect_err("only content can be corrected");
    }
}
