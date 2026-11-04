// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/promises?followUpItemId=<id>`: the promise whose follow-up is that action item,
//! as a list of one or none, which the item's page shows with its draft.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::PromiseResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::{draft, promise, timeline_record};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    follow_up_item_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    query: Result<Query<Filters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(filters) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let mut promises = Vec::new();
    if let Some(found) =
        promise::find_by_follow_up(&mut connection, filters.follow_up_item_id).await?
    {
        let record = timeline_record::find_many(&mut connection, &[found.timeline_record_id])
            .await?
            .pop();
        let draft_id = draft::ids_for_promises(&mut connection, &[found.id])
            .await?
            .pop()
            .map(|(_promise_id, draft_id)| draft_id);
        promises.push(PromiseResponse::new(found, record.as_ref(), draft_id));
    }
    Ok(Json(json!({ "promises": promises })))
}
