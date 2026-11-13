// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/promises?followUpItemId=<id>`: the promise whose follow-up is that action item,
//! as a list of one or none, which the item's page shows with its draft. A promise made in a record
//! naming a meeting the person may not read is held back with it (`models::meeting_reads`).

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
use crate::models::meeting::Reader;
use crate::models::{draft, meeting_reads, promise, timeline_record};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    follow_up_item_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    query: Result<Query<Filters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(filters) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let mut promises = Vec::new();
    let Some(found) =
        promise::find_by_follow_up(&mut connection, filters.follow_up_item_id).await?
    else {
        return Ok(Json(json!({ "promises": promises })));
    };
    // A promise is what was said where it was made, so it is held back with that record.
    let readable = meeting_reads::readable_among(
        &mut connection,
        &[found.timeline_record_id],
        Reader::of(&member.0),
    )
    .await?;
    if readable.contains(&found.timeline_record_id) {
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
