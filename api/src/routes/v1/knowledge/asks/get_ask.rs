// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/knowledge/asks/{id}`: one of the asker's asks, as it stands. The web app hears
//! when it changes (`knowledgeAsk.changed`) and loads it again; an MCP client, which hears no
//! events, waits for it with [`wait_for_answer`].

use std::time::Duration;

use anyhow::Context as _;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::Instant;
use uuid::Uuid;

use super::KnowledgeAskResponse;
use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::knowledge_ask;
use crate::state::AppState;

/// How often a wait reads the ask again. An answer takes seconds to tens of seconds, so a second
/// late costs nothing, and one read of a row by its key a second costs less than following
/// every event every process hears.
const WAIT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// One ask, as a tool names it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskArguments {
    /// The ask's id, as `knowledge_ask` answered it.
    pub id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let current = read(&state.database, &member, id).await?;
    Ok(Json(json!({ "ask": current })))
}

/// Ask `id`, when `asker` asked it.
///
/// # Errors
/// [`ApiError::NotFound`] for an ask that does not exist or is someone else's, and an internal
/// error when the database does not answer.
async fn read(
    database: &Pool,
    asker: &MemberUser,
    id: Uuid,
) -> Result<KnowledgeAskResponse, ApiError> {
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let current = knowledge_ask::get_for(&mut connection, id, asker.id())
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(current.into())
}

/// Ask `id` once it is answered or failed, or as it stands after `within`, whichever is first.
///
/// For a caller that hears no events, such as an MCP client: `within` keeps the wait inside the
/// request timeout every route has, and the caller asks again for an answer still being written.
///
/// # Errors
/// As [`read`].
pub async fn wait_for_answer(
    database: &Pool,
    asker: &MemberUser,
    id: Uuid,
    within: Duration,
) -> Result<KnowledgeAskResponse, ApiError> {
    let deadline = Instant::now() + within;
    loop {
        let current = read(database, asker, id).await?;
        let now = Instant::now();
        if current.state.is_over() || now >= deadline {
            return Ok(current);
        }
        tokio::time::sleep(WAIT_POLL_INTERVAL.min(deadline - now)).await;
    }
}
