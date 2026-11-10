// Copyright © 2026 Jalapeno Labs

//! What Elysium's own Whisper heard in a capture's recording, one per capture, beside the
//! transcript its source sent. See `crate::meetings::transcribe` and `docs/meetings.md`,
//! Transcription.
//!
//! Whisper transcribes each capture with audio on its own, so a meeting recorded by Teams and by
//! Fireflies has two of these. A capture is transcribed once: [`record`] writes only for a capture
//! that has none, so a later attempt of the step keeps what the first stored, as a capture's
//! fetch does. Its times are on the capture's own clock, as its recording's are.

use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::meeting_transcriptions;

/// A capture's transcription as the steps after it read it: the transcript, and the hash they
/// name it by. Its capture is the one it was found by.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = meeting_transcriptions, check_for_backend(diesel::pg::Pg))]
pub struct MeetingTranscription {
    /// A `crate::whisper::WhisperTranscript`.
    pub transcript: Value,
    /// The SHA-256 of `transcript` as it was serialized to be stored, as hex.
    pub transcript_hash: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_transcriptions)]
pub struct NewMeetingTranscription {
    pub capture_id: Uuid,
    pub transcript: Value,
    pub transcript_hash: String,
}

/// Capture `capture_id`'s transcription, when it has one.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    capture_id: Uuid,
) -> QueryResult<Option<MeetingTranscription>> {
    meeting_transcriptions::table
        .find(capture_id)
        .select(MeetingTranscription::as_select())
        .first(connection)
        .await
        .optional()
}

/// Stores `transcription` for its capture, and answers it. Answers `None`, writing nothing,
/// when the capture was transcribed already: the first transcription stored is the one every
/// later step reads.
///
/// # Errors
/// Propagates any database error, such as a capture that no longer exists.
pub async fn record(
    connection: &mut AsyncPgConnection,
    transcription: NewMeetingTranscription,
) -> QueryResult<Option<MeetingTranscription>> {
    diesel::insert_into(meeting_transcriptions::table)
        .values(transcription)
        .on_conflict(meeting_transcriptions::capture_id)
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
    use crate::models::meeting::tests::stored;
    use crate::test_support::migrated_database;

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_capture_keeps_its_first_transcription_and_loses_it_with_its_meeting() {
        let (_url, mut connection) = migrated_database().await;
        let (meeting, capture) = stored(&mut connection).await;
        assert!(
            find(&mut connection, capture.id)
                .await
                .expect("reads")
                .is_none()
        );

        let first = NewMeetingTranscription {
            capture_id: capture.id,
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
            capture_id: capture.id,
            transcript: json!({ "segments": [{ "text": "another" }] }),
            transcript_hash: "b".repeat(64),
        };
        assert!(
            record(&mut connection, second)
                .await
                .expect("answers")
                .is_none()
        );
        let kept = find(&mut connection, capture.id)
            .await
            .expect("reads")
            .expect("kept");
        assert_eq!(kept.transcript_hash, "a".repeat(64));
        assert_eq!(kept.transcript, json!({ "segments": [] }));

        diesel::delete(crate::database::schema::meetings::table.find(meeting.id))
            .execute(&mut connection)
            .await
            .expect("deletes");
        assert!(
            find(&mut connection, capture.id)
                .await
                .expect("reads")
                .is_none()
        );
    }
}
