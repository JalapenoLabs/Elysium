// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/runs`: runs, newest first, a page at a time. `before` is the oldest run the
//! client already has; `hasMore` says whether any is older than this page.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::RunResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::workflow_run;
use crate::state::AppState;

/// How many runs one page holds: a screen's worth and then some, since every message and
/// meeting may start one and the list grows quickly.
const PAGE_SIZE: usize = 50;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    before: Option<Uuid>,
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
    // One more than a page, to learn whether another page follows.
    let limit = i64::try_from(PAGE_SIZE + 1).expect("a page's size fits an i64");
    let mut runs = workflow_run::list(&mut connection, query.before, limit).await?;
    let has_more = runs.len() > PAGE_SIZE;
    runs.truncate(PAGE_SIZE);
    let runs: Vec<RunResponse> = runs.into_iter().map(RunResponse::from).collect();
    Ok(Json(json!({ "runs": runs, "hasMore": has_more })))
}
