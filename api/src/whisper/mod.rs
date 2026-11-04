// Copyright © 2026 Jalapeno Labs

//! Whisper: transcribes a recording word by word on the host's own CPUs, in a container of its
//! own with no network. See `docs/meetings.md`, Transcription.
//!
//! # A container per transcription
//!
//! Anthropic offers no transcription of recorded audio, and a recording never leaves the host,
//! so the engine runs faster-whisper itself, from the image `whisper/` builds, with Whisper
//! large-v3 baked in. Each transcription is one container ([`Whisper::transcribe`]), created
//! through the filtered Docker proxy the way the engine runs the workspace, the mail server,
//! and browsers, and removed when it ends ([`container`]):
//!
//! - it has no network at all (`network_mode: none`). The recording goes in on its standard
//!   input and the transcript comes out on its standard output, both over Docker's attach
//!   stream, so it shares no network and no volume with anything;
//! - it keeps nothing, so a crash leaves nothing half done, and a release's image is the only
//!   thing to keep in step;
//! - it costs nothing while no meeting is being transcribed. Loading the model takes seconds,
//!   nothing beside an hour of audio, where a service kept running would hold its 3 GB of
//!   memory between meetings.
//!
//! # CPUs
//!
//! A transcription decodes on [`transcription_threads`] threads, and its container is capped at
//! as many CPUs, at a quarter of the default CPU weight, so the API, Postgres, and agents' turns
//! win whenever they want the same cores. One transcription runs at a time per engine: two
//! would only share the same cores, each finishing later.

mod container;

use std::fmt::Display;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::{
    AttachContainerOptionsBuilder, CreateContainerOptionsBuilder, CreateImageOptionsBuilder,
    RemoveContainerOptionsBuilder,
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;
use tracing::{Level, event};
use uuid::Uuid;

use crate::runner::workspace::{LineSplitter, split_reference};

/// The most threads a transcription decodes on. `CTranslate2` gains little past this, and every
/// thread more is one the host's other work cannot have.
const MAX_THREADS: usize = 16;

/// How long one transcription may take before it is given up on. An hour of audio took 36
/// minutes on 4 threads of a Ryzen 9 7950X (`docs/meetings.md`, Transcription), so six hours
/// covers a ten hour recording on a small host, and a hung container is still let go of.
const TRANSCRIPTION_TIMEOUT: Duration = Duration::from_hours(6);

/// The longest transcript read back. An hour of speech is about 1 MB of JSON with every word
/// timed; this is room for days of audio, and a bound on what a broken container can make the
/// engine hold.
const MAX_TRANSCRIPT_BYTES: usize = 64 * 1024 * 1024;

/// How much of what the container printed a failure keeps, from its end, where the error is.
const OUTPUT_TAIL: usize = 1_000;

/// The exit codes `whisper/transcribe.py` answers besides 0; change them together.
const EXIT_UNREADABLE: i64 = 2;
const EXIT_NO_AUDIO: i64 = 3;

/// The threads a transcription decodes on, on a host with `cpus` logical CPUs: half of them,
/// which on a host with two threads per core is about one per physical core, where matrix
/// work stops gaining from more threads, and no more than [`MAX_THREADS`]. The other half stays
/// with the API, the engine, Postgres, and agents' turns. On the 32 vCPU production VM that is
/// 16.
pub fn transcription_threads(cpus: usize) -> usize {
    (cpus / 2).clamp(1, MAX_THREADS)
}

/// A recording's transcript as Whisper heard it: timed segments, each with its timed words,
/// every time in whole milliseconds from the recording's start, as the source's transcript is.
/// Whisper tells no speakers apart, so a segment names none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperTranscript {
    /// The model and the build that transcribed it, as the image names them.
    pub model: String,
    /// The language Whisper detected, as an ISO 639-1 code such as `en`.
    pub language: String,
    pub language_probability: f64,
    /// How long the recording's audio runs.
    pub duration_ms: i64,
    pub segments: Vec<WhisperSegment>,
}

/// A stretch of speech Whisper decoded together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    /// What was said, trimmed.
    pub text: String,
    pub words: Vec<WhisperWord>,
}

/// One word, as Whisper wrote it: its leading space is kept, so a segment's words joined with
/// nothing between them read as the segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperWord {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// How sure Whisper was of the word, from 0 to 1.
    pub probability: f64,
}

/// What a transcription found.
#[derive(Debug, Clone, PartialEq)]
pub enum Transcribed {
    Transcript(WhisperTranscript),
    /// The recording holds no audio stream, such as a video recorded without sound.
    NoAudio,
}

/// Why a transcription failed.
#[derive(Debug, thiserror::Error)]
pub enum WhisperError {
    #[error("Docker: {0}")]
    Docker(#[from] DockerError),
    #[error("the recording could not be read from storage: {0}")]
    Recording(String),
    #[error("Whisper cannot read the recording: {0}")]
    Unreadable(String),
    #[error("Whisper ran out of its {} GiB of memory", container::MEMORY_BYTES >> 30)]
    OutOfMemory,
    #[error("Whisper took longer than {} hours", TRANSCRIPTION_TIMEOUT.as_secs() / 3600)]
    TimedOut,
    #[error("Whisper exited with status {status}: {output}")]
    Exited { status: i64, output: String },
    #[error("Whisper's transcript is larger than {MAX_TRANSCRIPT_BYTES} bytes")]
    TooLarge,
    #[error("Whisper's transcript is not in the shape Elysium reads: {0}")]
    Malformed(serde_json::Error),
}

impl WhisperError {
    /// Whether another attempt may get past it: Docker or storage not answering, or the
    /// container stopping for a reason of its own. A recording it cannot read, a transcription
    /// that ran out of memory or time, and a transcript Elysium cannot read would all fail the
    /// same way again.
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Docker(_) | Self::Recording(_) | Self::Exited { .. }
        )
    }
}

/// Runs transcriptions, one at a time, each in a container of its own. Cheap to clone; clones
/// share the one slot.
#[derive(Debug, Clone)]
pub struct Whisper {
    docker: Docker,
    /// The `elysium-whisper` image of the same release as the engine.
    image: String,
    threads: usize,
    slot: Arc<Semaphore>,
}

impl Whisper {
    /// Transcribes in `image` through `docker`, on [`transcription_threads`] of the CPUs this
    /// process sees, which in the engine's container are the host's.
    pub fn new(docker: Docker, image: String) -> Self {
        let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Self {
            docker,
            image,
            threads: transcription_threads(cpus),
            slot: Arc::new(Semaphore::new(1)),
        }
    }

    /// The threads each transcription decodes on.
    pub const fn threads(&self) -> usize {
        self.threads
    }

    /// Transcribes `recording`, streamed in as it arrives, in a container named for `key`: a
    /// step's job id, so an attempt that follows one cut short removes what it left running.
    /// Every line the container reports goes to `progress`; none of them quotes what was said.
    /// Waits while another transcription runs.
    ///
    /// # Errors
    /// [`WhisperError`]; [`WhisperError::is_retryable`] says whether another attempt may pass.
    pub async fn transcribe<Recording, Failure>(
        &self,
        key: Uuid,
        recording: Recording,
        progress: impl Fn(&str),
    ) -> Result<Transcribed, WhisperError>
    where
        Recording: Stream<Item = Result<Bytes, Failure>> + Unpin,
        Failure: Display,
    {
        let _slot = match Arc::clone(&self.slot).try_acquire_owned() {
            Ok(slot) => slot,
            Err(_busy) => {
                progress("Waiting for another transcription to finish");
                Arc::clone(&self.slot)
                    .acquire_owned()
                    .await
                    .expect("the slot's semaphore is never closed")
            }
        };
        self.ensure_image(&progress).await?;

        let name = container::name(key);
        self.remove(&name).await?;
        self.docker
            .create_container(
                Some(CreateContainerOptionsBuilder::new().name(&name).build()),
                container::spec(&self.image, self.threads),
            )
            .await?;
        let running = Running {
            docker: self.docker.clone(),
            name,
            removed: false,
        };
        progress(&format!(
            "Transcribing on {} threads, in a container with no network",
            self.threads
        ));

        let finished = tokio::time::timeout(
            TRANSCRIPTION_TIMEOUT,
            self.run(&running.name, recording, &progress),
        )
        .await;
        running.remove().await;
        finished.map_err(|_elapsed| WhisperError::TimedOut)?
    }

    /// Attaches to the created container, starts it, feeds it the recording, and reads what it
    /// answers.
    async fn run<Recording, Failure>(
        &self,
        name: &str,
        recording: Recording,
        progress: &impl Fn(&str),
    ) -> Result<Transcribed, WhisperError>
    where
        Recording: Stream<Item = Result<Bytes, Failure>> + Unpin,
        Failure: Display,
    {
        // Attached before it starts, so nothing it prints is missed.
        let attach = AttachContainerOptionsBuilder::new()
            .stdin(true)
            .stdout(true)
            .stderr(true)
            .stream(true)
            .build();
        let attached = self.docker.attach_container(name, Some(attach)).await?;
        self.docker.start_container(name, None).await?;

        let feeding = async {
            let fed = feed(attached.input, recording).await;
            if fed.is_err() {
                // The container waits for the rest of a recording that will never come.
                let _gone = self.docker.kill_container(name, None).await;
            }
            fed
        };
        let (fed, read) = tokio::join!(feeding, read_output(attached.output, progress));
        fed?;
        let output = read?;

        let status = self.exit_status(name).await?;
        match status {
            0 => {
                let transcript =
                    serde_json::from_slice(&output.stdout).map_err(WhisperError::Malformed)?;
                Ok(Transcribed::Transcript(transcript))
            }
            EXIT_NO_AUDIO => Ok(Transcribed::NoAudio),
            EXIT_UNREADABLE => Err(WhisperError::Unreadable(output.stderr_tail)),
            status => {
                let inspected = self.docker.inspect_container(name, None).await?;
                let out_of_memory = inspected
                    .state
                    .and_then(|state| state.oom_killed)
                    .unwrap_or(false);
                if out_of_memory {
                    return Err(WhisperError::OutOfMemory);
                }
                Err(WhisperError::Exited {
                    status,
                    output: output.stderr_tail,
                })
            }
        }
    }

    /// How the container ended, once it has.
    async fn exit_status(&self, name: &str) -> Result<i64, WhisperError> {
        let mut waiting = self.docker.wait_container(name, None);
        match waiting.next().await {
            Some(Ok(response)) => Ok(response.status_code),
            // bollard answers a non-zero exit as an error carrying the status.
            Some(Err(DockerError::DockerContainerWaitError { code, .. })) => Ok(code),
            Some(Err(error)) => Err(error.into()),
            None => Err(WhisperError::Exited {
                status: -1,
                output: "Docker did not say how the transcription ended".to_owned(),
            }),
        }
    }

    /// Pulls the image unless Docker has it already. A release's image is about 4 GB, so the
    /// first transcription after an update waits for it.
    async fn ensure_image(&self, progress: &impl Fn(&str)) -> Result<(), WhisperError> {
        match self.docker.inspect_image(&self.image).await {
            Ok(_) => return Ok(()),
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }
        event!(name: "whisper.image.pull_started", Level::INFO, container.image.name = %self.image, "pulling");
        progress("Pulling Whisper's image, which carries the model");
        let (repository, tag) = split_reference(&self.image);
        let options = CreateImageOptionsBuilder::new()
            .from_image(repository)
            .tag(tag)
            .build();
        let mut pulling = self.docker.create_image(Some(options), None, None);
        while let Some(update) = pulling.next().await {
            update?;
        }
        Ok(())
    }

    /// Removes container `name`, running or not, if it exists.
    async fn remove(&self, name: &str) -> Result<(), WhisperError> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        match self.docker.remove_container(name, Some(options)).await {
            Err(error) if !has_status(&error, 404) => Err(error.into()),
            _removed_or_missing => Ok(()),
        }
    }
}

/// A transcription's container, removed when it is done with, or, when the attempt is dropped
/// mid-way (its engine stopping, or its claim lost), removed in the background.
struct Running {
    docker: Docker,
    name: String,
    removed: bool,
}

impl Running {
    /// Removes the container, logging rather than failing when Docker refuses: what it left
    /// is removed by name when its step runs again.
    async fn remove(mut self) {
        self.removed = true;
        remove_logged(&self.docker, &self.name).await;
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.removed {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            event!(
                name: "whisper.container.left_running",
                Level::WARN,
                container.name = %self.name,
                "a transcription was dropped outside the runtime; its step's next attempt removes it",
            );
            return;
        };
        let docker = self.docker.clone();
        let name = std::mem::take(&mut self.name);
        runtime.spawn(async move { remove_logged(&docker, &name).await });
    }
}

async fn remove_logged(docker: &Docker, name: &str) {
    let options = RemoveContainerOptionsBuilder::new().force(true).build();
    match docker.remove_container(name, Some(options)).await {
        Err(error) if !has_status(&error, 404) => event!(
            name: "whisper.container.remove_failed",
            Level::WARN,
            container.name = %name,
            error.message = %error,
            "a transcription's container could not be removed; its step's next attempt removes it",
        ),
        _removed_or_missing => {}
    }
}

/// Streams `recording` into the container's standard input, then closes it, which is how the
/// transcriber knows the recording is whole.
///
/// The container closing its end first is not a failure here: it stopped reading because it
/// failed, and its exit status says why.
async fn feed<Recording, Failure>(
    mut input: Pin<Box<dyn AsyncWrite + Send>>,
    mut recording: Recording,
) -> Result<(), WhisperError>
where
    Recording: Stream<Item = Result<Bytes, Failure>> + Unpin,
    Failure: Display,
{
    while let Some(chunk) = recording.next().await {
        let chunk = chunk.map_err(|error| WhisperError::Recording(format!("{error:#}")))?;
        if input.write_all(&chunk).await.is_err() {
            return Ok(());
        }
    }
    // Ends the write half alone; the transcript still arrives on the read half.
    let _closed_early = input.shutdown().await;
    Ok(())
}

/// What a container printed: its standard output whole, and the end of its standard error.
#[derive(Debug, Default)]
struct Output {
    stdout: Vec<u8>,
    stderr_tail: String,
}

/// Reads the attach stream until the container ends: standard output is the transcript, and
/// each line of standard error is progress.
async fn read_output(
    mut output: Pin<Box<dyn Stream<Item = Result<LogOutput, DockerError>> + Send>>,
    progress: &impl Fn(&str),
) -> Result<Output, WhisperError> {
    let mut read = Output::default();
    let mut lines = LineSplitter::default();
    let mut report = |line: String| {
        progress(&line);
        read.stderr_tail.push_str(&line);
        read.stderr_tail.push('\n');
        read.stderr_tail = tail(&read.stderr_tail, OUTPUT_TAIL);
    };
    while let Some(chunk) = output.next().await {
        match chunk? {
            LogOutput::StdOut { message } => {
                if read.stdout.len() + message.len() > MAX_TRANSCRIPT_BYTES {
                    return Err(WhisperError::TooLarge);
                }
                read.stdout.extend_from_slice(&message);
            }
            LogOutput::StdErr { message } => lines.push(&message).into_iter().for_each(&mut report),
            LogOutput::Console { .. } | LogOutput::StdIn { .. } => {}
        }
    }
    if let Some(line) = lines.finish() {
        report(line);
    }
    read.stderr_tail = read.stderr_tail.trim_end().to_owned();
    Ok(read)
}

/// The end of `text`, at most `limit` bytes, never cut inside a character.
fn tail(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

fn has_status(error: &DockerError, status: u16) -> bool {
    matches!(error, DockerError::DockerResponseServerError { status_code, .. } if *status_code == status)
}

#[cfg(test)]
mod tests;
