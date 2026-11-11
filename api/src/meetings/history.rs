// Copyright © 2026 Jalapeno Labs

//! Undoing a change to a meeting. See `docs/meetings.md`, History.
//!
//! Every change to a meeting is in its history (`models::meeting_event`), with who made it. An
//! undo reverses one change and is recorded as a change of its own naming it (`undid`), so it can
//! be undone in turn. Only a change whose effect still stands is undone, so a later change is
//! never overwritten:
//!
//! | Change                | Its undo                                                         |
//! |-----------------------|------------------------------------------------------------------|
//! | `edited`              | Stands the replaced words up again and moves its marks back (`models::meeting_word`) |
//! | `marked`, `unmarked`  | Removes the mark, or puts it back (`models::meeting_mark`)        |
//! | `hold_changed`        | Sets the hold back, while the transcript is not yet corrected, as holding does |
//! | `offset_changed`      | Puts the capture's offset back, while nothing moved it since (`super::clock`) |
//! | `merged`              | Splits the captures back out (`super::duplicates::split`)        |
//! | `split`               | Merges them again (`super::duplicates::merge_again`)             |
//! | `built`               | None: what caused a build is undone, and builds it again         |
//! | `correction_approved` | None: approving let the meeting's run go on                      |
//! | `capture_added`       | None: the capture is the meeting's once its run takes it in      |
//!
//! A merge and a split are recorded in both meetings' histories, and undone from the history of
//! the meeting the captures went into.

use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_correction;
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::{meeting_mark, meeting_word};

/// Why a change could not be undone.
#[derive(Debug, thiserror::Error)]
pub enum UndoError {
    /// The meeting has no such change.
    #[error("not found")]
    NotFound,
    /// Its kind cannot be undone: a build, or an approval.
    #[error("this change cannot be undone")]
    NotUndoable,
    /// What it changed moved on since, so undoing it would overwrite the later change.
    #[error("what it changed has changed since")]
    ChangedSince,
    /// A hold no longer changes once the meeting's transcript is corrected.
    #[error("the meeting's transcript is corrected already")]
    Corrected,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    /// The meeting's transcripts could not be read to line it up again.
    #[error(transparent)]
    Transcript(anyhow::Error),
}

/// Undoes change `event_id` of meeting `meeting_id` as `author`, under the meeting's lock, and
/// answers the undo's own history entry. Who may undo is the caller's to decide.
///
/// # Errors
/// [`UndoError::NotFound`] when the meeting has no such change, [`UndoError::NotUndoable`] for a
/// kind that cannot be undone, [`UndoError::ChangedSince`] when what it changed moved on since,
/// [`UndoError::Corrected`] for a hold once the transcript is corrected; any database error.
pub async fn undo(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    event_id: Uuid,
    author: Author<'_>,
) -> Result<MeetingEvent, UndoError> {
    connection
        .transaction(async |connection| {
            let current = match meeting::lock(connection, meeting_id).await {
                Ok(current) => current,
                Err(diesel::result::Error::NotFound) => return Err(UndoError::NotFound),
                Err(error) => return Err(UndoError::Database(error)),
            };
            let undone = match meeting_event::find(connection, meeting_id, event_id).await {
                Ok(undone) => undone,
                Err(diesel::result::Error::NotFound) => return Err(UndoError::NotFound),
                Err(error) => return Err(UndoError::Database(error)),
            };
            let undo = match undone.kind() {
                Some(EventKind::Edited) => meeting_word::undo(connection, &undone, author).await?,
                Some(EventKind::Marked | EventKind::Unmarked) => {
                    meeting_mark::undo(connection, &undone, author).await?
                }
                Some(EventKind::HoldChanged) => {
                    undo_hold(connection, &current, &undone, author).await?
                }
                Some(EventKind::OffsetChanged) => {
                    Some(super::clock::undo(connection, &undone, author).await?)
                }
                Some(EventKind::Merged) => {
                    Some(super::duplicates::split(connection, &current, &undone, author).await?)
                }
                Some(EventKind::Split) => Some(
                    super::duplicates::merge_again(connection, &current, &undone, author).await?,
                ),
                Some(
                    EventKind::Built | EventKind::CorrectionApproved | EventKind::CaptureAdded,
                )
                | None => {
                    return Err(UndoError::NotUndoable);
                }
            };
            undo.ok_or(UndoError::ChangedSince)
        })
        .await
}

/// Records that meeting `meeting_id`'s hold went from `before` to `after`, as `author`.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn record_hold(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    (before, after): (bool, bool),
    author: Author<'_>,
    undid: Option<Uuid>,
) -> diesel::QueryResult<MeetingEvent> {
    meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id,
            kind: EventKind::HoldChanged,
            author,
            data: json!({ "before": before, "after": after }),
            undid,
        },
    )
    .await
}

/// Sets `current`'s hold back to what `undone` changed it from, while it still holds what
/// `undone` set and its transcript is not yet corrected. `None` when the hold changed since.
async fn undo_hold(
    connection: &mut AsyncPgConnection,
    current: &Meeting,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> Result<Option<MeetingEvent>, UndoError> {
    let (Some(before), Some(after)) = (
        undone.data.get("before").and_then(Value::as_bool),
        undone.data.get("after").and_then(Value::as_bool),
    ) else {
        // Only an entry written by hand says neither.
        return Ok(None);
    };
    if current.hold_for_approval != after {
        return Ok(None);
    }
    if meeting_correction::find(connection, current.id)
        .await?
        .is_some()
    {
        return Err(UndoError::Corrected);
    }
    meeting::set_hold_for_approval(connection, current.id, before).await?;
    Ok(Some(
        record_hold(
            connection,
            current.id,
            (after, before),
            author,
            Some(undone.id),
        )
        .await?,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::meetings::correct::{self, HoldError};
    use crate::models::meeting::tests::fetched_with;
    use crate::models::meeting_correction::NewMeetingCorrection;
    use crate::models::meeting_word::tests::person;
    use crate::realtime::EventBus;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_hold_is_undone_until_the_transcript_is_corrected_and_an_approval_never_is() {
        let (_url, mut connection) = migrated_database().await;
        let events = EventBus::in_process();
        let (meeting, _capture) = fetched_with(&mut connection, json!([])).await;

        correct::hold(&mut connection, &events, meeting.id, TEST_PERSON_ID, true)
            .await
            .expect("held");
        let held = meeting_event::history(&mut connection, meeting.id)
            .await
            .expect("history")
            .pop()
            .expect("the hold");
        assert_eq!(held.kind(), Some(EventKind::HoldChanged));

        let released = undo(&mut connection, meeting.id, held.id, person())
            .await
            .expect("undoes");
        assert_eq!(released.data["before"], json!(true));
        assert_eq!(released.data["after"], json!(false));
        assert!(
            !meeting::find(&mut connection, meeting.id)
                .await
                .expect("found")
                .hold_for_approval
        );
        // The hold changed since, so undoing it again would overwrite that.
        assert!(matches!(
            undo(&mut connection, meeting.id, held.id, person()).await,
            Err(UndoError::ChangedSince)
        ));
        // Undoing the undo holds it again.
        undo(&mut connection, meeting.id, released.id, person())
            .await
            .expect("holds again");
        assert!(
            meeting::find(&mut connection, meeting.id)
                .await
                .expect("found")
                .hold_for_approval
        );

        // Once its copy is stored, the hold no longer changes, by hand or by undo.
        meeting_correction::record(
            &mut connection,
            NewMeetingCorrection {
                meeting_id: meeting.id,
                transcript: json!([]),
                transcript_hash: "c".repeat(64),
                based_on: "source".to_owned(),
                source_transcript_hash: "a".repeat(64),
                whisper_transcript_hash: None,
                changed_lines: 0,
                fixes: json!([]),
            },
        )
        .await
        .expect("records")
        .expect("new");
        let last = meeting_event::history(&mut connection, meeting.id)
            .await
            .expect("history")
            .pop()
            .expect("the hold again");
        assert!(matches!(
            undo(&mut connection, meeting.id, last.id, person()).await,
            Err(UndoError::Corrected)
        ));
        assert!(matches!(
            correct::hold(&mut connection, &events, meeting.id, TEST_PERSON_ID, false).await,
            Err(HoldError::Corrected)
        ));

        correct::approve(&mut connection, &events, meeting.id, TEST_PERSON_ID)
            .await
            .expect("approved");
        let approved = meeting_event::history(&mut connection, meeting.id)
            .await
            .expect("history")
            .pop()
            .expect("the approval");
        assert_eq!(approved.kind(), Some(EventKind::CorrectionApproved));
        assert!(matches!(
            undo(&mut connection, meeting.id, approved.id, person()).await,
            Err(UndoError::NotUndoable)
        ));
        assert!(matches!(
            undo(&mut connection, meeting.id, Uuid::now_v7(), person()).await,
            Err(UndoError::NotFound)
        ));
    }
}
