// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/coding-sessions/{id}/events`: the thread's retained event history.
//!
//! The satellite has no paged history endpoint, so this replays the thread's stream
//! from the start and stops at the sequence the thread reported when the request
//! began. Events after that arrive live on the event stream; clients merge the two on
//! `sequence`. Only the latest [`HISTORY_LIMIT`] events are returned.

use std::collections::VecDeque;
use std::time::Duration;

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use futures_util::StreamExt as _;
use serde_json::{Value, json};

use crate::errors::ApiError;
use crate::fleet::views::SessionEvent;
use crate::models::coding_session;
use crate::state::AppState;

/// Most events one response carries. A long session's streamed output runs to
/// thousands of small events; the conversation view needs the recent end of it.
const HISTORY_LIMIT: usize = 5_000;

/// Longest the replay may take before the request gives up. Well under the API's
/// request timeout, so a slow satellite answers 502 rather than 408.
const HISTORY_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = coding_session::find(&mut connection, id).await?;
    drop(connection);

    let client = state.fleet.client(session.satellite_id).await?;
    let handle = client.threads().attach(session.thread_id.as_str()).await?;
    let latest_sequence = handle.get().await?.latest_sequence;
    if latest_sequence == 0 {
        return Ok(Json(json!({ "events": [], "truncated": false })));
    }

    let replay = async {
        let mut stream = handle.events_from(0).await?;
        let mut events = VecDeque::with_capacity(HISTORY_LIMIT.min(1_024));
        let mut truncated = false;

        while let Some(thread_event) = stream.next().await {
            let thread_event = thread_event?;
            let sequence = thread_event.sequence;

            if events.len() == HISTORY_LIMIT {
                events.pop_front();
                truncated = true;
            }
            events.push_back(SessionEvent::new(id, thread_event));

            if sequence >= latest_sequence {
                break;
            }
        }
        Ok::<_, ApiError>((events, truncated))
    };

    let (events, truncated) = tokio::time::timeout(HISTORY_TIMEOUT, replay)
        .await
        .map_err(|_elapsed| {
            ApiError::BadGateway("timed out replaying the thread's history".to_owned())
        })??;

    Ok(Json(json!({ "events": events, "truncated": truncated })))
}
