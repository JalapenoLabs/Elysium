// Copyright © 2026 Jalapeno Labs

//! A session's harness session, kept so a later thread can resume the same conversation.
//!
//! After each turn the fleet exports the thread's harness session (Claude's transcript,
//! Codex's rollout) and keeps the latest here, one row per session. A transcript holds
//! everything the agent read, which the satellite does not scrub, so it is compressed with
//! zstd and then sealed like any application secret. See `docs/studio.md`, Continuing.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::{coding_sessions, session_transcripts};

/// The largest sealed transcript kept, 64 MiB. A long conversation with attached images runs
/// to tens of megabytes before compression; a transcript past this is refused loudly rather
/// than cut short, since a truncated transcript would not resume.
pub const MAX_SEALED_BYTES: usize = 64 * 1024 * 1024;

/// zstd's default level: most of the ratio of higher levels at a fraction of their time,
/// which matters because a transcript is compressed after every turn.
const COMPRESSION_LEVEL: i32 = 3;

/// The harness family that wrote a transcript. A thread resumes only its own family's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessFamily {
    Claude,
    Codex,
}

impl HarnessFamily {
    const fn as_column(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    fn from_column(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

/// A transcript as stored: still sealed.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = session_transcripts, check_for_backend(diesel::pg::Pg))]
pub struct SessionTranscript {
    pub session_id: i64,
    harness: String,
    pub harness_session_id: String,
    transcript_sealed: Vec<u8>,
    /// The export's size before compression.
    pub size_bytes: i64,
    pub updated_at: DateTime<Utc>,
}

/// Why a transcript could not be kept or read back.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    #[error(
        "the transcript is {sealed_bytes} bytes sealed, over the {MAX_SEALED_BYTES} byte limit"
    )]
    TooLarge { sealed_bytes: usize },
    #[error("the transcript could not be compressed or decompressed: {0}")]
    Compression(#[from] std::io::Error),
    #[error("the transcript cannot be decrypted: {0}")]
    Sealed(#[from] OpenError),
    #[error("the stored harness {0} is not one this build knows")]
    UnknownHarness(String),
}

impl SessionTranscript {
    /// The family that wrote it.
    ///
    /// # Errors
    /// Returns [`TranscriptError::UnknownHarness`] for a value the check constraint would
    /// refuse, which means the row was written by hand.
    pub fn harness(&self) -> Result<HarnessFamily, TranscriptError> {
        HarnessFamily::from_column(&self.harness)
            .ok_or_else(|| TranscriptError::UnknownHarness(self.harness.clone()))
    }

    /// Decrypts and decompresses the export, exactly as the satellite handed it over.
    ///
    /// # Errors
    /// Returns [`TranscriptError::Sealed`] if the key changed or the bytes were altered or
    /// copied from another row, and [`TranscriptError::Compression`] if they do not
    /// decompress.
    pub fn open(&self, cipher: &Cipher) -> Result<Vec<u8>, TranscriptError> {
        use secrecy::ExposeSecret as _;

        let compressed = cipher.open(
            &self.transcript_sealed,
            &transcript_context(self.session_id),
        )?;
        Ok(zstd::decode_all(compressed.expose_secret())?)
    }
}

/// Associated data binding a sealed transcript to its session, so one copied into another
/// row refuses to open.
fn transcript_context(session_id: i64) -> Vec<u8> {
    let mut context = b"session_transcripts.transcript:".to_vec();
    context.extend_from_slice(&session_id.to_be_bytes());
    context
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = session_transcripts)]
struct TranscriptRow<'row> {
    session_id: i64,
    harness: &'row str,
    harness_session_id: &'row str,
    transcript_sealed: Vec<u8>,
    size_bytes: i64,
}

/// Keeps `transcript` as the session's latest, replacing any earlier one.
///
/// # Errors
/// Returns [`TranscriptError::TooLarge`] past [`MAX_SEALED_BYTES`], and propagates database
/// and compression errors.
pub async fn store(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    session_id: i64,
    harness: HarnessFamily,
    harness_session_id: &str,
    transcript: &[u8],
) -> Result<(), TranscriptError> {
    let compressed = zstd::encode_all(transcript, COMPRESSION_LEVEL)?;
    let sealed = cipher.seal(&compressed, &transcript_context(session_id));
    if sealed.len() > MAX_SEALED_BYTES {
        return Err(TranscriptError::TooLarge {
            sealed_bytes: sealed.len(),
        });
    }

    let row = TranscriptRow {
        session_id,
        harness: harness.as_column(),
        harness_session_id,
        transcript_sealed: sealed,
        size_bytes: i64::try_from(transcript.len()).unwrap_or(i64::MAX),
    };
    diesel::insert_into(session_transcripts::table)
        .values(&row)
        .on_conflict(session_transcripts::session_id)
        .do_update()
        .set(&row)
        .execute(connection)
        .await?;
    Ok(())
}

/// The most recently saved transcript among a Studio item's sessions, which is where a new
/// thread on the item resumes from.
///
/// # Errors
/// Propagates any database error.
pub async fn latest_for_studio_item(
    connection: &mut AsyncPgConnection,
    studio_item_id: Uuid,
) -> QueryResult<Option<SessionTranscript>> {
    session_transcripts::table
        .inner_join(coding_sessions::table)
        .filter(coding_sessions::studio_item_id.eq(studio_item_id))
        .order(session_transcripts::updated_at.desc())
        .select(SessionTranscript::as_select())
        .first(connection)
        .await
        .optional()
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;
    use crate::models::coding_session::{self, NewCodingSession};
    use crate::models::project::{self, NewProject};
    use crate::models::satellite::{self, NewSatellite};
    use crate::test_support::{cipher, migrated_database};

    async fn session(connection: &mut AsyncPgConnection) -> i64 {
        let satellite_id = satellite::create(
            connection,
            &cipher(),
            &NewSatellite {
                created_by: crate::test_support::TEST_PERSON_ID,
                name: "orbit".to_owned(),
                description: String::new(),
                url: "http://arsox:8080".to_owned(),
                secret: SecretString::from("bearer"),
                is_active: true,
            },
        )
        .await
        .expect("satellite")
        .id;
        let project_id = project::create(
            connection,
            &NewProject {
                created_by: crate::test_support::TEST_PERSON_ID,
                name: "Elysium".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("project")
        .id;
        let new_session = NewCodingSession {
            created_by: crate::test_support::TEST_PERSON_ID,
            id: coding_session::reserve_id(connection)
                .await
                .expect("reserve"),
            project_id: Some(project_id),
            satellite_id,
            thread_id: Uuid::now_v7().to_string(),
            title: "Session".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
        };
        coding_session::create(connection, &new_session)
            .await
            .expect("session")
            .id
    }

    async fn stored(connection: &mut AsyncPgConnection, session_id: i64) -> SessionTranscript {
        session_transcripts::table
            .find(session_id)
            .select(SessionTranscript::as_select())
            .first(connection)
            .await
            .expect("stored")
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_transcript_is_sealed_replaced_and_opened_intact() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let session_id = session(&mut connection).await;
        let first = br#"{"type":"user","message":"model me a banana"}"#.repeat(200);

        store(
            &mut connection,
            &cipher,
            session_id,
            HarnessFamily::Claude,
            "abc",
            &first,
        )
        .await
        .expect("store");
        let kept = stored(&mut connection, session_id).await;
        assert_eq!(kept.harness().expect("known"), HarnessFamily::Claude);
        assert_eq!(kept.open(&cipher).expect("opens"), first);
        assert!(
            kept.transcript_sealed.len() < first.len(),
            "repetitive JSON lines compress"
        );
        assert!(
            !kept
                .transcript_sealed
                .windows(6)
                .any(|window| window == b"banana"),
            "nothing readable is stored"
        );

        let second = b"a later turn".to_vec();
        store(
            &mut connection,
            &cipher,
            session_id,
            HarnessFamily::Claude,
            "abc",
            &second,
        )
        .await
        .expect("replace");
        assert_eq!(
            stored(&mut connection, session_id)
                .await
                .open(&cipher)
                .expect("opens"),
            second
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_transcript_opens_only_in_its_own_row_under_its_own_key() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let session_id = session(&mut connection).await;
        store(
            &mut connection,
            &cipher,
            session_id,
            HarnessFamily::Codex,
            "r1",
            b"rollout",
        )
        .await
        .expect("store");

        let mut moved = stored(&mut connection, session_id).await;
        moved.session_id += 1;
        assert!(matches!(
            moved.open(&cipher),
            Err(TranscriptError::Sealed(_))
        ));

        let kept = stored(&mut connection, session_id).await;
        assert!(matches!(
            kept.open(&crate::test_support::cipher()),
            Err(TranscriptError::Sealed(_))
        ));
    }
}
