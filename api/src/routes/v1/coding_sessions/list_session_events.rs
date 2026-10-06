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
use serde::Serialize;
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

/// Which of a thread's events to read.
#[derive(Debug, Clone, Copy)]
pub struct EventWindow {
    /// Only events after this sequence; `0` reads from the start.
    pub after_sequence: u64,
    /// The most events answered. When more match, the latest are kept.
    pub limit: usize,
    /// Keeps only events Elysium renders (messages, tools, turns, plans, questions,
    /// incidents), leaving out the satellite's bookkeeping.
    pub rendered_only: bool,
}

/// A slice of a thread's history, oldest first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub events: Vec<SessionEvent>,
    /// Whether earlier events in the window were left out to stay within the limit.
    pub truncated: bool,
    /// The thread's latest sequence when the read began. Reading after it answers what
    /// happened since.
    pub latest_sequence: u64,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let window = EventWindow {
        after_sequence: 0,
        limit: HISTORY_LIMIT,
        rendered_only: false,
    };
    let page = read_events(&state, id, window).await?;
    Ok(Json(
        json!({ "events": page.events, "truncated": page.truncated }),
    ))
}

/// Replays the session `id`'s thread over `window`, up to the sequence it had reached when the
/// read began.
///
/// # Errors
/// `404` for an unknown session, or `502` when the satellite fails or the replay outlasts
/// [`HISTORY_TIMEOUT`].
pub async fn read_events(
    state: &AppState,
    id: i64,
    window: EventWindow,
) -> Result<EventPage, ApiError> {
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
    if latest_sequence <= window.after_sequence {
        return Ok(EventPage {
            events: Vec::new(),
            truncated: false,
            latest_sequence,
        });
    }

    let replay = async {
        // The satellite resumes after the sequence it is given.
        let mut stream = handle.events_from(window.after_sequence).await?;
        let mut events = VecDeque::with_capacity(window.limit.min(1_024));
        let mut truncated = false;

        while let Some(thread_event) = stream.next().await {
            let thread_event = thread_event?;
            let sequence = thread_event.sequence;
            let event = SessionEvent::new(id, thread_event);

            let kept = !window.rendered_only || event.payload.is_some();
            if kept {
                if events.len() == window.limit {
                    events.pop_front();
                    truncated = true;
                }
                events.push_back(event);
            }

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

    Ok(EventPage {
        events: events.into(),
        truncated,
        latest_sequence,
    })
}
