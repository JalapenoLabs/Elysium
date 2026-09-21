// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/changesets`: every changeset, newest first, or those in the states `state`
//! lists, such as `state=pending` for the ones waiting for review.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::ChangesetResponse;
use crate::errors::ApiError;
use crate::models::changeset::{self, ChangesetState};
use crate::routes::v1::action_items::comma_separated;
use crate::state::AppState;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default, deserialize_with = "comma_separated")]
    state: Vec<ChangesetState>,
}

pub async fn handle(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changesets: Vec<ChangesetResponse> = changeset::list(&mut connection, &query.state)
        .await?
        .into_iter()
        .map(ChangesetResponse::from)
        .collect();
    Ok(Json(json!({ "changesets": changesets })))
}
