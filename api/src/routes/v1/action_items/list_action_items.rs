// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/action-items`: items, newest first, narrowed by the query.
//!
//! `state` takes one or more states, comma separated; `project` a project id or `none`;
//! `initiative` an initiative id; `waiting` and `snoozed` `true` or `false`; and
//! `deleted=true` lists only deleted items. Without filters, every item not deleted.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{comma_separated, item_responses, project_filter};
use crate::errors::ApiError;
use crate::models::action_item::{self, ActionItemFilter, ActionItemState, ProjectFilter};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default, deserialize_with = "comma_separated")]
    state: Vec<ActionItemState>,
    #[serde(default, deserialize_with = "project_filter")]
    project: Option<ProjectFilter>,
    initiative: Option<Uuid>,
    waiting: Option<bool>,
    snoozed: Option<bool>,
    #[serde(default)]
    deleted: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let filter = ActionItemFilter {
        states: query.state,
        project: query.project,
        initiative: query.initiative,
        waiting: query.waiting,
        snoozed: query.snoozed,
        deleted: query.deleted,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let items = action_item::list(&mut connection, &filter, Utc::now()).await?;
    let items = item_responses(&mut connection, items).await?;

    Ok(Json(json!({ "items": items })))
}
