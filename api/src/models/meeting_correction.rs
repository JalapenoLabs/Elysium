// Copyright © 2026 Jalapeno Labs

//! A meeting's corrected transcript, one per meeting, beside the raw ones it was made from. See
//! `crate::meetings::correct` and `docs/meetings.md`, Correction.
//!
//! The raw transcripts (the source's in `meeting_captures.transcript`, Whisper's in
//! `meeting_transcriptions`) are never changed: correction writes this copy instead, once
//! ([`record`] writes only for a meeting that has none), so a later attempt of the step keeps
//! what the first stored. Approving it ([`approve`]) is optional, and is recorded here with who
//! approved it.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::{meeting_corrections, meetings};
use crate::models::meeting::Meeting;

/// Which transcript a corrected copy follows line by line. Stored as
/// `meeting_corrections.based_on`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasedOn {
    /// The source's lines, with their speakers.
    Source,
    /// Whisper's segments, for a meeting whose source sent no lines. They name no speakers.
    Whisper,
}

impl BasedOn {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Whisper => "whisper",
        }
    }
}

/// A corrected copy as its readers read it. What it was made from (the hashes of the
/// transcripts it read) and its fixes are written with it and read by queries of their own
/// ([`recent_fixes`]).
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = meeting_corrections, check_for_backend(diesel::pg::Pg))]
pub struct MeetingCorrection {
    pub meeting_id: Uuid,
    /// An array of `crate::meetings::source::TranscriptLine`, one per line of the transcript
    /// it follows, in its order, with its speakers and times.
    pub transcript: Value,
    /// The SHA-256 of `transcript` as it was serialized to be stored, as hex.
    pub transcript_hash: String,
    pub changed_lines: i32,
    pub approved_by: Option<Uuid>,
    pub approved_at: Option<DateTime<Utc>>,
    /// The inbox item raised when its run was held for approval, while that item exists.
    pub review_item_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_corrections)]
pub struct NewMeetingCorrection {
    pub meeting_id: Uuid,
    pub transcript: Value,
    pub transcript_hash: String,
    pub based_on: String,
    /// The SHA-256 of the transcripts it read: the source's, and Whisper's when there was one.
    pub source_transcript_hash: String,
    pub whisper_transcript_hash: Option<String>,
    pub changed_lines: i32,
    /// An array of `crate::meetings::correction::Fix`.
    pub fixes: Value,
}

/// Meeting `meeting_id`'s corrected transcript, when it has one.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Option<MeetingCorrection>> {
    meeting_corrections::table
        .find(meeting_id)
        .select(MeetingCorrection::as_select())
        .first(connection)
        .await
        .optional()
}

/// The corrected transcripts of `meeting_ids` that have one, in no order: what a list of
/// meetings says of each one's correction.
///
/// # Errors
/// Propagates any database error.
pub async fn find_many(
    connection: &mut AsyncPgConnection,
    meeting_ids: &[Uuid],
) -> QueryResult<Vec<MeetingCorrection>> {
    meeting_corrections::table
        .filter(meeting_corrections::meeting_id.eq_any(meeting_ids))
        .select(MeetingCorrection::as_select())
        .load(connection)
        .await
}

/// Stores `correction` for its meeting, and answers it. Answers `None`, writing nothing, when
/// the meeting has one already: the first stored is the one every later step reads.
///
/// # Errors
/// Propagates any database error, such as a meeting that no longer exists.
pub async fn record(
    connection: &mut AsyncPgConnection,
    correction: NewMeetingCorrection,
) -> QueryResult<Option<MeetingCorrection>> {
    diesel::insert_into(meeting_corrections::table)
        .values(correction)
        .on_conflict(meeting_corrections::meeting_id)
        .do_nothing()
        .returning(MeetingCorrection::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Records `item_id` as the inbox item raised to review meeting `meeting_id`'s correction.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the meeting has no correction.
pub async fn set_review_item(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    item_id: Uuid,
) -> QueryResult<MeetingCorrection> {
    diesel::update(meeting_corrections::table.find(meeting_id))
        .set(meeting_corrections::review_item_id.eq(item_id))
        .returning(MeetingCorrection::as_returning())
        .get_result(connection)
        .await
}

/// Records that `approved_by` approved meeting `meeting_id`'s correction at `at`. One approved
/// already keeps who approved it first, and is answered as it is.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the meeting has no correction.
pub async fn approve(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    approved_by: Uuid,
    at: DateTime<Utc>,
) -> QueryResult<MeetingCorrection> {
    let approved = diesel::update(meeting_corrections::table.find(meeting_id))
        .filter(meeting_corrections::approved_at.is_null())
        .set((
            meeting_corrections::approved_by.eq(approved_by),
            meeting_corrections::approved_at.eq(at),
        ))
        .returning(MeetingCorrection::as_returning())
        .get_result(connection)
        .await
        .optional()?;
    match approved {
        Some(approved) => Ok(approved),
        None => find(connection, meeting_id)
            .await?
            .ok_or(diesel::result::Error::NotFound),
    }
}

/// `owner`'s meetings whose correction raised inbox item `item_id`: the one an item asking for
/// a review is about.
///
/// # Errors
/// Propagates any database error.
pub async fn meetings_reviewed_by(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
    owner: Uuid,
) -> QueryResult<Vec<Meeting>> {
    meeting_corrections::table
        .inner_join(meetings::table)
        .filter(meeting_corrections::review_item_id.eq(item_id))
        .filter(meetings::created_by.eq(owner))
        .select(Meeting::as_select())
        .load(connection)
        .await
}

#[derive(Debug, QueryableByName)]
struct FixesRow {
    #[diesel(sql_type = diesel::sql_types::Jsonb)]
    fixes: Value,
}

/// The fixes of the newest `limit` corrections of meetings other than `meeting_id`, approved
/// ones first, each correction's as stored: what a correction learns from the company's other
/// meetings.
///
/// # Errors
/// Propagates any database error.
pub async fn recent_fixes(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    limit: i32,
) -> QueryResult<Vec<Value>> {
    let rows: Vec<FixesRow> = diesel::sql_query(
        "SELECT fixes FROM meeting_corrections \
         WHERE meeting_id <> $1 AND changed_lines > 0 \
         ORDER BY approved_at IS NULL, created_at DESC \
         LIMIT $2",
    )
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<Integer, _>(limit)
    .load(connection)
    .await?;
    Ok(rows.into_iter().map(|row| row.fixes).collect())
}

#[derive(Debug, QueryableByName)]
struct SpeakerRow {
    #[diesel(sql_type = Text)]
    speaker: String,
}

/// The names speakers went by in the newest `meetings` meetings other than `meeting_id`, in
/// every capture of each, as their sources labelled them, most often heard first: sources name a
/// speaker from their calendar or account, so these are usually spelled as the people spell
/// themselves.
///
/// # Errors
/// Propagates any database error.
pub async fn recent_speakers(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    meetings: i32,
    limit: i32,
) -> QueryResult<Vec<String>> {
    let rows: Vec<SpeakerRow> = diesel::sql_query(
        "SELECT line->>'speaker' AS speaker \
         FROM (SELECT id FROM meetings WHERE id <> $1 \
               ORDER BY started_at DESC LIMIT $2) AS recent \
         JOIN meeting_captures captures ON captures.meeting_id = recent.id, \
              jsonb_array_elements(captures.transcript) AS line \
         WHERE coalesce(line->>'speaker', '') <> '' \
         GROUP BY 1 ORDER BY count(*) DESC, 1 \
         LIMIT $3",
    )
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<Integer, _>(meetings)
    .bind::<Integer, _>(limit)
    .load(connection)
    .await?;
    Ok(rows.into_iter().map(|row| row.speaker).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::meeting::tests::fetched_with;
    use crate::test_support::migrated_database;

    async fn stored(connection: &mut AsyncPgConnection, transcript: Value) -> Meeting {
        fetched_with(connection, transcript).await.0
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_correction_learns_other_meetings_fixes_and_speakers_but_not_its_own() {
        let (_url, mut connection) = migrated_database().await;
        let line =
            |speaker: &str| json!({ "startMs": 0, "endMs": 1, "speaker": speaker, "text": "Hi" });
        let earlier = stored(
            &mut connection,
            json!([
                line("Priya Raman"),
                line("Priya Raman"),
                line("Jon Smith"),
                line("")
            ]),
        )
        .await;
        let current = stored(&mut connection, json!([line("Someone Else")])).await;
        let fixes = json!([{ "heard": "Ely see um", "written": "Elysium" }]);
        record(
            &mut connection,
            NewMeetingCorrection {
                meeting_id: earlier.id,
                transcript: json!([]),
                transcript_hash: "a".repeat(64),
                based_on: BasedOn::Source.as_str().to_owned(),
                source_transcript_hash: "b".repeat(64),
                whisper_transcript_hash: None,
                changed_lines: 1,
                fixes: fixes.clone(),
            },
        )
        .await
        .expect("records")
        .expect("new");

        assert_eq!(
            recent_fixes(&mut connection, current.id, 30)
                .await
                .expect("reads"),
            vec![fixes]
        );
        assert_eq!(
            recent_fixes(&mut connection, earlier.id, 30)
                .await
                .expect("reads"),
            Vec::<Value>::new(),
            "a meeting learns nothing from its own correction"
        );
        assert_eq!(
            recent_speakers(&mut connection, current.id, 30, 10)
                .await
                .expect("reads"),
            vec!["Priya Raman".to_owned(), "Jon Smith".to_owned()],
            "most often heard first, unnamed speakers left out, its own meeting's too"
        );
    }
}
