// Copyright © 2026 Jalapeno Labs

//! A meeting's history: every change to it, with who made it. See `docs/meetings.md`, History.
//!
//! Each change is one row, shaped like a page's history (`page_events`): its kind, who made it and
//! the agent acting for them, and a JSON object shaped by its kind. The changes that write the
//! final transcript (`models::meeting_word`) are also what its words name: the change that wrote
//! a word, and the one that replaced it. An undo is a change of its own naming the one it reversed
//! (`undid`), so it can be undone in turn; which kinds can be undone, and how, is
//! `crate::meetings::history`'s.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::meeting_events;
use crate::models::agent::Author;

/// The kinds of change a meeting's history records. Stored as text, so a later kind needs no
/// migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// Its final transcript was lined up from its captures (`models::meeting_word::Rebuilt`):
    /// first built, or built again because its captures, their offsets, or its corrected copy
    /// changed, keeping every edit. A transcript built before captures were aligned records
    /// `{ captureId, builtFrom, words }` instead.
    Built,
    /// A range of its words was replaced (`models::meeting_word::WordChange`).
    Edited,
    /// A mark was made, or put back: `{ markId, kind, startWordId, endWordId, data }`.
    Marked,
    /// A mark was removed: `{ markId, kind }`.
    Unmarked,
    /// Whether it waits for its person's approval changed: `{ before, after }`.
    HoldChanged,
    /// Its corrected transcript was approved: `{}`.
    CorrectionApproved,
    /// A person set a capture's offset onto its clock by hand: `{ captureId, before, after }`,
    /// each `{ offsetMs, offsetSource }`.
    OffsetChanged,
    /// One meeting's captures were merged into another (`crate::meetings::duplicates::Moved`),
    /// recorded in both.
    Merged,
    /// Captures merged in were split back out into their meeting, recorded in both, as a merge's
    /// undo or the undo of one.
    Split,
}

impl EventKind {
    const ALL: [Self; 9] = [
        Self::Built,
        Self::Edited,
        Self::Marked,
        Self::Unmarked,
        Self::HoldChanged,
        Self::CorrectionApproved,
        Self::OffsetChanged,
        Self::Merged,
        Self::Split,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Built => "built",
            Self::Edited => "edited",
            Self::Marked => "marked",
            Self::Unmarked => "unmarked",
            Self::HoldChanged => "hold_changed",
            Self::CorrectionApproved => "correction_approved",
            Self::OffsetChanged => "offset_changed",
            Self::Merged => "merged",
            Self::Split => "split",
        }
    }

    /// The kind stored as `text`, if this build knows it.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// One change to a meeting.
#[derive(Debug, Clone, PartialEq, Queryable, Selectable)]
#[diesel(table_name = meeting_events, check_for_backend(diesel::pg::Pg))]
pub struct MeetingEvent {
    pub id: Uuid,
    pub meeting_id: Uuid,
    /// An [`EventKind`] name. Kept as text so entries written by a later version still load.
    pub kind: String,
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    /// A JSON object shaped by `kind`, and `undid` for an undo.
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

impl MeetingEvent {
    /// Its kind, when this build knows it.
    pub fn kind(&self) -> Option<EventKind> {
        EventKind::parse(&self.kind)
    }

    /// The change it undid, when it is an undo.
    pub fn undid(&self) -> Option<Uuid> {
        self.data
            .get("undid")
            .and_then(Value::as_str)
            .and_then(|undid| undid.parse().ok())
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_events)]
struct NewMeetingEvent {
    id: Uuid,
    meeting_id: Uuid,
    kind: String,
    created_by: Uuid,
    agent: Option<String>,
    agent_name: Option<String>,
    data: Value,
    created_at: DateTime<Utc>,
}

/// A change about to be recorded.
#[derive(Debug, Clone)]
pub struct Change<'author> {
    /// The id the entry is stored under, chosen beforehand so what the change writes can name it.
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub kind: EventKind,
    pub author: Author<'author>,
    /// A JSON object shaped by `kind`.
    pub data: Value,
    /// The change this one undoes, when it is an undo.
    pub undid: Option<Uuid>,
}

/// Records `change` in its meeting's history, and answers the entry.
///
/// # Errors
/// Propagates any database error, such as a meeting that does not exist.
pub async fn record(
    connection: &mut AsyncPgConnection,
    change: Change<'_>,
) -> QueryResult<MeetingEvent> {
    let mut data = change.data;
    if let (Some(undid), Some(object)) = (change.undid, data.as_object_mut()) {
        object.insert("undid".to_owned(), Value::String(undid.to_string()));
    }
    diesel::insert_into(meeting_events::table)
        .values(NewMeetingEvent {
            id: change.id,
            meeting_id: change.meeting_id,
            kind: change.kind.as_str().to_owned(),
            created_by: change.author.user_id,
            agent: change.author.agent.map(|agent| agent.id().to_owned()),
            agent_name: change
                .author
                .agent
                .and_then(|agent| agent.name().map(ToOwned::to_owned)),
            data,
            created_at: change.author.at,
        })
        .returning(MeetingEvent::as_returning())
        .get_result(connection)
        .await
}

/// Entry `id` of meeting `meeting_id`'s history.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the meeting has no such entry.
pub async fn find(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    id: Uuid,
) -> QueryResult<MeetingEvent> {
    meeting_events::table
        .find(id)
        .filter(meeting_events::meeting_id.eq(meeting_id))
        .select(MeetingEvent::as_select())
        .first(connection)
        .await
}

/// Meeting `meeting_id`'s history, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn history(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<MeetingEvent>> {
    meeting_events::table
        .filter(meeting_events::meeting_id.eq(meeting_id))
        .order(meeting_events::id.asc())
        .select(MeetingEvent::as_select())
        .load(connection)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_stored_as_text_it_parses() {
        for kind in EventKind::ALL {
            assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
            assert!(
                kind.as_str()
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '_'),
                "{} fits meeting_events_kind_check",
                kind.as_str()
            );
        }
        assert_eq!(EventKind::parse("sideways"), None);
    }
}
