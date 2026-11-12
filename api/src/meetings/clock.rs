// Copyright © 2026 Jalapeno Labs

//! Where a capture sits on its meeting's clock, set by a person. See `docs/meetings.md`, One
//! clock.
//!
//! Aligning finds each capture's offset on its own (`super::align`), and a person nudges one by
//! hand from the meeting's options when it is off ([`nudge`]). A hand-set offset is recorded in
//! the meeting's history, its transcript is lined up again at once, and aligning never changes it
//! after. Undoing it ([`undo`]) puts back the offset it replaced, as long as nothing moved the
//! capture since.

use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::final_transcript;
use super::history::UndoError;
use crate::models::agent::Author;
use crate::models::meeting;
use crate::models::meeting_capture::{self, MeetingCapture, OffsetSource};
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};

/// The furthest a capture may sit from its meeting's clock, either way: twelve hours, more than any
/// meeting runs, so an offset typed with a slip of the keyboard is refused rather than throwing a
/// capture's words out past the end of every meeting.
pub const OFFSET_MAX_MS: i64 = 12 * 60 * 60 * 1_000;

/// A capture's offset, and how it was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offset {
    pub offset_ms: i64,
    pub offset_source: OffsetSource,
}

impl Offset {
    fn of(capture: &MeetingCapture) -> Self {
        Self {
            offset_ms: capture.offset_ms,
            offset_source: capture.offset_source(),
        }
    }
}

/// What a hand-set offset changed, as the meeting's `offset_changed` entry records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetChange {
    pub capture_id: Uuid,
    pub before: Offset,
    pub after: Offset,
}

/// Why an offset could not be set.
#[derive(Debug, thiserror::Error)]
pub enum NudgeError {
    /// The meeting has no such capture.
    #[error("not found")]
    NotFound,
    /// The offset lies further from the meeting's clock than [`OFFSET_MAX_MS`].
    #[error("an offset is at most twelve hours either way")]
    OutOfRange,
    /// The meeting is deleted, so it changes no further until it is restored.
    #[error("the meeting is deleted")]
    Deleted,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    /// The meeting's transcripts could not be read to line it up again.
    #[error(transparent)]
    Transcript(#[from] anyhow::Error),
}

/// Sets capture `capture_id` of meeting `meeting_id` `offset_ms` from the meeting's clock by hand,
/// as `author`, records it, and lines the meeting's transcript up again, in one transaction under
/// the meeting's lock. Who may is the caller's to decide. Answers the history entry.
///
/// # Errors
/// [`NudgeError::NotFound`] for a capture that is not the meeting's, [`NudgeError::OutOfRange`]
/// for an offset past [`OFFSET_MAX_MS`], [`NudgeError::Deleted`] while the meeting is deleted; any
/// database error.
pub async fn nudge(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    capture_id: Uuid,
    offset_ms: i64,
    author: Author<'_>,
) -> Result<MeetingEvent, NudgeError> {
    if !(-OFFSET_MAX_MS..=OFFSET_MAX_MS).contains(&offset_ms) {
        return Err(NudgeError::OutOfRange);
    }
    connection
        .transaction(async |connection| {
            match meeting::lock(connection, meeting_id).await {
                // Read under the lock a deletion takes, so no offset moves on a meeting deleted.
                Ok(locked) if locked.deleted_at.is_some() => return Err(NudgeError::Deleted),
                Ok(_locked) => {}
                Err(diesel::result::Error::NotFound) => return Err(NudgeError::NotFound),
                Err(error) => return Err(error.into()),
            }
            let capture = match meeting_capture::find(connection, capture_id).await {
                Ok(capture) if capture.meeting_id == meeting_id => capture,
                // Another meeting's capture is no capture of this one.
                Ok(_) | Err(diesel::result::Error::NotFound) => return Err(NudgeError::NotFound),
                Err(error) => return Err(error.into()),
            };
            let change = OffsetChange {
                capture_id,
                before: Offset::of(&capture),
                after: Offset {
                    offset_ms,
                    offset_source: OffsetSource::Hand,
                },
            };
            let entry = apply(connection, meeting_id, change, author, None).await?;
            Ok(entry)
        })
        .await
}

/// Undoes `undone`, an `offset_changed` entry, as `author`: its capture goes back to the offset it
/// had, and the transcript is lined up again. The caller holds the meeting's lock.
///
/// # Errors
/// [`UndoError::ChangedSince`] once the capture moved since, by another offset or a merge;
/// [`UndoError::NotUndoable`] for an entry not in its shape; any database error.
pub(crate) async fn undo(
    connection: &mut AsyncPgConnection,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> Result<MeetingEvent, UndoError> {
    let Ok(change) = serde_json::from_value::<OffsetChange>(undone.data.clone()) else {
        tracing::event!(
            name: "meetings.history.unreadable",
            tracing::Level::WARN,
            meeting_event.id = %undone.id,
            "an offset's history entry is not in its shape; it cannot be undone",
        );
        return Err(UndoError::NotUndoable);
    };
    let capture = meeting_capture::find(connection, change.capture_id).await?;
    if capture.meeting_id != undone.meeting_id || Offset::of(&capture) != change.after {
        return Err(UndoError::ChangedSince);
    }
    let inverse = OffsetChange {
        capture_id: change.capture_id,
        before: change.after,
        after: change.before,
    };
    apply(
        connection,
        undone.meeting_id,
        inverse,
        author,
        Some(undone.id),
    )
    .await
    .map_err(|error| match error {
        NudgeError::Database(error) => UndoError::Database(error),
        NudgeError::Transcript(error) => UndoError::Transcript(error),
        NudgeError::NotFound | NudgeError::OutOfRange => UndoError::ChangedSince,
        // Only `nudge` reads the meeting; an undo is refused for a deleted one before it is here.
        NudgeError::Deleted => UndoError::Deleted,
    })
}

/// Records `change` in meeting `meeting_id`'s history as `author`, the undo of `undid` when it is
/// one, moves its capture, and lines the transcript up again.
async fn apply(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    change: OffsetChange,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> Result<MeetingEvent, NudgeError> {
    let entry = meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id,
            kind: EventKind::OffsetChanged,
            author,
            data: serde_json::to_value(change).expect("an offset change is plain fields"),
            undid,
        },
    )
    .await?;
    meeting_capture::set_offset(
        connection,
        change.capture_id,
        change.after.offset_ms,
        change.after.offset_source,
    )
    .await?;
    final_transcript::rebuild(connection, meeting_id, author).await?;
    Ok(entry)
}
