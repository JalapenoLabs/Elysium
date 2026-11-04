// Copyright © 2026 Jalapeno Labs

//! Meetings read from a meeting source, and how far each one's processing got. See
//! `crate::meetings` and `docs/meetings.md`.
//!
//! A meeting belongs to whoever owns the account it was read from, like every timeline
//! record. It moves through `pending` (listed), `recorded` (fetched by its run of the meeting
//! workflow), `segmented`, and `done`, and is `failed` once its run fails, or a step of the
//! processing loop has failed [`MAX_ATTEMPTS`] times.
//!
//! What a fetch stores, the source's transcript and the recording, is written once
//! ([`record_fetched`]): a later fetch of the same meeting writes nothing.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
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
    /// Listed by its source; its run of the meeting workflow fetches it.
    Pending,
    /// Its transcript is stored, and its recording too, unless the source keeps none or its
    /// connector names nowhere to keep it.
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
    pub connector_id: Option<Uuid>,
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
    /// The SHA-256 of the transcript its fetch stored (`crate::meetings::fetch`), as hex.
    /// `None` until it is fetched.
    pub transcript_hash: Option<String>,
}

impl Meeting {
    /// The meeting's state. A value the constraint does not allow reads as failed.
    pub fn state(&self) -> MeetingState {
        MeetingState::parse(&self.state).unwrap_or(MeetingState::Failed)
    }

    /// Where its recording is stored, if it is.
    pub fn stored_recording(&self) -> Option<StoredRecording> {
        Some(StoredRecording {
            storage_location_id: self.storage_location_id?,
            path: self.recording_path.clone()?,
            content_type: self.recording_content_type.clone(),
        })
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meetings)]
pub struct NewMeeting {
    pub id: Uuid,
    pub connector_id: Option<Uuid>,
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
        .on_conflict((meetings::connector_id, meetings::external_id))
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
    connector_id: Uuid,
    external_ids: &[String],
) -> QueryResult<Vec<String>> {
    meetings::table
        .filter(meetings::connector_id.eq(connector_id))
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

/// The meeting `id`, whoever owns it, for the engine's own work on it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Meeting> {
    meetings::table
        .find(id)
        .select(Meeting::as_select())
        .first(connection)
        .await
}

/// What a meeting's first fetch stores.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// An array of `crate::meetings::source::TranscriptLine`.
    pub transcript: Value,
    /// The SHA-256 of `transcript`, as hex.
    pub transcript_hash: String,
    /// Where its recording was put, or `None` when it has none to keep.
    pub recording: Option<StoredRecording>,
}

/// Stores the meeting's first fetch, its transcript and where its recording is kept, and moves
/// it to `recorded`. Answers `None`, writing nothing, when the meeting was fetched already: the
/// transcript a meeting was first fetched with is the one every later step reads, so nothing a
/// later fetch finds replaces it.
///
/// # Errors
/// Propagates any database error.
pub async fn record_fetched(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    fetched: Fetched,
) -> QueryResult<Option<Meeting>> {
    let (storage_location_id, recording_path, recording_content_type) = match fetched.recording {
        Some(stored) => (
            Some(stored.storage_location_id),
            Some(stored.path),
            stored.content_type,
        ),
        None => (None, None, None),
    };
    diesel::update(meetings::table.find(id))
        .filter(meetings::transcript_hash.is_null())
        .set((
            meetings::transcript.eq(fetched.transcript),
            meetings::transcript_hash.eq(Some(fetched.transcript_hash)),
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
        .optional()
}

/// Where a meeting's recording was stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredRecording {
    pub storage_location_id: Uuid,
    pub path: String,
    pub content_type: Option<String>,
}

/// Marks a meeting still waiting to be fetched as `failed`, with `error`, because its run of the
/// meeting workflow failed. Answers `None`, writing nothing, for a meeting that is no longer
/// waiting to be fetched.
///
/// # Errors
/// Propagates any database error.
pub async fn record_fetch_failure(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    error: &str,
) -> QueryResult<Option<Meeting>> {
    diesel::update(meetings::table.find(id))
        .filter(meetings::state.eq(MeetingState::Pending.as_str()))
        .set((
            meetings::state.eq(MeetingState::Failed.as_str()),
            meetings::last_error.eq(Some(error)),
        ))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
        .optional()
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
/// resumes trimming, one that was fetched resumes segmenting, and any other waits to be fetched
/// again. A meeting whose run of the meeting workflow failed also needs that run's failed step
/// retried, which `crate::meetings::workflow::retry` does alongside this.
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
    // A meeting fetched before transcripts were hashed shows it by its recording alone.
    let resume_at = if meeting.segments.is_some() {
        MeetingState::Segmented
    } else if meeting.transcript_hash.is_some() || meeting.recording_path.is_some() {
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
    use crate::connectors::ConnectorKind;
    use crate::models::connector::{self, AccountRef, NewConnector};
    use crate::models::storage_location;
    use crate::models::token_account::{self, NewTokenAccount};
    use crate::test_support::{
        TEST_PERSON_ID, cipher, location_for_every_project, migrated_database,
    };

    /// A hash of the right shape; the model stores what it is given.
    const TRANSCRIPT_HASH: &str =
        "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945";

    /// A fetch with one line, its recording kept in `location_id` when one is given.
    fn fetched(location_id: Option<Uuid>) -> Fetched {
        Fetched {
            transcript: json!([{ "startMs": 0, "endMs": 1000, "speaker": "Ada", "text": "Hi." }]),
            transcript_hash: TRANSCRIPT_HASH.to_owned(),
            recording: location_id.map(|storage_location_id| StoredRecording {
                storage_location_id,
                path: "meetings/x/recording.mp4".to_owned(),
                content_type: Some("video/mp4".to_owned()),
            }),
        }
    }

    fn new_meeting(account_id: Uuid, external_id: &str) -> NewMeeting {
        NewMeeting {
            id: Uuid::now_v7(),
            connector_id: Some(account_id),
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

    /// A Fireflies key and the connector it feeds, keeping recordings in `location_id`.
    async fn fireflies_connector(
        connection: &mut AsyncPgConnection,
        cipher: &crate::crypto::Cipher,
        location_id: Uuid,
        key: &str,
    ) -> (token_account::TokenAccount, connector::Connector) {
        let key = token_account::create(
            connection,
            NewTokenAccount::new(
                cipher,
                "fireflies",
                "Fireflies",
                &SecretString::from(key),
                "ada@example.com",
                TEST_PERSON_ID,
            ),
        )
        .await
        .expect("a key");
        let mut new_connector = NewConnector::new(
            ConnectorKind::Fireflies.as_str(),
            AccountRef::Token(key.id),
            TEST_PERSON_ID,
        );
        new_connector.storage_location_id = Some(location_id);
        let created = connector::create(connection, new_connector)
            .await
            .expect("a connector");
        (key, created)
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
        let (key, account) =
            fireflies_connector(&mut connection, &cipher, location.id, "fireflies-key").await;
        let credential = key.credential(&cipher).expect("opens");
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

        let recorded = record_fetched(&mut connection, retried.id, fetched(Some(location.id)))
            .await
            .expect("recorded")
            .expect("the first fetch is stored");
        assert_eq!(recorded.state(), MeetingState::Recorded);
        assert_eq!(recorded.storage_location_id, Some(location.id));
        assert_eq!(
            recorded.transcript_hash.as_deref(),
            Some(TRANSCRIPT_HASH),
            "the hash is stored with the transcript"
        );

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
    async fn a_meeting_is_fetched_once_and_a_later_fetch_writes_nothing() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let location = storage_location::create(
            &mut connection,
            &cipher,
            &location_for_every_project("Recordings"),
        )
        .await
        .expect("a location");
        let (_key, account) =
            fireflies_connector(&mut connection, &cipher, location.id, "fireflies-key").await;
        let pending = insert_new(&mut connection, new_meeting(account.id, "01HX"))
            .await
            .expect("insert")
            .expect("new");

        let first = record_fetched(&mut connection, pending.id, fetched(None))
            .await
            .expect("recorded")
            .expect("the first fetch is stored");
        assert_eq!(first.state(), MeetingState::Recorded);
        assert_eq!(first.stored_recording(), None);

        // Another fetch, even one that found a recording, leaves the meeting exactly as it was.
        let again = record_fetched(&mut connection, pending.id, fetched(Some(location.id)))
            .await
            .expect("a second write runs");
        assert!(again.is_none(), "a fetched meeting is not written again");
        let unchanged = find(&mut connection, pending.id).await.expect("found");
        assert_eq!(unchanged.updated_at, first.updated_at);
        assert_eq!(unchanged.storage_location_id, None);

        // Only a meeting still waiting to be fetched takes its run's failure.
        assert!(
            record_fetch_failure(&mut connection, pending.id, "Fireflies refused")
                .await
                .expect("runs")
                .is_none()
        );
        let waiting = insert_new(&mut connection, new_meeting(account.id, "01HY"))
            .await
            .expect("insert")
            .expect("new");
        let failed = record_fetch_failure(&mut connection, waiting.id, "Fireflies refused")
            .await
            .expect("runs")
            .expect("a waiting meeting fails");
        assert_eq!(failed.state(), MeetingState::Failed);
        assert_eq!(failed.last_error.as_deref(), Some("Fireflies refused"));
        let retried = retry(&mut connection, failed.id, TEST_PERSON_ID)
            .await
            .expect("retried");
        assert_eq!(
            retried.state(),
            MeetingState::Pending,
            "a meeting never fetched waits to be fetched again"
        );

        // A fetched meeting that failed later resumes after its fetch, even with no recording.
        let mut failed_later = first;
        for _ in 0..MAX_ATTEMPTS {
            failed_later = record_failure(&mut connection, &failed_later, "no answer")
                .await
                .expect("a failure");
        }
        assert_eq!(failed_later.state(), MeetingState::Failed);
        let resumed = retry(&mut connection, failed_later.id, TEST_PERSON_ID)
            .await
            .expect("retried");
        assert_eq!(resumed.state(), MeetingState::Recorded);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_connector_reads_through_exactly_one_account() {
        let (_url, mut connection) = migrated_database().await;
        let key = token_account::create(
            &mut connection,
            NewTokenAccount::new(
                &cipher(),
                "fireflies",
                "Fireflies",
                &SecretString::from("key"),
                "",
                TEST_PERSON_ID,
            ),
        )
        .await
        .expect("a key");

        let mut neither = NewConnector::new("fireflies", AccountRef::Token(key.id), TEST_PERSON_ID);
        neither.token_account_id = None;
        assert!(
            connector::create(&mut connection, neither).await.is_err(),
            "a connector with no account is refused"
        );

        connector::create(
            &mut connection,
            NewConnector::new("fireflies", AccountRef::Token(key.id), TEST_PERSON_ID),
        )
        .await
        .expect("the first Fireflies connector");
        assert!(
            connector::create(
                &mut connection,
                NewConnector::new("fireflies", AccountRef::Token(key.id), TEST_PERSON_ID),
            )
            .await
            .is_err(),
            "no two connectors share an account"
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
