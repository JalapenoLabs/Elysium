// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings`: every one of the signed-in person's meetings, newest first.
//!
//! The knowledge base's Meetings page lists them all and searches and filters them in the browser,
//! and its tree lists each under Meetings and under the brief of every project it has a topic on,
//! so the listing has no limit (`docs/knowledge.md`, The tree). Each answers the projects its topics
//! are on, `projectIds`, which both read.
//!
//! `reviewItemId` keeps the meeting whose corrected transcript an inbox item asks to review,
//! which the item's page links to.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::responses;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::{meeting, meeting_correction};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    review_item_id: Option<Uuid>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    query: Result<Query<Filters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(filters) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let meetings = match filters.review_item_id {
        Some(item_id) => {
            meeting_correction::meetings_reviewed_by(&mut connection, item_id, current.id()).await?
        }
        None => meeting::list_owned(&mut connection, current.id()).await?,
    };
    let answered = responses(&mut connection, meetings).await?;
    Ok(Json(json!({ "meetings": answered })))
}
