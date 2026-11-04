// Copyright © 2026 Jalapeno Labs

//! What Elysium's own Whisper heard in a meeting's recording, one per meeting, beside the
//! source's transcript. See `crate::meetings::transcribe` and `docs/meetings.md`,
//! Transcription.
//!
//! A meeting is transcribed once: [`record`] writes only for a meeting that has none, so a
//! later attempt of the step keeps what the first stored, as a meeting's fetch does.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::meeting_transcriptions;

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = meeting_transcriptions, check_for_backend(diesel::pg::Pg))]
#[expect(
    dead_code,
    reason = "the transcription step reads only the hash; correction (#79) and segmenting (#80) \
              read the transcript"
)]
pub struct MeetingTranscription {
    pub meeting_id: Uuid,
    /// A `crate::whisper::WhisperTranscript`.
    pub transcript: Value,
    /// The SHA-256 of `transcript` as it was serialized to be stored, as hex.
    pub transcript_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_transcriptions)]
pub struct NewMeetingTranscription {
    pub meeting_id: Uuid,
    pub transcript: Value,
    pub transcript_hash: String,
}

/// Meeting `meeting_id`'s transcription, when it has one.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Option<MeetingTranscription>> {
    meeting_transcriptions::table
        .find(meeting_id)
        .select(MeetingTranscription::as_select())
        .first(connection)
        .await
        .optional()
}

/// Stores `transcription` for its meeting, and answers it. Answers `None`, writing nothing,
/// when the meeting was transcribed already: the first transcription stored is the one every
/// later step reads.
///
/// # Errors
/// Propagates any database error, such as a meeting that no longer exists.
pub async fn record(
    connection: &mut AsyncPgConnection,
    transcription: NewMeetingTranscription,
) -> QueryResult<Option<MeetingTranscription>> {
    diesel::insert_into(meeting_transcriptions::table)
        .values(transcription)
        .on_conflict(meeting_transcriptions::meeting_id)
        .do_nothing()
        .returning(MeetingTranscription::as_returning())
        .get_result(connection)
        .await
        .optional()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::meeting::{self, NewMeeting};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_keeps_its_first_transcription_and_loses_it_with_the_meeting() {
        let (_url, mut connection) = migrated_database().await;
        let stored = meeting::insert_new(
            &mut connection,
            NewMeeting {
                id: Uuid::now_v7(),
                connector_id: None,
                source_kind: "fireflies".to_owned(),
                external_id: "01HX".to_owned(),
                title: "Weekly sync".to_owned(),
                started_at: Utc::now(),
                duration_ms: None,
                organizer: String::new(),
                participants: json!([]),
                url: None,
                transcript: json!([]),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("insert")
        .expect("new");
        assert!(
            find(&mut connection, stored.id)
                .await
                .expect("reads")
                .is_none()
        );

        let first = NewMeetingTranscription {
            meeting_id: stored.id,
            transcript: json!({ "segments": [] }),
            transcript_hash: "a".repeat(64),
        };
        let recorded = record(&mut connection, first)
            .await
            .expect("records")
            .expect("the first is stored");
        assert_eq!(recorded.transcript_hash, "a".repeat(64));

        // A second attempt's transcription writes nothing; the first stays.
        let second = NewMeetingTranscription {
            meeting_id: stored.id,
            transcript: json!({ "segments": [{ "text": "another" }] }),
            transcript_hash: "b".repeat(64),
        };
        assert!(
            record(&mut connection, second)
                .await
                .expect("answers")
                .is_none()
        );
        let kept = find(&mut connection, stored.id)
            .await
            .expect("reads")
            .expect("kept");
        assert_eq!(kept.transcript_hash, "a".repeat(64));
        assert_eq!(kept.transcript, json!({ "segments": [] }));

        diesel::delete(crate::database::schema::meetings::table.find(stored.id))
            .execute(&mut connection)
            .await
            .expect("deletes");
        assert!(
            find(&mut connection, stored.id)
                .await
                .expect("reads")
                .is_none()
        );
    }
}
