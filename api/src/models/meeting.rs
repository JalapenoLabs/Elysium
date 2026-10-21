// Copyright © 2026 Jalapeno Labs

//! Meetings read from a meeting source, and how far each one's processing got. See
//! `crate::meetings` and `docs/meetings.md`.
//!
//! A meeting belongs to whoever owns the account it was read from, like every timeline
//! record. It moves through `pending` (read), `recorded`, `segmented`, and `done`, one step
//! per pass of the processing loop, and is `failed` once a step has failed
//! [`MAX_ATTEMPTS`] times.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::meetings;

/// How many times one step may fail before the meeting is given up on. A failure is most
/// often a provider or a model that did not answer, which a later pass gets past; three
/// in a row is something a person has to look at.
pub const MAX_ATTEMPTS: i32 = 3;

/// How far a meeting's processing got. Stored as `meetings.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingState {
    /// Read from its source, with its transcript.
    Pending,
    /// Its recording is stored, or the source keeps none.
    Recorded,
    /// Elysia has cut its transcript into topic segments.
    Segmented,
    /// Its segments are on the timeline.
    Done,
    /// A step failed [`MAX_ATTEMPTS`] times.
    Failed,
}

impl MeetingState {
    const ALL: [Self; 5] = [
        Self::Pending,
        Self::Recorded,
        Self::Segmented,
        Self::Done,
        Self::Failed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Recorded => "recorded",
            Self::Segmented => "segmented",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    /// The state stored as `text`, if it is one.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == text)
    }
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = meetings, check_for_backend(diesel::pg::Pg))]
pub struct Meeting {
    pub id: Uuid,
    pub connector_account_id: Option<Uuid>,
    pub source_kind: String,
    pub external_id: String,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: Option<i64>,
    pub organizer: String,
    /// An array of strings.
    pub participants: Value,
    pub url: Option<String>,
    /// An array of `{ startMs, endMs, speaker, text }`; see `crate::meetings::TranscriptLine`.
    pub transcript: Value,
    pub storage_location_id: Option<Uuid>,
    pub recording_path: Option<String>,
    pub recording_content_type: Option<String>,
    /// The segments Elysia found; see `crate::meetings::segmentation::Segment`.
    pub segments: Option<Value>,
    /// See [`MeetingState`].
    pub state: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Meeting {
    /// The meeting's state. A value the constraint does not allow reads as failed.
    pub fn state(&self) -> MeetingState {
        MeetingState::parse(&self.state).unwrap_or(MeetingState::Failed)
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meetings)]
pub struct NewMeeting {
    pub id: Uuid,
    pub connector_account_id: Option<Uuid>,
    pub source_kind: String,
    pub external_id: String,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: Option<i64>,
    pub organizer: String,
    pub participants: Value,
    pub url: Option<String>,
    pub transcript: Value,
    pub created_by: Uuid,
}

/// Stores `meeting` unless its account already read the same one, and returns it when it
/// is new.
///
/// # Errors
/// Propagates any database error.
pub async fn insert_new(
    connection: &mut AsyncPgConnection,
    meeting: NewMeeting,
) -> QueryResult<Option<Meeting>> {
    diesel::insert_into(meetings::table)
        .values(meeting)
        .on_conflict((meetings::connector_account_id, meetings::external_id))
        .do_nothing()
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// The external ids among `external_ids` that the account already read, so a listing can
/// skip them without fetching each one's transcript.
///
/// # Errors
/// Propagates any database error.
pub async fn known_external_ids(
    connection: &mut AsyncPgConnection,
    connector_account_id: Uuid,
    external_ids: &[String],
) -> QueryResult<Vec<String>> {
    meetings::table
        .filter(meetings::connector_account_id.eq(connector_account_id))
        .filter(meetings::external_id.eq_any(external_ids))
        .select(meetings::external_id)
        .load(connection)
        .await
}

/// Every meeting still to be processed, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_unfinished(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Meeting>> {
    meetings::table
        .filter(
            meetings::state.ne_all([MeetingState::Done.as_str(), MeetingState::Failed.as_str()]),
        )
        .select(Meeting::as_select())
        .order(meetings::created_at.asc())
        .load(connection)
        .await
}

/// `owner`'s meetings, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_owned(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
    limit: i64,
) -> QueryResult<Vec<Meeting>> {
    meetings::table
        .filter(meetings::created_by.eq(owner))
        .select(Meeting::as_select())
        .order((meetings::started_at.desc(), meetings::id.desc()))
        .limit(limit)
        .load(connection)
        .await
}

/// The meeting `id`, when `owner` owns it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or belongs to someone else.
pub async fn find_owned(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<Meeting> {
    meetings::table
        .filter(meetings::id.eq(id))
        .filter(meetings::created_by.eq(owner))
        .select(Meeting::as_select())
        .first(connection)
        .await
}

/// Records the meeting's transcript and where its recording is stored, or that there is
/// none, and moves the meeting to `recorded`.
///
/// # Errors
/// Propagates any database error.
pub async fn record_recording(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    transcript: Value,
    stored: Option<StoredRecording>,
) -> QueryResult<Meeting> {
    let (storage_location_id, recording_path, recording_content_type) = match stored {
        Some(stored) => (
            Some(stored.storage_location_id),
            Some(stored.path),
            stored.content_type,
        ),
        None => (None, None, None),
    };
    diesel::update(meetings::table.find(id))
        .set((
            meetings::transcript.eq(transcript),
            meetings::storage_location_id.eq(storage_location_id),
            meetings::recording_path.eq(recording_path),
            meetings::recording_content_type.eq(recording_content_type),
            meetings::state.eq(MeetingState::Recorded.as_str()),
            meetings::attempts.eq(0),
            meetings::last_error.eq(None::<String>),
        ))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

/// Where a meeting's recording was stored.
#[derive(Debug, Clone)]
pub struct StoredRecording {
    pub storage_location_id: Uuid,
    pub path: String,
    pub content_type: Option<String>,
}

/// Records the segments Elysia found and moves the meeting to `segmented`.
///
/// # Errors
/// Propagates any database error.
pub async fn record_segments(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    segments: Value,
) -> QueryResult<Meeting> {
    diesel::update(meetings::table.find(id))
        .set((
            meetings::segments.eq(Some(segments)),
            meetings::state.eq(MeetingState::Segmented.as_str()),
            meetings::attempts.eq(0),
            meetings::last_error.eq(None::<String>),
        ))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

/// Moves the meeting to `done`: its segments are on the timeline.
///
/// # Errors
/// Propagates any database error.
pub async fn record_done(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Meeting> {
    diesel::update(meetings::table.find(id))
        .set((
            meetings::state.eq(MeetingState::Done.as_str()),
            meetings::attempts.eq(0),
            meetings::last_error.eq(None::<String>),
        ))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

/// Records that the meeting's current step failed with `error`, and gives the meeting up
/// once that has happened [`MAX_ATTEMPTS`] times.
///
/// # Errors
/// Propagates any database error.
pub async fn record_failure(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    error: &str,
) -> QueryResult<Meeting> {
    let attempts = meeting.attempts.saturating_add(1);
    let state = if attempts >= MAX_ATTEMPTS {
        MeetingState::Failed
    } else {
        meeting.state()
    };
    diesel::update(meetings::table.find(meeting.id))
        .set((
            meetings::attempts.eq(attempts),
            meetings::last_error.eq(Some(error)),
            meetings::state.eq(state.as_str()),
        ))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

/// Gives a failed meeting another try from the step it failed at: a meeting with segments
/// resumes trimming, one with a recording resumes segmenting, and any other starts over.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist, belongs to someone else, or
/// has not failed.
pub async fn retry(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<Meeting> {
    let meeting = find_owned(connection, id, owner).await?;
    if meeting.state() != MeetingState::Failed {
        return Err(diesel::result::Error::NotFound);
    }
    let resume_at = if meeting.segments.is_some() {
        MeetingState::Segmented
    } else if meeting.recording_path.is_some() {
        MeetingState::Recorded
    } else {
        MeetingState::Pending
    };
    diesel::update(meetings::table.find(id))
        .set((
            meetings::state.eq(resume_at.as_str()),
            meetings::attempts.eq(0),
            meetings::last_error.eq(None::<String>),
        ))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

#[cfg(test)]
mod tests {
    use secrecy::{ExposeSecret, SecretString};
    use serde_json::json;

    use super::*;
    use crate::models::connector_account::{self, NewConnectorAccount};
    use crate::models::storage_location;
    use crate::test_support::{
        TEST_PERSON_ID, cipher, location_for_every_project, migrated_database,
    };

    fn new_meeting(account_id: Uuid, external_id: &str) -> NewMeeting {
        NewMeeting {
            id: Uuid::now_v7(),
            connector_account_id: Some(account_id),
            source_kind: "fireflies".to_owned(),
            external_id: external_id.to_owned(),
            title: "Weekly sync".to_owned(),
            started_at: Utc::now(),
            duration_ms: Some(1_800_000),
            organizer: "ada@example.com".to_owned(),
            participants: json!(["ada@example.com"]),
            url: None,
            transcript: json!([]),
            created_by: TEST_PERSON_ID,
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_is_stored_once_and_moves_through_its_steps() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let location = storage_location::create(
            &mut connection,
            &cipher,
            &location_for_every_project("Recordings"),
        )
        .await
        .expect("a location");
        let account = connector_account::create(
            &mut connection,
            NewConnectorAccount::with_credential(
                &cipher,
                "fireflies",
                &SecretString::from("fireflies-key"),
                Some(location.id),
                TEST_PERSON_ID,
            ),
        )
        .await
        .expect("an account");
        let credential = account
            .credential(&cipher)
            .expect("opens")
            .expect("a credential");
        assert_eq!(credential.expose_secret(), "fireflies-key");
        assert_eq!(account.browser_session_id, None);

        let stored = insert_new(&mut connection, new_meeting(account.id, "01HX"))
            .await
            .expect("insert")
            .expect("new");
        let again = insert_new(&mut connection, new_meeting(account.id, "01HX"))
            .await
            .expect("insert");
        assert!(
            again.is_none(),
            "the same meeting is stored once per account"
        );
        assert_eq!(
            known_external_ids(
                &mut connection,
                account.id,
                &["01HX".to_owned(), "other".to_owned()]
            )
            .await
            .expect("known"),
            vec!["01HX".to_owned()]
        );

        // Three failures give it up; a retry starts it over.
        let mut failing = stored;
        for _ in 0..MAX_ATTEMPTS {
            failing = record_failure(&mut connection, &failing, "Fireflies did not answer")
                .await
                .expect("a failure");
        }
        assert_eq!(failing.state(), MeetingState::Failed);
        assert!(
            list_unfinished(&mut connection)
                .await
                .expect("list")
                .is_empty()
        );
        let retried = retry(&mut connection, failing.id, TEST_PERSON_ID)
            .await
            .expect("retried");
        assert_eq!(retried.state(), MeetingState::Pending);
        assert_eq!((retried.attempts, retried.last_error), (0, None));

        let recorded = record_recording(
            &mut connection,
            retried.id,
            json!([{ "startMs": 0, "endMs": 1000, "speaker": "Ada", "text": "Hi." }]),
            Some(StoredRecording {
                storage_location_id: location.id,
                path: "meetings/x/recording.mp4".to_owned(),
                content_type: Some("video/mp4".to_owned()),
            }),
        )
        .await
        .expect("recorded");
        assert_eq!(recorded.state(), MeetingState::Recorded);
        assert_eq!(recorded.storage_location_id, Some(location.id));

        let segmented = record_segments(&mut connection, recorded.id, json!([]))
            .await
            .expect("segmented");
        assert_eq!(segmented.state(), MeetingState::Segmented);
        let done = record_done(&mut connection, segmented.id)
            .await
            .expect("done");
        assert_eq!(done.state(), MeetingState::Done);
        assert!(
            list_unfinished(&mut connection)
                .await
                .expect("list")
                .is_empty()
        );
        assert!(
            retry(&mut connection, done.id, TEST_PERSON_ID)
                .await
                .is_err(),
            "only a failed meeting is retried"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_account_signs_in_exactly_one_way() {
        let (_url, mut connection) = migrated_database().await;
        let mut neither = NewConnectorAccount::with_credential(
            &cipher(),
            "fireflies",
            &SecretString::from("key"),
            None,
            TEST_PERSON_ID,
        );
        neither.credential_encrypted = None;
        assert!(
            connector_account::create(&mut connection, neither)
                .await
                .is_err(),
            "an account with no way to sign in is refused"
        );
    }

    #[test]
    fn every_state_is_stored_as_text_it_parses() {
        for state in MeetingState::ALL {
            assert_eq!(MeetingState::parse(state.as_str()), Some(state));
        }
        assert_eq!(MeetingState::parse("sideways"), None);
    }
}
