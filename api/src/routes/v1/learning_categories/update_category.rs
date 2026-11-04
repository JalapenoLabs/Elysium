// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/learning-categories/{key}`: rename, describe, or reorder one of the
//! workspace's own categories. Admins only. Its key never changes, and a built-in category
//! answers `409`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::LearningCategoryResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::learning_category::{self, CategoryChanges};
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::validate_not_blank;
use crate::state::AppState;

/// The fields to change; those left out keep their value. Bounded as adding one is.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 60), custom(function = "validate_not_blank"))]
    label: Option<String>,
    #[validate(length(min = 1, max = 500), custom(function = "validate_not_blank"))]
    description: Option<String>,
    /// Where it goes among the workspace's own categories, counted from 0; past the end puts it
    /// last.
    position: Option<usize>,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(key) = path?;
    let Json(body) = body?;
    let body = RequestBody {
        label: body.label.map(|label| label.trim().to_owned()),
        description: body
            .description
            .map(|description| description.trim().to_owned()),
        position: body.position,
    };
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let updated = learning_category::update(
        &mut connection,
        &key,
        CategoryChanges {
            label: body.label,
            description: body.description,
            position: body.position,
        },
        admin.id(),
    )
    .await?;
    let counts = learning_category::learning_counts(&mut connection).await?;
    state
        .events
        .publish(&ServerEvent::LearningCategoriesChanged);

    let count = counts.get(&updated.key).copied().unwrap_or(0);
    Ok(Json(
        json!({ "category": LearningCategoryResponse::new(updated, count) }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_names_only_what_it_changes_and_never_the_key() {
        let moved: RequestBody = serde_json::from_value(json!({ "position": 0 })).expect("parses");
        moved.validate().expect("one field is enough");

        serde_json::from_value::<RequestBody>(json!({ "key": "renamed" }))
            .expect_err("a key never changes");
        serde_json::from_value::<RequestBody>(json!({ "position": -1 }))
            .expect_err("a place is counted from 0");
    }
}
