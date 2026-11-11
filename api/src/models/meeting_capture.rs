// Copyright © 2026 Jalapeno Labs

//! A meeting's captures: each one record of the meeting, as its source gave it. See
//! `docs/meetings.md`, Meetings and captures.
//!
//! A meeting (`models::meeting`) is the one event that happened; a capture is one record of it, a
//! connector's meeting today and an upload later. A capture keeps what its source gave, as it
//! came: the transcript and the recording, with the hash of each. Both are written once, by its
//! fetch ([`record_fetched`]), and never again, so every later step reads what the first fetch
//! stored. `UNIQUE (connector_id, external_id)` is what keeps a connector from recording one of
//! its meetings twice.
//!
//! A capture runs on its own clock, the one its transcript and recording are timed by. Its
//! `offset_ms` places that clock on the meeting's: a moment `t` of the capture is
//! `t + offset_ms` on the meeting's clock, which is the clock the final transcript is on.
//!
//! A meeting holds one capture until captures can be merged (#238); [`first_of`] is the one the
//! steps read until then.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::meeting_captures;

/// One capture of a meeting.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = meeting_captures, check_for_backend(diesel::pg::Pg))]
pub struct MeetingCapture {
    pub id: Uuid,
    pub meeting_id: Uuid,
    /// The connector it was read through; `None` for an upload, or once the connector is removed.
    pub connector_id: Option<Uuid>,
    /// The source it came from, such as `fireflies`.
    pub source_kind: String,
    /// The source's own id for it; `None` for an upload.
    pub external_id: Option<String>,
    /// Its page at the source.
    pub url: Option<String>,
    /// An array of `crate::meetings::source::TranscriptLine`, timed on the capture's own clock.
    pub transcript: Value,
    /// The SHA-256 of `transcript` as stored, as hex; `None` until it is fetched.
    pub transcript_hash: Option<String>,
    pub storage_location_id: Option<Uuid>,
    pub recording_path: Option<String>,
    pub recording_content_type: Option<String>,
    /// The SHA-256 of the recording's bytes as downloaded, as hex; `None` for a recording kept
    /// before captures were hashed.
    pub recording_hash: Option<String>,
    /// Where its clock falls on the meeting's, in milliseconds.
    pub offset_ms: i64,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MeetingCapture {
    /// Where its recording is stored, if it is.
    pub fn stored_recording(&self) -> Option<StoredRecording> {
        Some(StoredRecording {
            storage_location_id: self.storage_location_id?,
            path: self.recording_path.clone()?,
            content_type: self.recording_content_type.clone(),
        })
    }

    /// Whether its fetch stored what its source gave.
    pub const fn is_fetched(&self) -> bool {
        self.transcript_hash.is_some()
    }
}

/// Where a capture's recording was stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredRecording {
    pub storage_location_id: Uuid,
    pub path: String,
    pub content_type: Option<String>,
}

/// A capture as its source listed it, before its fetch.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_captures)]
pub struct NewMeetingCapture {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub connector_id: Option<Uuid>,
    pub source_kind: String,
    pub external_id: Option<String>,
    pub url: Option<String>,
    pub created_by: Uuid,
}

/// Stores `capture`, answering `None`, writing nothing, when its connector already recorded the
/// same one.
///
/// # Errors
/// Propagates any database error, such as a meeting that does not exist.
pub async fn insert_new(
    connection: &mut AsyncPgConnection,
    capture: NewMeetingCapture,
) -> QueryResult<Option<MeetingCapture>> {
    diesel::insert_into(meeting_captures::table)
        .values(capture)
        .on_conflict((
            meeting_captures::connector_id,
            meeting_captures::external_id,
        ))
        .do_nothing()
        .returning(MeetingCapture::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// The external ids among `external_ids` that the connector already recorded, so a listing can
/// skip them without fetching each one.
///
/// # Errors
/// Propagates any database error.
pub async fn known_external_ids(
    connection: &mut AsyncPgConnection,
    connector_id: Uuid,
    external_ids: &[String],
) -> QueryResult<Vec<String>> {
    let known: Vec<Option<String>> = meeting_captures::table
        .filter(meeting_captures::connector_id.eq(connector_id))
        .filter(meeting_captures::external_id.eq_any(external_ids))
        .select(meeting_captures::external_id)
        .load(connection)
        .await?;
    Ok(known.into_iter().flatten().collect())
}

/// Capture `id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<MeetingCapture> {
    meeting_captures::table
        .find(id)
        .select(MeetingCapture::as_select())
        .first(connection)
        .await
}

/// The first capture of each of `meeting_ids` that has one, in no order: what a list of meetings
/// says of each one's source.
///
/// # Errors
/// Propagates any database error.
pub async fn first_of_each(
    connection: &mut AsyncPgConnection,
    meeting_ids: &[Uuid],
) -> QueryResult<Vec<MeetingCapture>> {
    meeting_captures::table
        .filter(meeting_captures::meeting_id.eq_any(meeting_ids))
        .distinct_on(meeting_captures::meeting_id)
        .order((
            meeting_captures::meeting_id,
            meeting_captures::created_at.asc(),
            meeting_captures::id.asc(),
        ))
        .select(MeetingCapture::as_select())
        .load(connection)
        .await
}

/// Meeting `meeting_id`'s first capture: the one a meeting holds until captures can be merged.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the meeting has none, which only a meeting that does
/// not exist can have: a meeting is stored with its first capture.
pub async fn first_of(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<MeetingCapture> {
    meeting_captures::table
        .filter(meeting_captures::meeting_id.eq(meeting_id))
        .order((
            meeting_captures::created_at.asc(),
            meeting_captures::id.asc(),
        ))
        .select(MeetingCapture::as_select())
        .first(connection)
        .await
}

/// What a capture's first fetch stores.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// An array of `crate::meetings::source::TranscriptLine`.
    pub transcript: Value,
    /// The SHA-256 of `transcript`, as hex.
    pub transcript_hash: String,
    /// Where its recording was put, or `None` when it has none to keep.
    pub recording: Option<KeptRecording>,
}

/// A recording a fetch kept: where, and the hash of its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptRecording {
    pub stored: StoredRecording,
    /// The SHA-256 of its bytes as downloaded, as hex.
    pub hash: String,
}

/// Stores capture `id`'s first fetch, its transcript and where its recording is kept. Answers
/// `None`, writing nothing, when it was fetched already: the transcript a capture was first
/// fetched with is the one every later step reads, so nothing a later fetch finds replaces it.
///
/// # Errors
/// Propagates any database error.
pub async fn record_fetched(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    fetched: Fetched,
) -> QueryResult<Option<MeetingCapture>> {
    let (storage_location_id, recording_path, recording_content_type, recording_hash) =
        match fetched.recording {
            Some(kept) => (
                Some(kept.stored.storage_location_id),
                Some(kept.stored.path),
                kept.stored.content_type,
                Some(kept.hash),
            ),
            None => (None, None, None, None),
        };
    diesel::update(meeting_captures::table.find(id))
        .filter(meeting_captures::transcript_hash.is_null())
        .set((
            meeting_captures::transcript.eq(fetched.transcript),
            meeting_captures::transcript_hash.eq(Some(fetched.transcript_hash)),
            meeting_captures::storage_location_id.eq(storage_location_id),
            meeting_captures::recording_path.eq(recording_path),
            meeting_captures::recording_content_type.eq(recording_content_type),
            meeting_captures::recording_hash.eq(recording_hash),
        ))
        .returning(MeetingCapture::as_returning())
        .get_result(connection)
        .await
        .optional()
}
