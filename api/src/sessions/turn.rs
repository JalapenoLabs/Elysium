// Copyright © 2026 Jalapeno Labs

//! Following one turn of a session, from the runner's first event to its last.
//!
//! Each event is scrubbed of the turn's secrets, given the session's next sequence, recorded,
//! and only then published, so a client that refetches history after a live event always finds
//! it. When the turn starts and ends, the session's activity changes and is announced. When it
//! ends, the Claude session it reported is kept on the row for the next turn to resume, a Studio
//! session's deliverables are collected, and the turn's tool grant is dropped with the follower.

use chrono::Utc;
use tokio::sync::mpsc;
use tracing::{Level, event};
use uuid::Uuid;

use super::scrub::Scrubber;
use super::views::{ActivityState, SessionEvent};
use super::{Sessions, lock};
use crate::models::coding_session;
use crate::models::session_event::{self, StoredEvent};
use crate::realtime::ServerEvent;
use crate::runner::{RunnerEvent, TurnOutcome};
use crate::studio::artifacts::{self, Services, Source};
use crate::tools::grants::Grant;

/// One turn being followed.
#[derive(Debug)]
pub(super) struct Follower {
    pub sessions: Sessions,
    pub session_id: i64,
    pub studio_item_id: Option<Uuid>,
    pub turn_id: Uuid,
    pub scrubber: Scrubber,
    /// Held until the turn ends: dropping it revokes the turn's access to Elysium's tools.
    pub grant: Grant,
}

impl Follower {
    /// Records and publishes `events` until the turn's last.
    pub async fn follow(self, mut events: mpsc::Receiver<RunnerEvent>) {
        while let Some(runner_event) = events.recv().await {
            let runner_event = self.scrubber.scrub(runner_event);
            match &runner_event {
                RunnerEvent::TurnStarted { .. } => self.started().await,
                RunnerEvent::Finished(outcome) => {
                    self.record(runner_event.clone()).await;
                    self.finished(outcome).await;
                    return;
                }
                _ => {}
            }
            self.record(runner_event).await;
        }
        // The runner always sends `Finished` last; a channel that closes without it means the
        // runner itself went away, which only a panic inside it can cause.
        event!(
            name: "sessions.turn.unfinished",
            Level::ERROR,
            session.id = self.session_id,
            turn.id = %self.turn_id,
            "a turn's events ended without its outcome",
        );
        self.finished(&TurnOutcome::without_result(
            crate::runner::TurnStatus::Interrupted,
            "the turn ended without reporting how",
        ))
        .await;
    }

    async fn record(&self, runner_event: RunnerEvent) {
        let sequence = self.next_sequence().await;
        let occurred_at = Utc::now();
        let stored = StoredEvent {
            sequence,
            turn_id: Some(self.turn_id),
            occurred_at,
            event: runner_event,
        };
        let recorded = match self.sessions.inner.database.get().await {
            Ok(mut connection) => session_event::record(&mut connection, self.session_id, &stored)
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(message) = recorded {
            event!(
                name: "sessions.event.record_failure",
                Level::ERROR,
                session.id = self.session_id,
                event.sequence = sequence,
                error.message = %message,
                "could not record a session's event; it is shown live but missing from history",
            );
        }
        let view = SessionEvent::new(
            self.session_id,
            sequence,
            Some(self.turn_id),
            occurred_at,
            &stored.event,
        );
        self.sessions
            .inner
            .events
            .publish(&ServerEvent::SessionEvent(Box::new(view)));
    }

    /// The session's next sequence. The first turn after the API starts reads where the
    /// session's history ends; later ones count on from memory.
    async fn next_sequence(&self) -> u64 {
        let mut sequences = self.sessions.inner.sequences.lock().await;
        let last = match sequences.get(&self.session_id) {
            Some(last) => *last,
            None => match self.sessions.inner.database.get().await {
                Ok(mut connection) => {
                    session_event::latest_sequence(&mut connection, self.session_id)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(0)
                }
                Err(_pool_error) => 0,
            },
        };
        let next = last + 1;
        sequences.insert(self.session_id, next);
        next
    }

    async fn started(&self) {
        self.sessions.update(self.session_id, |live| {
            live.activity.state = ActivityState::Running;
            live.activity.queue_depth = live.activity.queue_depth.saturating_sub(1);
            live.activity.current_turn_id = Some(self.turn_id);
            live.activity.last_activity_at = Some(Utc::now());
        });
        self.sessions.announce(self.session_id).await;
    }

    async fn finished(&self, outcome: &TurnOutcome) {
        if let Some(harness_session_id) = &outcome.harness_session_id {
            self.keep_harness_session(harness_session_id.as_str()).await;
        }
        if let Some(studio_item_id) = self.studio_item_id {
            self.collect_deliverables(studio_item_id).await;
        }

        {
            let mut live = lock(&self.sessions.inner.live);
            if let Some(entry) = live.get_mut(&self.session_id) {
                entry.unfinished.retain(|turn_id| *turn_id != self.turn_id);
                entry.activity.current_turn_id = None;
                entry.activity.last_activity_at = Some(Utc::now());
                entry.activity.state = if entry.activity.queue_depth > 0 {
                    ActivityState::Queued
                } else {
                    ActivityState::Idle
                };
            }
        }
        self.sessions.announce(self.session_id).await;
    }

    async fn keep_harness_session(&self, harness_session_id: &str) {
        let kept = match self.sessions.inner.database.get().await {
            Ok(mut connection) => coding_session::set_harness_session_id(
                &mut connection,
                self.session_id,
                harness_session_id,
            )
            .await
            .map(drop)
            .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(message) = kept {
            event!(
                name: "sessions.harness_session.unrecorded",
                Level::WARN,
                session.id = self.session_id,
                error.message = %message,
                "could not keep the Claude session a turn reported; the next turn starts a new one",
            );
        }
    }

    async fn collect_deliverables(&self, studio_item_id: Uuid) {
        let inner = &self.sessions.inner;
        let services = Services {
            database: &inner.database,
            cipher: &inner.cipher,
            storage: &inner.storage,
            events: &inner.events,
        };
        let directory = Sessions::directory(self.session_id);
        let source = Source {
            session_id: self.session_id,
            studio_item_id,
            workspace: &inner.workspace,
            directory: &directory,
        };
        artifacts::collect(services, source).await;
    }
}
