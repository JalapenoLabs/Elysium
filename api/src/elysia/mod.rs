// Copyright © 2026 Jalapeno Labs

//! Elysia, Elysium's AI assistant, who starts work on her own.
//!
//! People talk to her through the conversation (`/api/v1/elysia/messages`). Everything that
//! starts her is a turn: a person's message or answer, a change in the workspace or a
//! provider, or her idle loop revisiting her work. Turns queue in `elysia_turns` and run one at
//! a time on the host runner, on her one Claude Code session, which each turn resumes so she
//! carries context. While she works she asks
//! questions, reports what she has taken on and when it will be ready, and proposes changesets
//! for anything a person must approve. See `docs/elysia.md`.
//!
//! - [`availability`]: the weekly hours she may start work on her own.
//! - [`status`]: the color she shows, and what holds her back.
//! - [`intake`]: which changes on the event bus become her `event` turns.
//! - [`dispatcher`]: runs the queue one turn at a time, and her idle loop.
//! - [`prompt`]: her standing instructions and each turn's prompt.
//! - [`runner`]: the seam to the host runner.
//! - [`trigger`]: what can start a turn.

pub mod availability;
pub mod dispatcher;
pub mod intake;
pub mod prompt;
pub mod runner;
pub mod status;
pub mod trigger;

use std::sync::Arc;

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use self::availability::AvailabilityWindow;
use self::dispatcher::DispatchContext;
use self::intake::IntakeContext;
use self::runner::TurnRunner;
use self::status::{Status, StatusInputs};
use crate::database::Pool;
use crate::models::elysia::{self, ElysiaState, Question, Turn, TurnStatus, Work};
use crate::realtime::EventBus;

/// Elysia's handle in [`crate::state::AppState`]. Cheap to clone; clones share one wake signal
/// and one runner.
#[derive(Debug, Clone)]
pub struct Elysia {
    /// Woken whenever a turn is queued or something that holds turns back changes, so the
    /// dispatcher looks at the queue at once instead of on its next tick.
    wake: Arc<Notify>,
    runner: TurnRunner,
}

impl Elysia {
    pub fn new(runner: TurnRunner) -> Self {
        Self {
            wake: Arc::new(Notify::new()),
            runner,
        }
    }

    /// What runs her turns.
    pub const fn runner(&self) -> &TurnRunner {
        &self.runner
    }

    /// Tells the dispatcher to look at the queue now.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Waits for [`Self::wake`].
    pub async fn woken(&self) {
        self.wake.notified().await;
    }

    /// Starts the intake and the dispatcher, which run until `shutdown`.
    pub fn start(&self, database: Pool, events: EventBus, shutdown: CancellationToken) -> Tasks {
        let intake = tokio::spawn(intake::run(
            IntakeContext {
                database: database.clone(),
                events: events.clone(),
                elysia: self.clone(),
            },
            shutdown.clone(),
        ));
        let dispatcher = tokio::spawn(dispatcher::run(
            DispatchContext {
                database,
                events,
                elysia: self.clone(),
            },
            shutdown,
        ));
        Tasks { intake, dispatcher }
    }
}

/// Elysia's background tasks, awaited at shutdown.
#[derive(Debug)]
pub struct Tasks {
    intake: JoinHandle<()>,
    dispatcher: JoinHandle<()>,
}

impl Tasks {
    /// Waits for both to stop, which they do once the shutdown token is cancelled.
    pub async fn stopped(self) {
        for (name, task) in [("intake", self.intake), ("dispatcher", self.dispatcher)] {
            if let Err(error) = task.await {
                event!(
                    name: "elysia.task.join.failure",
                    Level::ERROR,
                    elysia.task = name,
                    error.message = %error,
                    "an Elysia task ended abnormally",
                );
            }
        }
    }
}

/// Everything the interface shows about Elysia at a glance.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub state: ElysiaState,
    pub availability: Vec<AvailabilityWindow>,
    pub status: Status,
    /// Queued and running turns, oldest first.
    pub active_turns: Vec<Turn>,
    pub open_questions: Vec<Question>,
    pub work: Vec<Work>,
}

/// Reads the stored schedule, treating one that does not parse as no schedule.
///
/// Only [`availability::validate`]d schedules are ever written, so a failure here means the
/// row was edited by hand; it is logged rather than taking Elysia down.
pub fn stored_availability(state: &ElysiaState) -> Vec<AvailabilityWindow> {
    match serde_json::from_value(state.availability.clone()) {
        Ok(windows) => windows,
        Err(error) => {
            event!(
                name: "elysia.availability.unreadable",
                Level::WARN,
                error.message = %error,
                "the stored availability does not parse; treating it as any time",
            );
            Vec::new()
        }
    }
}

/// Reads Elysia's state, turns, questions, and work, and derives her status.
///
/// # Errors
/// Propagates any database error.
pub async fn snapshot(connection: &mut AsyncPgConnection) -> diesel::QueryResult<Snapshot> {
    let state = elysia::state(connection).await?;
    let active_turns = elysia::active_turns(connection).await?;
    let last_finished = elysia::last_finished_turn(connection).await?;
    let open_questions = elysia::open_questions(connection).await?;
    let work = elysia::unfinished_work(connection).await?;

    let availability = stored_availability(&state);
    let now = Utc::now();
    let status = status::derive(&StatusInputs {
        is_running: active_turns
            .iter()
            .any(|turn| turn.status == TurnStatus::Running),
        last_turn_failed: last_finished.is_some_and(|turn| turn.status == TurnStatus::Failed),
        open_questions: open_questions.len(),
        paused: state.paused,
        usage_paused_until: state.usage_paused_until,
        available: availability::is_available(&availability, now),
        now,
    });

    Ok(Snapshot {
        state,
        availability,
        status,
        active_turns,
        open_questions,
        work,
    })
}
