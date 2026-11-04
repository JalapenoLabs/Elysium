// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/knowledge/asks`: the asker's newest asks, newest first, as the Knowledge page's
//! Ask tab lists them.

use anyhow::Context as _;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::KnowledgeAskResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::knowledge_ask;
use crate::state::AppState;

/// How many asks the list holds: a few days of asking by hand, and an hour of the most one
/// person may ask at twice over. Older ones stay recorded and are not listed.
const ASKS_LISTED: i64 = 50;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let asks: Vec<KnowledgeAskResponse> =
        knowledge_ask::list_for(&mut connection, member.id(), ASKS_LISTED)
            .await?
            .into_iter()
            .map(KnowledgeAskResponse::from)
            .collect();
    Ok(Json(json!({ "asks": asks })))
}
