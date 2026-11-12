// Copyright © 2026 Jalapeno Labs

//! Done reviewing: a person says their review of a meeting is done, and Elysium segments it again
//! from its corrected words. See `docs/meetings.md`, Done reviewing.
//!
//! One transaction, under the meeting's lock, records the change in its history (`review_done`),
//! forgets what segmenting read of it and moves it back to `recorded`, and starts a run of the
//! meeting workflow at segmenting, as the person (`super::workflow::run_from_segmenting`). That
//! run reads the words as they now stand and takes its excerpts and black-outs again
//! (`super::excerpts::take`): every one a person made or changed stands, Elysia's own are taken
//! again or removed, and only a project an excerpt is newly on wakes its Project Manager. Edits
//! never wait for this: they refresh the excerpts over them at once (`super::excerpts::refresh`),
//! and what Done reviewing adds is reading the meeting whole again, and cutting the clips of the
//! excerpts whose words moved.

use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::json;
use uuid::Uuid;

use super::workflow;
use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting, MeetingState};
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::workflow_run::{self, Started, WorkflowRun};

/// Why a meeting could not be segmented again.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    /// Its captures were merged into another meeting, which is reviewed in their place.
    #[error("the meeting was merged into another")]
    Merged,
    /// It was never segmented, so its first segmenting is still to come.
    #[error("the meeting has not been segmented yet")]
    NotSegmented,
    /// A run of it is going, held, or failed and waiting for a person.
    #[error("the meeting has a run of its own")]
    Busy(Box<WorkflowRun>),
    /// What it holds could not be read into its run's input.
    #[error(transparent)]
    Input(anyhow::Error),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// What saying a review is done did.
#[derive(Debug)]
pub struct Reviewed {
    /// The meeting, back to `recorded` while it is segmented again.
    pub meeting: Meeting,
    /// Its history entry (`review_done`).
    pub entry: MeetingEvent,
    /// The run that segments it again.
    pub started: Started,
}

/// Segments `meeting` again, as `person`, who is done reviewing it. The caller decides who may.
///
/// # Errors
/// [`ReviewError::Merged`] for a meeting merged away, [`ReviewError::NotSegmented`] for one never
/// segmented, [`ReviewError::Busy`] while a run of it is live; any database error.
pub async fn done_reviewing(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    person: Uuid,
) -> Result<Reviewed, ReviewError> {
    connection
        .transaction(async |connection| {
            let locked = meeting::lock(connection, meeting.id).await?;
            if locked.merged_into.is_some() {
                return Err(ReviewError::Merged);
            }
            if let Some(live) = workflow::live_run(connection, locked.id).await? {
                return Err(ReviewError::Busy(Box::new(live)));
            }
            if locked.state() != MeetingState::Done {
                return Err(ReviewError::NotSegmented);
            }
            let at = Utc::now();
            let author = Author {
                user_id: person,
                agent: None,
                at,
            };
            let entry = meeting_event::record(
                connection,
                Change {
                    id: Uuid::now_v7(),
                    meeting_id: locked.id,
                    kind: EventKind::ReviewDone,
                    author,
                    data: json!({}),
                    undid: None,
                },
            )
            .await?;
            let reopened = meeting::reopen_for_segmenting(connection, locked.id).await?;
            let new_run = workflow::run_from_segmenting(connection, &reopened, person)
                .await
                .map_err(ReviewError::Input)?;
            let started = workflow_run::start(connection, new_run, at).await?;
            if !started.created {
                // A run became the meeting's since the look above (a meeting holds one live run),
                // so it stands and this transaction, the history entry with it, is rolled back.
                return Err(ReviewError::Busy(Box::new(started.settled.run)));
            }
            Ok(Reviewed {
                meeting: reopened,
                entry,
                started,
            })
        })
        .await
}
