// Copyright © 2026 Jalapeno Labs

//! The meeting workflow's second step, [`TranscribeMeeting`]: Elysium's own transcript of a
//! meeting's recording, word by word, from the local Whisper (`crate::whisper`).
//!
//! The source's transcript is the source's; this one is Elysium's, heard from the recording on
//! the host's own CPUs, so nothing leaves the host. It is stored beside the source's
//! (`models::meeting_transcription`), for correction to reconcile the two and for segmenting to
//! cut clips by its word timings.
//!
//! # When there is nothing to hear
//!
//! A meeting whose source keeps no recording, or whose recording has no audio stream, skips
//! the step, saying which: there is nothing to transcribe, and the run goes on with the
//! source's transcript alone.
//!
//! # Doing it once
//!
//! A meeting is transcribed once. The first transcription stored is kept
//! (`meeting_transcription::record` writes only for a meeting that has none), and running the
//! step again answers it without transcribing anything.
//!
//! # What it logs
//!
//! Every company member can read a run's log, and meetings are their owner's alone, so the log
//! carries Whisper's progress, durations, counts, and the hash, never a word of what was said.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::fetch::RecordedMeeting;
use crate::agent_models::AgentKind;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::meeting::StoredRecording;
use crate::models::meeting_transcription::{self, NewMeetingTranscription};
use crate::storage::Storage;
use crate::whisper::{Transcribed, Whisper, WhisperTranscript};
use crate::workflows::{Done, Step, StepContext, StepFailure, StepLog};

/// What [`TranscribeMeeting`] gives: everything the fetch gave, and which Whisper transcript is
/// stored for the meeting.
///
/// A [`RecordedMeeting`] reads as one with no transcript, which is what the step after receives
/// when this one is skipped, by a person or because the meeting had nothing to hear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribedMeeting {
    pub meeting_id: Uuid,
    /// The SHA-256 of the source's stored transcript, as hex.
    pub transcript_hash: String,
    pub recording: Option<StoredRecording>,
    /// The SHA-256 of the Whisper transcript stored for the meeting, as hex; `None` when it was
    /// not transcribed.
    #[serde(default)]
    pub whisper_transcript_hash: Option<String>,
}

/// Transcribes one meeting's recording. See the module's docs.
#[derive(Clone)]
pub struct TranscribeMeeting {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    cipher: Arc<Cipher>,
    storage: Storage,
    whisper: Whisper,
}

impl std::fmt::Debug for TranscribeMeeting {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TranscribeMeeting")
            .finish_non_exhaustive()
    }
}

/// What [`TranscribeMeeting`] is built from.
#[derive(Clone)]
pub struct TranscribeMeetingDeps {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    pub whisper: Whisper,
}

impl TranscribeMeeting {
    pub fn new(deps: TranscribeMeetingDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                cipher: deps.cipher,
                storage: deps.storage,
                whisper: deps.whisper,
            }),
        }
    }

    /// One attempt at transcribing `recorded`, in a container keyed by `step_job_id`.
    async fn transcribe(
        &self,
        recorded: RecordedMeeting,
        step_job_id: Uuid,
        log: &StepLog,
    ) -> Result<Done<TranscribedMeeting>, StepFailure> {
        let Some(recording) = recorded.recording.clone() else {
            log.line("The meeting has no recording kept");
            return Ok(Done::Skipped {
                reason: "No recording, so nothing to transcribe".to_owned(),
            });
        };

        let stored = {
            let mut connection = self.connection().await?;
            meeting_transcription::find(&mut connection, recorded.meeting_id)
                .await
                .map_err(StepFailure::retryable)?
        };
        if let Some(stored) = stored {
            log.line(&format!(
                "Already transcribed, SHA-256 {}; nothing was changed",
                stored.transcript_hash
            ));
            return Ok(transcribed(
                recorded,
                stored.transcript_hash,
                "Already transcribed".to_owned(),
            ));
        }

        let (location, access_key) = {
            let mut connection = self.connection().await?;
            super::location(
                &mut connection,
                &self.inner.cipher,
                recording.storage_location_id,
            )
            .await
            .map_err(StepFailure::retryable)?
        };
        let open = async {
            log.line(&format!("Reading the recording from {}", recording.path));
            let download = self
                .inner
                .storage
                .download(&location, &access_key, &recording.path)
                .await?;
            Ok(download.body)
        };
        let heard = self
            .inner
            .whisper
            .transcribe(step_job_id, open, |line| log.line(line))
            .await
            .map_err(|error| {
                let reason = format!("{error}");
                if error.is_retryable() {
                    StepFailure::retryable(reason)
                } else {
                    StepFailure::permanent(reason)
                }
            })?;
        let transcript = match heard {
            Transcribed::Transcript(transcript) => transcript,
            Transcribed::NoAudio => {
                return Ok(Done::Skipped {
                    reason: "The recording has no audio, so nothing to transcribe".to_owned(),
                });
            }
        };

        let summary = summary_of(&transcript);
        let hash = whisper_transcript_hash(&transcript);
        let written = {
            let mut connection = self.connection().await?;
            meeting_transcription::record(
                &mut connection,
                NewMeetingTranscription {
                    meeting_id: recorded.meeting_id,
                    transcript: serde_json::to_value(&transcript)
                        .map_err(StepFailure::permanent)?,
                    transcript_hash: hash.clone(),
                },
            )
            .await
            .map_err(StepFailure::retryable)?
        };
        let Some(written) = written else {
            // Another attempt stored its transcription first; that one is kept.
            return Err(StepFailure::retryable(
                "The meeting's transcription was stored by another attempt",
            ));
        };
        log.line(&format!("Stored the transcript, SHA-256 {hash}"));
        Ok(transcribed(recorded, written.transcript_hash, summary))
    }

    async fn connection(
        &self,
    ) -> Result<
        diesel_async::pooled_connection::deadpool::Object<diesel_async::AsyncPgConnection>,
        StepFailure,
    > {
        self.inner.database.get().await.map_err(|error| {
            StepFailure::retryable(format!("No database connection available: {error}"))
        })
    }
}

impl Step for TranscribeMeeting {
    const NAME: &'static str = "transcribe_meeting";
    /// Whisper runs on this host; no model of Claude's does.
    const AGENT_KIND: Option<AgentKind> = None;
    type Input = RecordedMeeting;
    type Output = TranscribedMeeting;

    async fn run(
        &self,
        input: RecordedMeeting,
        context: StepContext,
    ) -> Result<Done<TranscribedMeeting>, StepFailure> {
        self.transcribe(input, context.step_job_id, &context.log)
            .await
    }
}

/// The SHA-256 of `transcript` serialized as it is stored, in lowercase hex, as the source's
/// transcript is hashed (`fetch::transcript_hash`).
pub fn whisper_transcript_hash(transcript: &WhisperTranscript) -> String {
    let serialized =
        serde_json::to_vec(transcript).expect("a transcript is plain fields that serialize");
    hex::encode(Sha256::digest(serialized))
}

/// The step's success, naming the transcript stored for the meeting.
fn transcribed(
    recorded: RecordedMeeting,
    whisper_transcript_hash: String,
    summary: String,
) -> Done<TranscribedMeeting> {
    Done::Succeeded {
        output: TranscribedMeeting {
            meeting_id: recorded.meeting_id,
            transcript_hash: recorded.transcript_hash,
            recording: recorded.recording,
            whisper_transcript_hash: Some(whisper_transcript_hash),
        },
        summary: Some(summary),
    }
}

/// The one line a transcription's row collapses to: how much audio, and how much was heard in
/// it, never what.
fn summary_of(transcript: &WhisperTranscript) -> String {
    let words: usize = transcript
        .segments
        .iter()
        .map(|segment| segment.words.len())
        .sum();
    let seconds = transcript.duration_ms / 1000;
    format!(
        "Transcribed {}:{:02}:{:02} of audio in {}: {words} words",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        transcript.language,
    )
}

#[cfg(test)]
impl TranscribeMeeting {
    /// The step over `database`, with storage and a Docker that answer nothing: enough for a
    /// meeting with no recording, or one transcribed already.
    pub fn for_test(database: Pool, cipher: Arc<Cipher>) -> Self {
        let docker = bollard::Docker::connect_with_http(
            "http://127.0.0.1:9",
            5,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("a static address parses");
        Self::new(TranscribeMeetingDeps {
            database,
            cipher,
            storage: Storage::new(reqwest::Client::new()),
            whisper: Whisper::new(docker, "elysium-whisper:test".to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::meeting::{self, NewMeeting};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};
    use crate::whisper::{WhisperSegment, WhisperWord};

    fn unconnected_pool() -> Pool {
        let manager = diesel_async::pooled_connection::AsyncDieselConnectionManager::new(
            "postgres://unused.invalid/none",
        );
        Pool::builder(manager)
            .build()
            .expect("a pool builds without connecting")
    }

    fn transcript(text: &str) -> WhisperTranscript {
        WhisperTranscript {
            model: "Whisper large-v3".to_owned(),
            language: "en".to_owned(),
            language_probability: 0.99,
            duration_ms: 3_723_000,
            segments: vec![WhisperSegment {
                start_ms: 500,
                end_ms: 2_100,
                text: text.to_owned(),
                words: text
                    .split(' ')
                    .enumerate()
                    .map(|(index, word)| WhisperWord {
                        start_ms: 500 + 400 * i64::try_from(index).expect("small"),
                        end_ms: 900 + 400 * i64::try_from(index).expect("small"),
                        text: format!(" {word}"),
                        probability: 0.9,
                    })
                    .collect(),
            }],
        }
    }

    fn recorded(meeting_id: Uuid, recording: Option<StoredRecording>) -> RecordedMeeting {
        RecordedMeeting {
            meeting_id,
            transcript_hash: "a".repeat(64),
            recording,
        }
    }

    #[tokio::test]
    async fn a_meeting_without_a_recording_skips_the_step_and_touches_nothing() {
        // Neither the database nor Docker answers, so reaching either would fail the attempt.
        let step = TranscribeMeeting::for_test(
            unconnected_pool(),
            Arc::new(crate::test_support::cipher()),
        );
        let context = StepContext::for_test::<TranscribeMeeting>(unconnected_pool());
        let log = context.log.clone();

        let done = step
            .run(recorded(Uuid::now_v7(), None), context)
            .await
            .expect("skipping is not a failure");
        assert_eq!(
            done,
            Done::Skipped {
                reason: "No recording, so nothing to transcribe".to_owned(),
            }
        );
        assert!(log.unstored().contains("no recording"));
    }

    #[test]
    fn what_the_fetch_gave_reads_as_a_meeting_with_no_whisper_transcript() {
        // A skipped transcription hands the next step what the fetch gave.
        let fetched = recorded(Uuid::now_v7(), None);
        let read: TranscribedMeeting =
            serde_json::from_value(serde_json::to_value(&fetched).expect("serializes"))
                .expect("reads");
        assert_eq!(read.meeting_id, fetched.meeting_id);
        assert_eq!(read.transcript_hash, fetched.transcript_hash);
        assert_eq!(read.whisper_transcript_hash, None);
    }

    #[test]
    fn a_transcript_hashes_as_its_stored_json() {
        let heard = transcript("Launch is Friday");
        assert_eq!(
            whisper_transcript_hash(&heard),
            whisper_transcript_hash(&heard.clone())
        );
        assert_ne!(
            whisper_transcript_hash(&heard),
            whisper_transcript_hash(&transcript("Launch is Monday"))
        );
        let mut retimed = heard.clone();
        retimed.segments[0].words[1].start_ms += 10;
        assert_ne!(
            whisper_transcript_hash(&heard),
            whisper_transcript_hash(&retimed)
        );
    }

    #[test]
    fn the_summary_counts_what_was_heard_and_never_says_it() {
        let summary = summary_of(&transcript("Launch is Friday"));
        assert_eq!(summary, "Transcribed 1:02:03 of audio in en: 3 words");
        assert!(!summary.contains("Launch"));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_transcribed_already_answers_its_transcript_without_whisper() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let stored = meeting::insert_new(
            &mut connection,
            NewMeeting {
                id: Uuid::now_v7(),
                connector_id: None,
                source_kind: "fireflies".to_owned(),
                external_id: "01HX".to_owned(),
                title: "Weekly sync".to_owned(),
                started_at: chrono::Utc::now(),
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
        let heard = transcript("Launch is Friday");
        let hash = whisper_transcript_hash(&heard);
        meeting_transcription::record(
            &mut connection,
            NewMeetingTranscription {
                meeting_id: stored.id,
                transcript: serde_json::to_value(&heard).expect("serializes"),
                transcript_hash: hash.clone(),
            },
        )
        .await
        .expect("records")
        .expect("new");

        // Docker answers nothing, so transcribing again would fail the attempt.
        let step =
            TranscribeMeeting::for_test(database.clone(), Arc::new(crate::test_support::cipher()));
        let recording = StoredRecording {
            storage_location_id: Uuid::now_v7(),
            path: format!("meetings/{}/recording.mp4", stored.id),
            content_type: Some("video/mp4".to_owned()),
        };
        let context = StepContext::for_test::<TranscribeMeeting>(database);
        let done = step
            .run(recorded(stored.id, Some(recording.clone())), context)
            .await
            .expect("answers what is stored");
        assert_eq!(
            done,
            Done::Succeeded {
                output: TranscribedMeeting {
                    meeting_id: stored.id,
                    transcript_hash: "a".repeat(64),
                    recording: Some(recording),
                    whisper_transcript_hash: Some(hash),
                },
                summary: Some("Already transcribed".to_owned()),
            }
        );
    }
}
