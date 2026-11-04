// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/learning-categories`: every category, as it is offered: the built-in ones
//! first, then the workspace's own, each in its order, with how many learnings each holds.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::LearningCategoryResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::learning_category;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let counts = learning_category::learning_counts(&mut connection).await?;
    let categories: Vec<LearningCategoryResponse> = learning_category::list(&mut connection)
        .await?
        .into_iter()
        .map(|category| {
            let count = counts.get(&category.key).copied().unwrap_or(0);
            LearningCategoryResponse::new(category, count)
        })
        .collect();

    Ok(Json(json!({ "categories": categories })))
}
