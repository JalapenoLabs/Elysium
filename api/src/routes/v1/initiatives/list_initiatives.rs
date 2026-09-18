// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/initiatives`: initiatives by name, each with its progress.
//!
//! `state` takes one or more states, comma separated; `project` a project id or `none`;
//! and `deleted=true` lists only deleted initiatives.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use super::initiative_responses;
use crate::errors::ApiError;
use crate::models::action_item::ProjectFilter;
use crate::models::initiative::{self, InitiativeFilter, InitiativeState};
use crate::routes::v1::action_items::{comma_separated, project_filter};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default, deserialize_with = "comma_separated")]
    state: Vec<InitiativeState>,
    #[serde(default, deserialize_with = "project_filter")]
    project: Option<ProjectFilter>,
    #[serde(default)]
    deleted: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let filter = InitiativeFilter {
        states: query.state,
        project: query.project,
        deleted: query.deleted,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let initiatives = initiative::list(&mut connection, &filter).await?;
    let initiatives = initiative_responses(&mut connection, initiatives, Utc::now()).await?;

    Ok(Json(json!({ "initiatives": initiatives })))
}
