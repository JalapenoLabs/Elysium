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

/// Which end of a window to keep when more events match than the limit allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// The most recent, for a view of what is happening now. Older events are dropped.
    Latest,
    /// The oldest, for a reader paging forward from a cursor, so no event is skipped.
    Earliest,
}

/// Which of a thread's events to read.
#[derive(Debug, Clone, Copy)]
pub struct EventWindow {
    /// Only events after this sequence; `0` reads from the start.
    pub after_sequence: u64,
    /// The most events answered.
    pub limit: usize,
    /// Which end of the window is answered when more events match than `limit`.
    pub keep: Keep,
    /// Keeps only events Elysium renders (messages, tools, turns, plans, questions,
    /// incidents), leaving out the satellite's bookkeeping.
    pub rendered_only: bool,
}

/// A slice of a thread's history, oldest first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub events: Vec<SessionEvent>,
    /// Whether more events matched than were answered.
    pub truncated: bool,
    /// The thread's latest sequence when the read began.
    pub latest_sequence: u64,
    /// Where the next read continues: passed back as the window's `after_sequence`, it answers
    /// the events after these, skipping none. When the page kept its earliest events and more
    /// remain, that is the last event answered; otherwise it is `latest_sequence`.
    pub next_sequence: u64,
}

/// Whether a replay should go on reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

/// Decides, event by event, what a page keeps. Kept apart from the satellite's stream so the
/// window's rules are tested on their own.
#[derive(Debug)]
struct EventCollector {
    window: EventWindow,
    events: VecDeque<SessionEvent>,
    truncated: bool,
}

impl EventCollector {
    fn new(window: EventWindow) -> Self {
        Self {
            window,
            events: VecDeque::with_capacity(window.limit.min(1_024)),
            truncated: false,
        }
    }

    /// Takes the next event in sequence order.
    fn offer(&mut self, event: SessionEvent) -> Flow {
        if self.window.rendered_only && event.payload.is_none() {
            return Flow::Continue;
        }
        if self.events.len() < self.window.limit {
            self.events.push_back(event);
            return Flow::Continue;
        }

        self.truncated = true;
        match self.window.keep {
            Keep::Latest => {
                self.events.pop_front();
                self.events.push_back(event);
                Flow::Continue
            }
            // The page is full and one more matched: that is all a cursor read needs to know.
            Keep::Earliest => Flow::Stop,
        }
    }

    fn finish(self, latest_sequence: u64) -> EventPage {
        let resumes_mid_window = self.truncated && self.window.keep == Keep::Earliest;
        let next_sequence = match self.events.back() {
            Some(last) if resumes_mid_window => last.sequence,
            _ => latest_sequence,
        };
        EventPage {
            events: self.events.into(),
            truncated: self.truncated,
            latest_sequence,
            next_sequence,
        }
    }
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let window = EventWindow {
        after_sequence: 0,
        limit: HISTORY_LIMIT,
        keep: Keep::Latest,
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
        return Ok(EventCollector::new(window).finish(latest_sequence));
    }

    let replay = async {
        // The satellite resumes after the sequence it is given.
        let mut stream = handle.events_from(window.after_sequence).await?;
        let mut collector = EventCollector::new(window);

        while let Some(thread_event) = stream.next().await {
            let thread_event = thread_event?;
            let sequence = thread_event.sequence;
            let flow = collector.offer(SessionEvent::new(id, thread_event));
            if flow == Flow::Stop || sequence >= latest_sequence {
                break;
            }
        }
        Ok::<_, ApiError>(collector)
    };

    let collector = tokio::time::timeout(HISTORY_TIMEOUT, replay)
        .await
        .map_err(|_elapsed| {
            ApiError::BadGateway("timed out replaying the thread's history".to_owned())
        })??;
    Ok(collector.finish(latest_sequence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::views::EventPayload;

    fn event(sequence: u64, rendered: bool) -> SessionEvent {
        SessionEvent {
            session_id: 1,
            sequence,
            turn_id: None,
            occurred_at: None,
            event_type: "agent.message".to_owned(),
            member_id: None,
            payload: rendered.then(|| EventPayload::AgentMessage {
                author: None,
                text: format!("message {sequence}"),
            }),
        }
    }

    fn collect(window: EventWindow, events: impl IntoIterator<Item = SessionEvent>) -> EventPage {
        let mut collector = EventCollector::new(window);
        for event in events {
            if collector.offer(event) == Flow::Stop {
                break;
            }
        }
        collector.finish(900)
    }

    fn sequences(page: &EventPage) -> Vec<u64> {
        page.events.iter().map(|event| event.sequence).collect()
    }

    #[test]
    fn a_cursor_read_keeps_the_earliest_and_resumes_after_the_last_it_answered() {
        let window = EventWindow {
            after_sequence: 500,
            limit: 3,
            keep: Keep::Earliest,
            rendered_only: false,
        };
        let page = collect(window, (501..=900).map(|sequence| event(sequence, true)));
        assert_eq!(sequences(&page), [501, 502, 503]);
        assert!(page.truncated);
        assert_eq!(
            page.next_sequence, 503,
            "the next read starts at 504, skipping nothing"
        );
        assert_eq!(page.latest_sequence, 900);
    }

    #[test]
    fn walking_a_gap_page_by_page_reaches_the_head_without_skipping() {
        let history: Vec<u64> = (1..=10).collect();
        let mut cursor = 0;
        let mut read = Vec::new();
        loop {
            let window = EventWindow {
                after_sequence: cursor,
                limit: 4,
                keep: Keep::Earliest,
                rendered_only: false,
            };
            let remaining = history
                .iter()
                .filter(|sequence| **sequence > cursor)
                .map(|sequence| event(*sequence, true));
            let mut collector = EventCollector::new(window);
            for event in remaining {
                if collector.offer(event) == Flow::Stop {
                    break;
                }
            }
            let page = collector.finish(10);
            read.extend(sequences(&page));
            cursor = page.next_sequence;
            if !page.truncated {
                break;
            }
        }
        assert_eq!(read, history);
        assert_eq!(cursor, 10);
    }

    #[test]
    fn a_tail_read_keeps_the_latest_and_points_at_the_head() {
        let window = EventWindow {
            after_sequence: 0,
            limit: 3,
            keep: Keep::Latest,
            rendered_only: false,
        };
        let page = collect(window, (1..=900).map(|sequence| event(sequence, true)));
        assert_eq!(sequences(&page), [898, 899, 900]);
        assert!(page.truncated);
        assert_eq!(page.next_sequence, 900);
    }

    #[test]
    fn unrendered_events_neither_fill_the_page_nor_hold_the_cursor_back() {
        let window = EventWindow {
            after_sequence: 0,
            limit: 2,
            keep: Keep::Earliest,
            rendered_only: true,
        };
        let events = [
            event(1, false),
            event(2, true),
            event(3, false),
            event(4, true),
        ];
        let page = collect(window, events);
        assert_eq!(sequences(&page), [2, 4]);
        assert!(!page.truncated, "nothing rendered was left out");
        assert_eq!(
            page.next_sequence, 900,
            "a complete page resumes at the head"
        );
    }
}
