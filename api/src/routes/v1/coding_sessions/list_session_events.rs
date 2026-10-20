// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/coding-sessions/{id}/events`: the session's kept event history.
//!
//! Elysium records every event a session's turns report (`crate::sessions`), so history is
//! read from Postgres. Events after the response arrive live on the
//! event stream; clients merge the two on `sequence`. Only the latest [`HISTORY_LIMIT`] are
//! returned.

use std::collections::VecDeque;

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde::Serialize;
use serde_json::{Value, json};

use crate::errors::ApiError;
use crate::models::session_event::StoredEvent;
use crate::models::{coding_session, session_event};
use crate::sessions::views::SessionEvent;
use crate::state::AppState;

/// Most events one response carries. A long session's streamed output runs to
/// thousands of small events; the conversation view needs the recent end of it.
const HISTORY_LIMIT: usize = 5_000;

/// Which end of a window to keep when more events match than the limit allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// The most recent, for a view of what is happening now. Older events are dropped.
    Latest,
    /// The oldest, for a reader paging forward from a cursor, so no event is skipped.
    Earliest,
}

/// Which of a session's events to read.
#[derive(Debug, Clone, Copy)]
pub struct EventWindow {
    /// Only events after this sequence; `0` reads from the start.
    pub after_sequence: u64,
    /// The most events answered.
    pub limit: usize,
    /// Which end of the window is answered when more events match than `limit`.
    pub keep: Keep,
    /// Keeps only events Elysium renders (messages, tools, turns, incidents), leaving out
    /// bookkeeping such as rate limit notices.
    pub rendered_only: bool,
}

/// A slice of a session's history, oldest first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub events: Vec<SessionEvent>,
    /// Whether more events matched than were answered.
    pub truncated: bool,
    /// The session's latest sequence when the read began.
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

/// Decides, event by event, what a page keeps. Kept apart from the database read so the
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

/// Reads the session `id`'s recorded events over `window`, up to the latest recorded when the
/// read began.
///
/// # Errors
/// `404` for an unknown session, or an internal error when the database fails.
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
    // Answers 404 for a session that does not exist, rather than an empty history.
    coding_session::find(&mut connection, id).await?;
    let latest_sequence = session_event::latest_sequence(&mut connection, id)
        .await?
        .unwrap_or_default();
    let mut collector = EventCollector::new(window);
    if latest_sequence <= window.after_sequence {
        return Ok(collector.finish(latest_sequence));
    }

    // The conversation view's read, every event from the start keeping the latest, is the
    // common one and the costliest; the database answers it with only the rows it keeps.
    if window.after_sequence == 0 && window.keep == Keep::Latest && !window.rendered_only {
        let (stored_events, truncated) =
            session_event::latest(&mut connection, id, window.limit).await?;
        let events = stored_events
            .iter()
            .map(|stored| view(id, stored))
            .collect();
        return Ok(EventPage {
            events,
            truncated,
            latest_sequence,
            next_sequence: latest_sequence,
        });
    }

    let stored_events = session_event::after(&mut connection, id, window.after_sequence).await?;
    for stored in &stored_events {
        if collector.offer(view(id, stored)) == Flow::Stop {
            break;
        }
    }
    Ok(collector.finish(latest_sequence))
}

/// The view clients render of a stored event.
fn view(session_id: i64, stored: &StoredEvent) -> SessionEvent {
    SessionEvent::new(
        session_id,
        stored.sequence,
        stored.turn_id,
        stored.occurred_at,
        &stored.event,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::views::EventPayload;

    fn event(sequence: u64, rendered: bool) -> SessionEvent {
        SessionEvent {
            session_id: 1,
            sequence,
            turn_id: None,
            occurred_at: None,
            event_type: "agent.message",
            payload: rendered.then(|| EventPayload::AgentMessage {
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
