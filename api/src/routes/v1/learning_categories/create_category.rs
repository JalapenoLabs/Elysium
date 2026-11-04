// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/learning-categories`: add one of the workspace's own categories, after the
//! last of them. Admins only.
//!
//! Answers `201` with the category, and `409` when a category, built-in or not, already has the
//! key.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{LearningCategoryResponse, validate_key};
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::learning_category::{self, NewCategory};
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::validate_not_blank;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// What learnings store and agents send, such as `eda_tools`. It never changes.
    #[validate(custom(function = "validate_key"))]
    key: String,
    /// What people read, such as `EDA tools`.
    #[validate(length(min = 1, max = 60), custom(function = "validate_not_blank"))]
    label: String,
    /// What belongs there, for the agent choosing a category.
    #[validate(length(min = 1, max = 500), custom(function = "validate_not_blank"))]
    description: String,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    let body = RequestBody {
        key: body.key.trim().to_owned(),
        label: body.label.trim().to_owned(),
        description: body.description.trim().to_owned(),
    };
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let created = learning_category::create(
        &mut connection,
        NewCategory {
            key: body.key,
            label: body.label,
            description: body.description,
        },
        admin.id(),
    )
    .await?;
    state
        .events
        .publish(&ServerEvent::LearningCategoriesChanged);

    Ok((
        StatusCode::CREATED,
        Json(json!({ "category": LearningCategoryResponse::new(created, 0) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_category_needs_a_plain_key_a_label_and_a_description() {
        let valid = json!({
            "key": "eda_tools",
            "label": "EDA tools",
            "description": "Simulators, synthesis, and their licenses.",
        });
        serde_json::from_value::<RequestBody>(valid.clone())
            .expect("parses")
            .validate()
            .expect("passes");

        for (field, value) in [("key", "EDA tools"), ("label", " "), ("description", "")] {
            let mut body = valid.clone();
            body[field] = json!(value);
            let errors = serde_json::from_value::<RequestBody>(body)
                .expect("parses")
                .validate()
                .expect_err("refused");
            assert!(errors.field_errors().contains_key(field), "{field}");
        }

        let mut stray = valid;
        stray["builtIn"] = json!(true);
        serde_json::from_value::<RequestBody>(stray).expect_err("nobody adds a built-in category");
    }
}
