// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/learnings`: every live learning, most recently changed first, or with
//! `?deleted=true` every deleted one, most recently deleted first.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::LearningResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::learning;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default)]
    deleted: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let learnings: Vec<LearningResponse> = learning::list(&mut connection, query.deleted)
        .await?
        .into_iter()
        .map(LearningResponse::from)
        .collect();

    Ok(Json(json!({ "learnings": learnings })))
}
