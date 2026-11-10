// Copyright © 2026 Jalapeno Labs

//! The meeting workflow's first step, [`FetchMeeting`]: a meeting's transcript and recording,
//! exactly as its source keeps them.
//!
//! One step reads every meeting source, through the meeting's connector and its kind's source
//! ([`Sources::for_kind`]), so a new source needs no step of its own. What it stores is what the
//! source recorded: the timestamped transcript, and the recording uploaded to the connector's
//! storage location. Nothing a source's own AI wrote (summaries, action items, notes) is asked
//! for or kept, since the model behind it is unknown; every later step reads the transcript.
//!
//! # Doing it once
//!
//! A meeting is fetched once. The first fetch stores the transcript with its SHA-256
//! ([`transcript_hash`]) and moves the meeting to `recorded`, in one write that only a meeting
//! never fetched takes (`models::meeting::record_fetched`). Running the step again for the same
//! meeting, a retried attempt or a later run, fetches the source again and compares hashes, and
//! writes and uploads nothing: the first fetch is what every later step has read. The log says
//! whether the source still holds the same transcript.
//!
//! An attempt cut short after its upload but before its write uploads the recording again, to
//! the same path, on the next attempt.
//!
//! # What it logs
//!
//! Every company member can read a run's log, and meetings are their owner's alone, so the log
//! names the source, counts, the hash, and storage paths, never a title, a name, or anything
//! said.

use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{Level, event};
use uuid::Uuid;

use super::media;
use super::source::{RecordingSource, SourceError, TranscriptLine};
use super::sources::Sources;
use crate::agent_models::AgentKind;
use crate::connectors::activity::Activity;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::connector::{self, Connector};
use crate::models::meeting::{self, Fetched, Meeting, StoredRecording};
use crate::realtime::{EventBus, ServerEvent};
use crate::runner::workspace::WorkspaceContainer;
use crate::storage::Storage;
use crate::workflows::{Done, Step, StepContext, StepFailure, StepLog};

/// How many redirects a recording download may follow, each to an `https` named host.
const MAX_REDIRECTS: usize = 5;

/// What [`FetchMeeting`] takes: the meeting to fetch, which the meetings loop stored when its
/// source listed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingToFetch {
    pub meeting_id: Uuid,
}

/// What [`FetchMeeting`] gives: the meeting, which transcript it was fetched with, and where its
/// recording is kept, for the steps that transcribe and correct it.
///
/// A [`MeetingToFetch`] reads as one that was never fetched, which is what the step after
/// receives when a person skips the fetch: every step after it then skips the meeting too, so
/// skipping the fetch skips the meeting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedMeeting {
    pub meeting_id: Uuid,
    /// The SHA-256 of the stored transcript, as hex; `None` when the meeting was not fetched.
    #[serde(default)]
    pub transcript_hash: Option<String>,
    /// `None` when the source keeps no recording, or its connector names nowhere to keep it.
    pub recording: Option<StoredRecording>,
}

/// Fetches one meeting from its source. See the module's docs.
#[derive(Clone)]
pub struct FetchMeeting {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    activity: Activity,
    cipher: Arc<Cipher>,
    storage: Storage,
    workspace: WorkspaceContainer,
    sources: Sources,
    /// For recording downloads: follows redirects only to `https` named hosts, so a source's
    /// address can never lead Elysium into its own network.
    downloads: reqwest::Client,
}

impl std::fmt::Debug for FetchMeeting {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FetchMeeting")
            .finish_non_exhaustive()
    }
}

/// What [`FetchMeeting`] is built from.
#[derive(Clone)]
pub struct FetchMeetingDeps {
    pub database: Pool,
    pub events: EventBus,
    pub activity: Activity,
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    pub workspace: WorkspaceContainer,
    pub sources: Sources,
}

impl FetchMeeting {
    /// # Errors
    /// Fails when the download client cannot be built.
    pub fn new(deps: FetchMeetingDeps) -> anyhow::Result<Self> {
        let downloads = reqwest::Client::builder()
            .user_agent(concat!("elysium-api/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if super::source::is_public_download(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .context("cannot build the recording download client")?;
        Ok(Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                activity: deps.activity,
                cipher: deps.cipher,
                storage: deps.storage,
                workspace: deps.workspace,
                sources: deps.sources,
                downloads,
            }),
        })
    }

    /// One attempt at fetching meeting `meeting_id`.
    async fn fetch(
        &self,
        meeting_id: Uuid,
        log: &StepLog,
    ) -> Result<Done<RecordedMeeting>, StepFailure> {
        let current = self.meeting(meeting_id).await?;
        let account = self.account_of(&current).await?;
        let source = {
            let mut connection = self.connection().await?;
            self.inner
                .sources
                .signed_in(&mut connection, &self.inner.cipher, &account)
                .await
                .map_err(StepFailure::retryable)?
        };
        let _syncing = self.inner.activity.begin(account.id).await;

        log.line(&format!("Fetching the meeting from {}", account.kind));
        let fetched = source
            .fetch(&current.external_id)
            .await
            .map_err(source_failure)?;
        let fetched_hash = transcript_hash(&fetched.transcript);
        log.line(&format!(
            "The source sent {} transcript lines, SHA-256 {fetched_hash}",
            fetched.transcript.len()
        ));

        if current.transcript_hash.is_some() {
            return already_fetched(&current, &fetched_hash, log);
        }

        let recording = self
            .keep_recording(&current, &account, fetched.recording.as_ref(), log)
            .await?;
        let first_fetch = Fetched {
            transcript: serde_json::to_value(&fetched.transcript)
                .map_err(StepFailure::permanent)?,
            transcript_hash: fetched_hash.clone(),
            recording,
        };
        let written = {
            let mut connection = self.connection().await?;
            meeting::record_fetched(&mut connection, meeting_id, first_fetch)
                .await
                .map_err(StepFailure::retryable)?
        };
        let Some(recorded) = written else {
            // Another attempt stored its fetch between this one's read and its write.
            let current = self.meeting(meeting_id).await?;
            return already_fetched(&current, &fetched_hash, log);
        };

        self.inner.events.publish(&ServerEvent::MeetingChanged {
            id: recorded.id,
            state: recorded.state.clone(),
            created_by: recorded.created_by,
        });
        let recording = recorded.stored_recording();
        let line_count = fetched.transcript.len();
        let summary = if recording.is_some() {
            format!("Fetched {line_count} transcript lines and the recording")
        } else {
            format!("Fetched {line_count} transcript lines, with no recording kept")
        };
        Ok(Done::Succeeded {
            output: RecordedMeeting {
                meeting_id,
                transcript_hash: Some(fetched_hash),
                recording,
            },
            summary: Some(summary),
        })
    }

    /// Downloads the recording the source offered and uploads it to the connector's storage
    /// location, answering where it was kept, or `None` when there is nothing to keep or nowhere
    /// to keep it.
    async fn keep_recording(
        &self,
        current: &Meeting,
        account: &Connector,
        offered: Option<&RecordingSource>,
        log: &StepLog,
    ) -> Result<Option<StoredRecording>, StepFailure> {
        let Some(offered) = offered else {
            log.line("The source keeps no recording of this meeting");
            return Ok(None);
        };
        let Some(storage_location_id) = account.storage_location_id else {
            // A connector resumed before its person picked a storage location has nowhere to
            // keep the recording; the meeting goes on from its transcript alone.
            event!(
                name: "meetings.recording.unkept",
                Level::WARN,
                meeting.id = %current.id,
                connector.id = %account.id,
                "the source offered a recording, but the connector names no storage location",
            );
            log.line(
                "The source offered a recording, but the connector names no storage location \
                 to keep it in",
            );
            return Ok(None);
        };

        let (location, access_key) = {
            let mut connection = self.connection().await?;
            super::location(&mut connection, &self.inner.cipher, storage_location_id)
                .await
                .map_err(StepFailure::retryable)?
        };
        let local = media::download(
            &self.inner.downloads,
            &self.inner.workspace,
            current.id,
            offered,
        )
        .await
        .map_err(StepFailure::retryable)?;
        let path = media::recording_path(current.id, media::extension_of(&local.file_name));
        let kept = media::Kept {
            storage: &self.inner.storage,
            location: &location,
            access_key: &access_key,
        };
        media::upload(
            kept,
            &self.inner.workspace,
            current.id,
            &local.file_name,
            &path,
            &local.content_type,
        )
        .await
        .map_err(StepFailure::retryable)?;
        log.line(&format!(
            "Kept the recording ({}) at {path}",
            local.content_type
        ));
        Ok(Some(StoredRecording {
            storage_location_id,
            path,
            content_type: Some(local.content_type),
        }))
    }

    /// The connector `current` was read through, which signs in to its source. One removed, or
    /// one that changed hands, fails the step for good.
    async fn account_of(&self, current: &Meeting) -> Result<Connector, StepFailure> {
        let removed = || {
            StepFailure::permanent("The meeting's connector was removed, so it cannot be fetched")
        };
        let connector_id = current.connector_id.ok_or_else(removed)?;
        let mut connection = self.connection().await?;
        let found = match connector::find(&mut connection, connector_id).await {
            Ok(found) => found,
            Err(diesel::result::Error::NotFound) => return Err(removed()),
            Err(error) => return Err(StepFailure::retryable(error)),
        };
        if found.created_by != current.created_by {
            return Err(StepFailure::permanent(
                "The meeting's connector belongs to someone else",
            ));
        }
        Ok(found)
    }

    /// Meeting `meeting_id`. A meeting that is gone fails the step for good.
    async fn meeting(&self, meeting_id: Uuid) -> Result<Meeting, StepFailure> {
        let mut connection = self.connection().await?;
        match meeting::find(&mut connection, meeting_id).await {
            Ok(found) => Ok(found),
            Err(diesel::result::Error::NotFound) => Err(StepFailure::permanent(
                "The meeting no longer exists, so there is nothing to fetch",
            )),
            Err(error) => Err(StepFailure::retryable(error)),
        }
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

#[cfg(test)]
impl FetchMeeting {
    /// The step over `database`, opening keys with `cipher` and reading meetings through
    /// `sources`, with a workspace and storage that answer nothing: enough for a meeting whose
    /// source keeps no recording.
    pub fn for_test(database: Pool, cipher: Arc<Cipher>, sources: Sources) -> Self {
        let docker = bollard::Docker::connect_with_http(
            "http://127.0.0.1:9",
            5,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("a static address parses");
        let events = EventBus::in_process();
        Self::new(FetchMeetingDeps {
            activity: Activity::new(database.clone(), events.clone()),
            database,
            events,
            cipher,
            storage: Storage::new(reqwest::Client::new()),
            workspace: WorkspaceContainer::new(docker, "elysium-workspace:test".to_owned()),
            sources,
        })
        .expect("the download client builds")
    }
}

impl Step for FetchMeeting {
    const NAME: &'static str = "fetch_meeting";
    /// It reads and stores; no model runs.
    const AGENT_KIND: Option<AgentKind> = None;
    type Input = MeetingToFetch;
    type Output = RecordedMeeting;

    async fn run(
        &self,
        input: MeetingToFetch,
        context: StepContext,
    ) -> Result<Done<RecordedMeeting>, StepFailure> {
        self.fetch(input.meeting_id, &context.log).await
    }
}

/// The SHA-256 of `transcript` serialized as it is stored, in lowercase hex. Two fetches that
/// found the same lines hash alike, and a change to any line, its times, or its speaker changes
/// the hash.
pub fn transcript_hash(transcript: &[TranscriptLine]) -> String {
    let serialized =
        serde_json::to_vec(transcript).expect("transcript lines are plain fields that serialize");
    hex::encode(Sha256::digest(serialized))
}

/// How an attempt ends for a meeting fetched before: with what was stored then, writing nothing,
/// and saying whether the source still holds that transcript.
fn already_fetched(
    current: &Meeting,
    fetched_hash: &str,
    log: &StepLog,
) -> Result<Done<RecordedMeeting>, StepFailure> {
    let Some(stored_hash) = current.transcript_hash.clone() else {
        // Only a write this attempt could not see leads here, which the next attempt reads.
        return Err(StepFailure::retryable(
            "The meeting's fetch was being stored by another attempt",
        ));
    };
    let summary = if stored_hash == fetched_hash {
        "Already fetched, and the source's transcript is unchanged"
    } else {
        log.line(&format!(
            "The source's transcript changed since the meeting was fetched (SHA-256 \
             {stored_hash}); the one fetched first is kept"
        ));
        "Already fetched; the source's transcript changed since, and the first is kept"
    };
    log.line("Nothing was changed");
    Ok(Done::Succeeded {
        output: RecordedMeeting {
            meeting_id: current.id,
            transcript_hash: Some(stored_hash),
            recording: current.stored_recording(),
        },
        summary: Some(summary.to_owned()),
    })
}

/// A source's refusal as a step failure: a key it refuses, or a meeting it does not have, fails
/// the step for good until a person acts; anything else may pass on another attempt.
fn source_failure(error: SourceError) -> StepFailure {
    match error {
        SourceError::Unauthorized(_) | SourceError::NotFound => StepFailure::permanent(error),
        SourceError::Refused(_) => StepFailure::retryable(error),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::Router;
    use axum::extract::State;
    use diesel_async::AsyncPgConnection;
    use serde_json::{Value, json};
    use url::Url;

    use super::*;
    use crate::connectors::ConnectorKind;
    use crate::meetings::fireflies::Fireflies;
    use crate::models::connector::{AccountRef, NewConnector};
    use crate::models::meeting::{MeetingState, NewMeeting};
    use crate::models::token_account::{self, NewTokenAccount};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    fn line(start_ms: i64, speaker: &str, text: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms,
            end_ms: start_ms + 1000,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_transcript_hashes_as_its_stored_json_so_the_same_lines_hash_alike() {
        // The SHA-256 of `[]`: the hash is of the JSON the transcript is stored as, which keeps
        // it the same from one build to the next.
        assert_eq!(
            transcript_hash(&[]),
            "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
        );

        let lines = vec![
            line(0, "Ada", "Launch is Friday."),
            line(2000, "Grace", "Agreed."),
        ];
        assert_eq!(transcript_hash(&lines), transcript_hash(&lines.clone()));

        let mut reworded = lines.clone();
        reworded[1].text = "Agreed!".to_owned();
        let mut retimed = lines.clone();
        retimed[1].start_ms += 1;
        let mut reattributed = lines.clone();
        reattributed[1].speaker = "Ada".to_owned();
        for changed in [reworded, retimed, reattributed] {
            assert_ne!(transcript_hash(&changed), transcript_hash(&lines));
        }
    }

    #[test]
    fn a_refused_key_or_a_missing_meeting_waits_for_a_person_and_anything_else_is_retried() {
        let unauthorized = source_failure(SourceError::Unauthorized("copy it again".to_owned()));
        assert!(!unauthorized.retryable);
        assert!(!source_failure(SourceError::NotFound).retryable);
        assert!(source_failure(SourceError::Refused("slow down".to_owned())).retryable);
    }

    /// A fake Fireflies that answers every query with one meeting's sentences and no
    /// recording, the sentences replaceable as a test goes.
    struct FakeFireflies {
        sentences: Arc<Mutex<Value>>,
        endpoint: Url,
    }

    impl FakeFireflies {
        async fn start(sentences: Value) -> Self {
            let sentences = Arc::new(Mutex::new(sentences));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let address = listener.local_addr().expect("address");
            let endpoint = Url::parse(&format!("http://{address}/graphql")).expect("a url");
            let router = Router::new()
                .fallback(answer_fireflies)
                .with_state(Arc::clone(&sentences));
            tokio::spawn(async move { axum::serve(listener, router).await });
            Self {
                sentences,
                endpoint,
            }
        }

        fn sources(&self) -> Sources {
            Sources::with_fireflies(Fireflies::at(reqwest::Client::new(), self.endpoint.clone()))
        }

        fn replace_sentences(&self, sentences: Value) {
            *self.sentences.lock().expect("lock") = sentences;
        }
    }

    async fn answer_fireflies(State(sentences): State<Arc<Mutex<Value>>>) -> axum::Json<Value> {
        let sentences = sentences.lock().expect("lock").clone();
        axum::Json(json!({
            "data": {
                "transcript": {
                    "id": "01HX",
                    "video_url": null,
                    "audio_url": null,
                    "sentences": sentences,
                },
            },
        }))
    }

    /// A pending meeting of the test person's Fireflies connector, which names no storage
    /// location.
    async fn pending_meeting(connection: &mut AsyncPgConnection, cipher: &Cipher) -> Meeting {
        let key = token_account::create(
            connection,
            NewTokenAccount::new(
                cipher,
                "fireflies",
                "Fireflies",
                &secrecy::SecretString::from("fireflies-key"),
                "ada@example.com",
                TEST_PERSON_ID,
            ),
        )
        .await
        .expect("a key");
        let account = connector::create(
            connection,
            NewConnector::new(
                ConnectorKind::Fireflies.as_str(),
                AccountRef::Token(key.id),
                TEST_PERSON_ID,
            ),
        )
        .await
        .expect("a connector");
        meeting::insert_new(
            connection,
            NewMeeting {
                id: Uuid::now_v7(),
                connector_id: Some(account.id),
                source_kind: "fireflies".to_owned(),
                external_id: "01HX".to_owned(),
                title: "Quarterly roadmap".to_owned(),
                started_at: chrono::Utc::now(),
                duration_ms: Some(1_800_000),
                organizer: "ada@example.com".to_owned(),
                participants: json!(["ada@example.com"]),
                url: None,
                transcript: json!([]),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("insert")
        .expect("new")
    }

    /// One attempt at fetching `meeting_id`, its output and summary, and what it logged.
    async fn fetch_once(
        step: &FetchMeeting,
        database: &Pool,
        meeting_id: Uuid,
    ) -> (RecordedMeeting, Option<String>, String) {
        let context = StepContext::for_test::<FetchMeeting>(database.clone());
        let log = context.log.clone();
        let done = step
            .run(MeetingToFetch { meeting_id }, context)
            .await
            .expect("the fetch succeeds");
        let Done::Succeeded { output, summary } = done else {
            panic!("a fetch is never held");
        };
        (output, summary, log.unstored())
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn fetching_the_same_meeting_again_changes_nothing() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let cipher = Arc::new(crate::test_support::cipher());
        let pending = pending_meeting(&mut connection, &cipher).await;
        let fireflies = FakeFireflies::start(json!([
            { "start_time": 0.5, "end_time": 3.0, "speaker_name": "Ada", "text": "Launch is Friday." },
            { "start_time": "4", "end_time": "6.5", "speaker_name": "Grace", "text": "Agreed." },
        ]))
        .await;
        let step = FetchMeeting::for_test(database.clone(), cipher, fireflies.sources());

        // The first fetch stores the transcript as the source sent it, with its hash.
        let (first, first_summary, first_log) = fetch_once(&step, &database, pending.id).await;
        let expected_lines = vec![
            TranscriptLine {
                start_ms: 500,
                end_ms: 3000,
                speaker: "Ada".to_owned(),
                text: "Launch is Friday.".to_owned(),
            },
            TranscriptLine {
                start_ms: 4000,
                end_ms: 6500,
                speaker: "Grace".to_owned(),
                text: "Agreed.".to_owned(),
            },
        ];
        assert_eq!(first.meeting_id, pending.id);
        assert_eq!(
            first.transcript_hash,
            Some(transcript_hash(&expected_lines))
        );
        assert_eq!(first.recording, None, "the source keeps no recording");
        assert_eq!(
            first_summary.as_deref(),
            Some("Fetched 2 transcript lines, with no recording kept")
        );
        let recorded = meeting::find(&mut connection, pending.id)
            .await
            .expect("found");
        assert_eq!(recorded.state(), MeetingState::Recorded);
        assert_eq!(
            serde_json::from_value::<Vec<TranscriptLine>>(recorded.transcript.clone())
                .expect("stored lines"),
            expected_lines
        );
        assert_eq!(
            recorded.transcript_hash.as_deref(),
            first.transcript_hash.as_deref()
        );

        // The same meeting again: the same answer, and the meeting untouched.
        let (again, again_summary, again_log) = fetch_once(&step, &database, pending.id).await;
        assert_eq!(again, first);
        assert_eq!(
            again_summary.as_deref(),
            Some("Already fetched, and the source's transcript is unchanged")
        );
        let unchanged = meeting::find(&mut connection, pending.id)
            .await
            .expect("found");
        assert_eq!(unchanged.updated_at, recorded.updated_at);

        // The source reworded a line since: the first fetch is still the one kept.
        fireflies.replace_sentences(json!([
            { "start_time": 0.5, "end_time": 3.0, "speaker_name": "Ada", "text": "Launch is Monday." },
        ]));
        let (changed, changed_summary, changed_log) =
            fetch_once(&step, &database, pending.id).await;
        assert_eq!(changed, first);
        assert_eq!(
            changed_summary.as_deref(),
            Some("Already fetched; the source's transcript changed since, and the first is kept")
        );
        let kept = meeting::find(&mut connection, pending.id)
            .await
            .expect("found");
        assert_eq!(kept.updated_at, recorded.updated_at);
        assert_eq!(kept.transcript, recorded.transcript);

        // Every member can read a run's log, so it never says what the meeting is or what was
        // said in it.
        for log in [first_log, again_log, changed_log] {
            for private in ["Quarterly roadmap", "Ada", "Grace", "Launch", "Agreed"] {
                assert!(!log.contains(private), "the log names {private:?}: {log}");
            }
        }
    }
}
