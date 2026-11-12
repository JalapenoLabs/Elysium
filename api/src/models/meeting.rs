// Copyright © 2026 Jalapeno Labs

//! Meetings: each the one event that happened, with how far its processing got. See
//! `crate::meetings` and `docs/meetings.md`.
//!
//! A meeting is the event; its captures (`models::meeting_capture`) are the records of it, each
//! with what its source gave. The meeting keeps the event's own fields (its title, when it started
//! and for how long, who organized it and took part) and what Elysium made of it: its segments,
//! its final transcript (`models::meeting_word`), and its history (`models::meeting_event`).
//!
//! A meeting belongs to whoever owns the account its first capture was read from, or who uploaded
//! it. It moves through `pending` (listed), `recorded` (its capture fetched by its run of the
//! meeting workflow), `segmented`, and `done`, each written by a step of that run. How far it got
//! is all it keeps of its processing: whether it failed, and why, is its run's
//! (`crate::meetings::workflow`). Its segments are written once ([`record_segments`]).
//!
//! # Who reads it
//!
//! A meeting is [`Visibility::Public`], read by every member; [`Visibility::Private`], read by its
//! owner and the people it is shared with (`models::meeting_share`); or new and
//! [`Visibility::Unchecked`], private to its owner until its correction decides. Admins read every
//! meeting. [`readable_by`] is that rule, the one place it is written: finding a meeting
//! ([`find_readable`]), listing them ([`list_readable`]), and leaving the meetings someone cannot
//! read out of the knowledge index ([`hidden_from`]) all ask it. What a reader may change is
//! `crate::meetings::access`'s. See `docs/meeting-review.md`, Who sees a meeting.
//!
//! A meeting whose captures its person merged into another one names it (`merged_into`). It
//! holds no captures while it does, and is listed nowhere and processed no further; splitting
//! the captures back out clears it (`crate::meetings::duplicates`).

use chrono::{DateTime, Utc};
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::sql_types::Bool;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::{meeting_corrections, meeting_shares, meetings};
use crate::models::meeting_capture::{self, MeetingCapture, NewMeetingCapture};
use crate::models::user::User;

/// How far a meeting's processing got. Stored as `meetings.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingState {
    /// Listed by its source; its run of the meeting workflow fetches it.
    Pending,
    /// Its capture's transcript is stored, and its recording too, unless the source keeps none or
    /// its connector names nowhere to keep it.
    Recorded,
    /// Elysia has cut its transcript into topic segments.
    Segmented,
    /// Its segments are on the timeline.
    Done,
}

impl MeetingState {
    const ALL: [Self; 4] = [Self::Pending, Self::Recorded, Self::Segmented, Self::Done];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Recorded => "recorded",
            Self::Segmented => "segmented",
            Self::Done => "done",
        }
    }

    /// The state stored as `text`, if it is one.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == text)
    }
}

/// Who reads a meeting besides its owner, the people it is shared with, and admins. Stored as
/// `meetings.visibility`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// New, and private to its owner until its correction reads it and decides
    /// (`crate::meetings::correct`). Read exactly as [`Self::Private`] is.
    Unchecked,
    /// Read by its owner and the people it is shared with alone.
    Private,
    /// Read by every member.
    Public,
}

impl Visibility {
    const ALL: [Self; 3] = [Self::Unchecked, Self::Private, Self::Public];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unchecked => "unchecked",
            Self::Private => "private",
            Self::Public => "public",
        }
    }

    /// The visibility stored as `text`, if it is one.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|visibility| visibility.as_str() == text)
    }
}

/// Someone reading meetings: who they are, and how far their reading reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reader {
    pub id: Uuid,
    pub reach: Reach,
}

/// How far a reader's reading of meetings reaches, by their role (`docs/auth.md`, Roles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// An admin, who may do everything, reads every meeting, which is how one shares or opens a
    /// private meeting nobody shared with them.
    Every,
    /// A member, or one of Elysium's machines reading the knowledge base, reads every public
    /// meeting besides its own and those shared with it.
    Public,
    /// A guest, and anyone without a role, reads only its own and those shared with it.
    Shared,
}

impl Reader {
    /// `user` as a reader of meetings. Anyone who is neither a company member nor a machine
    /// reaches only what is theirs or shared with them, so a role added later gains nothing by
    /// accident.
    pub fn of(user: &User) -> Self {
        let reach = if user.is_admin() {
            Reach::Every
        } else if user.is_company_member() || user.is_machine() {
            Reach::Public
        } else {
            Reach::Shared
        };
        Self { id: user.id, reach }
    }
}

/// Whether a meeting may be read by `reader`: the one statement of who reads a meeting, which
/// every query reading meetings for someone filters by.
fn readable_by(reader: Reader) -> Box<dyn BoxableExpression<meetings::table, Pg, SqlType = Bool>> {
    let shared_with = meeting_shares::table
        .filter(meeting_shares::person_id.eq(reader.id))
        .select(meeting_shares::meeting_id);
    let own_or_shared = meetings::created_by
        .eq(reader.id)
        .or(meetings::id.eq_any(shared_with));
    match reader.reach {
        Reach::Every => Box::new(true.into_sql::<Bool>()),
        Reach::Public => {
            Box::new(own_or_shared.or(meetings::visibility.eq(Visibility::Public.as_str())))
        }
        Reach::Shared => Box::new(own_or_shared),
    }
}

/// Where a page of meetings starts: after the meeting that started at `started_at` with this
/// `id`, the last of the page before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub started_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = meetings, check_for_backend(diesel::pg::Pg))]
pub struct Meeting {
    pub id: Uuid,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: Option<i64>,
    pub organizer: String,
    /// An array of strings.
    pub participants: Value,
    /// The segments Elysia found; see `crate::meetings::segmentation::Segment`.
    pub segments: Option<Value>,
    /// See [`MeetingState`].
    pub state: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Whether its person asked to approve its corrected transcript before it is acted on,
    /// whatever its source's setting (`crate::meetings::correct`).
    pub hold_for_approval: bool,
    /// The meeting its captures were merged into, while they are.
    pub merged_into: Option<Uuid>,
    /// See [`Visibility`].
    pub visibility: String,
    /// The inbox item telling its owner that its correction kept it private, and why.
    pub visibility_item_id: Option<Uuid>,
}

impl Meeting {
    /// The meeting's state.
    ///
    /// # Panics
    /// Panics on a state `meetings_state_check` does not allow, which only a migration that
    /// drifted from [`MeetingState`] can cause.
    pub fn state(&self) -> MeetingState {
        MeetingState::parse(&self.state).expect("meetings_state_check allows only MeetingState")
    }

    /// Who reads the meeting.
    ///
    /// # Panics
    /// Panics on a visibility `meetings_visibility_check` does not allow, which only a migration
    /// that drifted from [`Visibility`] can cause.
    pub fn visibility(&self) -> Visibility {
        Visibility::parse(&self.visibility)
            .expect("meetings_visibility_check allows only Visibility")
    }

    /// The meeting's title where everyone in the workspace reads it, such as an inbox item or the
    /// Transcription doc: its own while it is public, and `None` while it is private, since the
    /// title of a one-to-one or a review can be what makes it private.
    pub fn public_title(&self) -> Option<&str> {
        let title = self.title.trim();
        (self.visibility() == Visibility::Public && !title.is_empty()).then_some(title)
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meetings)]
pub struct NewMeeting {
    pub id: Uuid,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: Option<i64>,
    pub organizer: String,
    pub participants: Value,
    pub created_by: Uuid,
}

/// Stores `meeting` with its first capture, `capture`, unless the capture's connector already
/// recorded the same one, and answers both when they are new. A capture its connector recorded
/// before leaves no meeting behind: the two are written together or not at all.
///
/// # Errors
/// Propagates any database error.
pub async fn insert_new(
    connection: &mut AsyncPgConnection,
    meeting: NewMeeting,
    capture: NewMeetingCapture,
) -> QueryResult<Option<(Meeting, MeetingCapture)>> {
    // A savepoint when the caller's transaction is already open.
    let stored = connection
        .transaction(async |connection| {
            let stored: Meeting = diesel::insert_into(meetings::table)
                .values(meeting)
                .returning(Meeting::as_returning())
                .get_result(connection)
                .await?;
            match meeting_capture::insert_new(connection, capture).await? {
                Some(captured) => Ok((stored, captured)),
                // Recorded before: the meeting just stored goes back with the transaction.
                None => Err(diesel::result::Error::RollbackTransaction),
            }
        })
        .await;
    match stored {
        Ok(stored) => Ok(Some(stored)),
        Err(diesel::result::Error::RollbackTransaction) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Every meeting whose segments are not on the timeline yet, oldest first. The condition is
/// `meetings_unfinished`'s own, so the partial index answers it.
///
/// # Errors
/// Propagates any database error.
pub async fn list_unfinished(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Meeting>> {
    meetings::table
        .filter(meetings::state.ne(MeetingState::Done.as_str()))
        // A meeting merged into another has nothing of its own left to process.
        .filter(meetings::merged_into.is_null())
        .select(Meeting::as_select())
        .order(meetings::created_at.asc())
        .load(connection)
        .await
}

/// Up to `limit` of the meetings `reader` may read, newest first, starting after `after` when
/// given, the later id first between two that started together, so two never tie and a page never
/// skips one. Those merged into another, which live on in it, are left out.
///
/// # Errors
/// Propagates any database error.
pub async fn list_readable(
    connection: &mut AsyncPgConnection,
    reader: Reader,
    after: Option<Cursor>,
    limit: i64,
) -> QueryResult<Vec<Meeting>> {
    let mut query = meetings::table
        .filter(readable_by(reader))
        .filter(meetings::merged_into.is_null())
        .select(Meeting::as_select())
        .order((meetings::started_at.desc(), meetings::id.desc()))
        .limit(limit)
        .into_boxed();
    if let Some(after) = after {
        query = query.filter(
            meetings::started_at
                .lt(after.started_at)
                .or(meetings::started_at
                    .eq(after.started_at)
                    .and(meetings::id.lt(after.id))),
        );
    }
    query.load(connection).await
}

/// The meeting `id`, when `reader` may read it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or `reader` may not read it, which
/// the routes answer alike, so a private meeting's existence is no answer either.
pub async fn find_readable(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    reader: Reader,
) -> QueryResult<Meeting> {
    meetings::table
        .filter(meetings::id.eq(id))
        .filter(readable_by(reader))
        .select(Meeting::as_select())
        .first(connection)
        .await
}

/// The ids of every meeting `reader` may not read: what the knowledge index leaves out of their
/// searches, the documents of each one's segments (`docs/knowledge.md`, Who sees what). Few, since
/// only private meetings nobody shared with them are.
///
/// # Errors
/// Propagates any database error.
pub async fn hidden_from(
    connection: &mut AsyncPgConnection,
    reader: Reader,
) -> QueryResult<Vec<Uuid>> {
    if reader.reach == Reach::Every {
        return Ok(Vec::new());
    }
    meetings::table
        .filter(diesel::dsl::not(readable_by(reader)))
        .select(meetings::id)
        .load(connection)
        .await
}

/// Sets who reads meeting `id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist.
pub async fn set_visibility(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    visibility: Visibility,
) -> QueryResult<Meeting> {
    diesel::update(meetings::table.find(id))
        .set(meetings::visibility.eq(visibility.as_str()))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

/// Records `item_id` as the inbox item telling meeting `id`'s owner what its correction decided.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist.
pub async fn set_visibility_item(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    item_id: Uuid,
) -> QueryResult<Meeting> {
    diesel::update(meetings::table.find(id))
        .set(meetings::visibility_item_id.eq(item_id))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

/// The meeting whose correction raised inbox item `item_id`, asking for its review, when `reader`
/// may read it.
///
/// # Errors
/// Propagates any database error.
pub async fn reviewed_by(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
    reader: Reader,
) -> QueryResult<Vec<Meeting>> {
    let reviewed = meeting_corrections::table
        .filter(meeting_corrections::review_item_id.eq(item_id))
        .select(meeting_corrections::meeting_id);
    meetings::table
        .filter(meetings::id.eq_any(reviewed))
        .filter(readable_by(reader))
        .select(Meeting::as_select())
        .load(connection)
        .await
}

/// The meeting inbox item `item_id` told its owner was kept private, when `reader` may read it.
///
/// # Errors
/// Propagates any database error.
pub async fn kept_private_by(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
    reader: Reader,
) -> QueryResult<Vec<Meeting>> {
    meetings::table
        .filter(meetings::visibility_item_id.eq(item_id))
        .filter(readable_by(reader))
        .select(Meeting::as_select())
        .load(connection)
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

/// The meeting `id`, locked until the transaction ends, so what changes it is decided one writer
/// at a time: the correction step deciding whether to hold its run, its person holding it by hand
/// or approving it, and every change to its final transcript.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist.
pub async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Meeting> {
    meetings::table
        .find(id)
        .for_update()
        .select(Meeting::as_select())
        .first(connection)
        .await
}

/// Sets whether meeting `id` waits for its person's approval of its corrected transcript.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist.
pub async fn set_hold_for_approval(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    hold: bool,
) -> QueryResult<Meeting> {
    diesel::update(meetings::table.find(id))
        .set(meetings::hold_for_approval.eq(hold))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

/// Records meeting `id`'s captures as merged into meeting `into`, or, given `None`, as split back
/// out into it again.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist.
pub async fn set_merged_into(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    into: Option<Uuid>,
) -> QueryResult<Meeting> {
    diesel::update(meetings::table.find(id))
        .set(meetings::merged_into.eq(into))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

/// `owner`'s meetings other than `except`, not merged into another, that started between `from`
/// and `to`, oldest first: where a meeting looks for one it may be the same as.
///
/// # Errors
/// Propagates any database error.
pub async fn started_between(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
    except: Uuid,
    (from, to): (DateTime<Utc>, DateTime<Utc>),
) -> QueryResult<Vec<Meeting>> {
    meetings::table
        .filter(meetings::created_by.eq(owner))
        .filter(meetings::id.ne(except))
        .filter(meetings::merged_into.is_null())
        .filter(meetings::started_at.between(from, to))
        .select(Meeting::as_select())
        .order((meetings::started_at.asc(), meetings::id.asc()))
        .load(connection)
        .await
}

/// Moves pending meeting `id` to `recorded`, once its capture's fetch is stored. A meeting past
/// `pending` already is answered as it is.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist.
pub async fn record_recorded(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Meeting> {
    let moved = diesel::update(meetings::table.find(id))
        .filter(meetings::state.eq(MeetingState::Pending.as_str()))
        .set(meetings::state.eq(MeetingState::Recorded.as_str()))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
        .optional()?;
    match moved {
        Some(moved) => Ok(moved),
        None => find(connection, id).await,
    }
}

/// Stores the segments Elysia found for fetched meeting `id` and moves it to `segmented`.
/// Answers `None`, writing nothing, when it has segments already: the first answer stored is the
/// one its clips and records are made from, so an attempt that runs again never asks twice.
///
/// # Errors
/// Propagates any database error.
pub async fn record_segments(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    segments: Value,
) -> QueryResult<Option<Meeting>> {
    diesel::update(meetings::table.find(id))
        .filter(meetings::segments.is_null())
        .set((
            meetings::segments.eq(Some(segments)),
            meetings::state.eq(MeetingState::Segmented.as_str()),
        ))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Moves meeting `id` to `done`: its segments are on the timeline.
///
/// # Errors
/// Propagates any database error.
pub async fn record_done(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Meeting> {
    diesel::update(meetings::table.find(id))
        .set(meetings::state.eq(MeetingState::Done.as_str()))
        .returning(Meeting::as_returning())
        .get_result(connection)
        .await
}

#[cfg(test)]
pub(crate) mod tests {
    use secrecy::{ExposeSecret, SecretString};
    use serde_json::json;

    use super::*;
    use crate::connectors::ConnectorKind;
    use crate::models::connector::{self, AccountRef, NewConnector};
    use crate::models::meeting_capture::{Fetched, KeptRecording, StoredRecording};
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
            recording: location_id.map(|storage_location_id| KeptRecording {
                stored: StoredRecording {
                    storage_location_id,
                    path: "meetings/x/recording.mp4".to_owned(),
                    content_type: Some("video/mp4".to_owned()),
                },
                hash: "c".repeat(64),
            }),
        }
    }

    /// A meeting of the test person's, as a source listed it, and its first capture through
    /// `connector_id` under `external_id`.
    pub(crate) fn listed(
        connector_id: Option<Uuid>,
        external_id: &str,
    ) -> (NewMeeting, NewMeetingCapture) {
        let meeting_id = Uuid::now_v7();
        (
            NewMeeting {
                id: meeting_id,
                title: "Weekly sync".to_owned(),
                started_at: Utc::now(),
                duration_ms: Some(1_800_000),
                organizer: "ada@example.com".to_owned(),
                participants: json!(["ada@example.com"]),
                created_by: TEST_PERSON_ID,
            },
            NewMeetingCapture {
                id: Uuid::now_v7(),
                meeting_id,
                connector_id,
                source_kind: "fireflies".to_owned(),
                external_id: Some(external_id.to_owned()),
                url: None,
                created_by: TEST_PERSON_ID,
            },
        )
    }

    /// Stores a listed meeting with no connector, as a test of anything after the listing needs.
    pub(crate) async fn stored(connection: &mut AsyncPgConnection) -> (Meeting, MeetingCapture) {
        let (meeting, capture) = listed(None, &Uuid::now_v7().to_string());
        insert_new(connection, meeting, capture)
            .await
            .expect("insert")
            .expect("new")
    }

    /// Stores a listed meeting with no connector whose capture was fetched with `transcript`, an
    /// array of transcript lines, under the transcript hash `"a"` sixty-four times, and no
    /// recording; the meeting is `recorded`.
    pub(crate) async fn fetched_with(
        connection: &mut AsyncPgConnection,
        transcript: Value,
    ) -> (Meeting, MeetingCapture) {
        let (meeting, capture) = stored(connection).await;
        let fetched = Fetched {
            transcript,
            transcript_hash: "a".repeat(64),
            recording: None,
        };
        let capture = meeting_capture::record_fetched(connection, capture.id, fetched)
            .await
            .expect("records")
            .expect("the first fetch");
        let meeting = record_recorded(connection, meeting.id)
            .await
            .expect("moves");
        (meeting, capture)
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
    #[expect(
        clippy::too_many_lines,
        reason = "one meeting listed twice, fetched, segmented, and done, with what each left"
    )]
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

        let (meeting, capture) = listed(Some(account.id), "01HX");
        let (stored, captured) = insert_new(&mut connection, meeting, capture)
            .await
            .expect("insert")
            .expect("new");
        assert_eq!(captured.meeting_id, stored.id);
        assert_eq!(
            captured.offset_ms, 0,
            "a first capture runs on the meeting's clock"
        );

        let (again, again_capture) = listed(Some(account.id), "01HX");
        let again_id = again.id;
        let second = insert_new(&mut connection, again, again_capture)
            .await
            .expect("insert");
        assert!(
            second.is_none(),
            "the same meeting is stored once per connector"
        );
        assert!(
            matches!(
                find(&mut connection, again_id).await,
                Err(diesel::result::Error::NotFound)
            ),
            "and the meeting stored for it is gone with its capture"
        );
        assert_eq!(
            meeting_capture::known_external_ids(
                &mut connection,
                account.id,
                &["01HX".to_owned(), "other".to_owned()]
            )
            .await
            .expect("known"),
            vec!["01HX".to_owned()]
        );

        let recorded = meeting_capture::record_fetched(
            &mut connection,
            captured.id,
            fetched(Some(location.id)),
        )
        .await
        .expect("recorded")
        .expect("the first fetch is stored");
        assert_eq!(recorded.storage_location_id, Some(location.id));
        assert_eq!(recorded.recording_hash, Some("c".repeat(64)));
        assert_eq!(
            recorded.transcript_hash.as_deref(),
            Some(TRANSCRIPT_HASH),
            "the hash is stored with the transcript"
        );
        let moved = record_recorded(&mut connection, stored.id)
            .await
            .expect("moves");
        assert_eq!(moved.state(), MeetingState::Recorded);

        let first_answer = json!([{ "startMs": 0, "endMs": 60_000 }]);
        let segmented = record_segments(&mut connection, stored.id, first_answer.clone())
            .await
            .expect("segmented")
            .expect("the first answer is stored");
        assert_eq!(segmented.state(), MeetingState::Segmented);
        // An attempt that runs again keeps the first answer its clips are cut from.
        let second = record_segments(&mut connection, stored.id, json!([]))
            .await
            .expect("a second write runs");
        assert!(second.is_none(), "segments are written once");
        assert_eq!(
            find(&mut connection, stored.id)
                .await
                .expect("found")
                .segments,
            Some(first_answer)
        );
        // A meeting past pending stays where it got.
        assert_eq!(
            record_recorded(&mut connection, stored.id)
                .await
                .expect("answers")
                .state(),
            MeetingState::Segmented
        );

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
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn two_listings_of_one_meeting_at_once_store_it_once() {
        let (url, mut connection) = migrated_database().await;
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
        let mut other = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("a second connection");

        let (first_meeting, first_capture) = listed(Some(account.id), "01HX");
        let (second_meeting, second_capture) = listed(Some(account.id), "01HX");
        let (first, second) = tokio::join!(
            insert_new(&mut connection, first_meeting, first_capture),
            insert_new(&mut other, second_meeting, second_capture),
        );
        let stored = [first.expect("inserts"), second.expect("inserts")];
        assert_eq!(
            stored.iter().filter(|stored| stored.is_some()).count(),
            1,
            "one listing stores it and the other finds it recorded"
        );
        let owner = Reader {
            id: TEST_PERSON_ID,
            reach: Reach::Public,
        };
        let meetings = list_readable(&mut connection, owner, None, 50)
            .await
            .expect("lists");
        assert_eq!(meetings.len(), 1, "no meeting is left without its capture");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_capture_is_fetched_once_and_a_later_fetch_writes_nothing() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let location = storage_location::create(
            &mut connection,
            &cipher,
            &location_for_every_project("Recordings"),
        )
        .await
        .expect("a location");
        let (_meeting, pending) = stored(&mut connection).await;
        assert!(!pending.is_fetched());

        let first = meeting_capture::record_fetched(&mut connection, pending.id, fetched(None))
            .await
            .expect("recorded")
            .expect("the first fetch is stored");
        assert!(first.is_fetched());
        assert_eq!(first.stored_recording(), None);

        // Another fetch, even one that found a recording, leaves the capture exactly as it was.
        let again = meeting_capture::record_fetched(
            &mut connection,
            pending.id,
            fetched(Some(location.id)),
        )
        .await
        .expect("a second write runs");
        assert!(again.is_none(), "a fetched capture is not written again");
        let unchanged = meeting_capture::find(&mut connection, pending.id)
            .await
            .expect("found");
        assert_eq!(unchanged.updated_at, first.updated_at);
        assert_eq!(unchanged.storage_location_id, None);
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
