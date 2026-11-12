// Copyright © 2026 Jalapeno Labs

//! The frames segmenting pins: the video it is offered stills of, and the stills it chose, kept
//! and pinned over the lines it judged each against. See `docs/meetings.md`, Attachments and
//! frames.
//!
//! A meeting has video when one of its captures keeps a recording that is one, and its frames are
//! kept where page attachments are, so a workspace with no such location chosen pins none and says
//! so. Before the turn, the recording is sampled for where its screen changes most
//! (`crate::meetings::frames`), and the turn gets a grant to `elysium_meeting_frames` over that
//! capture alone (`crate::tools::frames`), with Claude Code's Task tool to hand stills to
//! subagents. The stills it pins are part of its stored answer ([`TakenFrame`]), so an attempt
//! that finds the answer stored pins the same ones. Each is taken again from the recording, kept
//! as a [`crate::images::Frame`], and dropped when it is a near duplicate of a frame the meeting
//! holds already, a person's or an earlier segmenting's, or of one pinned before it.

use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use futures_util::stream;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Recording, SegmentMeeting};
use crate::images;
use crate::meeting_attachments;
use crate::meetings::attachments::{self, AttachmentError};
use crate::meetings::frames::{self, Candidate, SAMPLE_INTERVAL_MS};
use crate::meetings::media;
use crate::meetings::segmentation::OfferedVideo;
use crate::models::agent::Author;
use crate::models::meeting::Meeting;
use crate::models::meeting_attachment::{self, StoredFrame};
use crate::models::meeting_capture::{self, MeetingCapture};
use crate::models::meeting_mark::{MarkError, Range};
use crate::models::storage_location::StorageLocation;
use crate::models::user::{self, ELYSIA_USER_ID};
use crate::tools::SegmentingScope;
use crate::workflows::{StepFailure, StepLog};

/// A frame segmenting pinned, as its answer is stored: its capture and its moment on that
/// capture's clock, the range of words it is pinned over, and what it shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakenFrame {
    pub capture_id: Uuid,
    pub capture_ms: i64,
    pub range: Range,
    pub caption: String,
}

/// The video a meeting's segmenting is offered: the capture, its recording's name in the
/// meeting's workspace directory, how long it runs, and where its screen changes most.
#[derive(Debug, Clone)]
pub struct Video {
    pub capture: MeetingCapture,
    pub recording_name: String,
    pub duration_ms: i64,
    pub candidates: Vec<Candidate>,
}

impl Video {
    /// As the prompt and its answer read it.
    pub fn offered(&self) -> OfferedVideo<'_> {
        OfferedVideo {
            offset_ms: self.capture.offset_ms,
            duration_ms: Some(self.duration_ms),
            candidates: &self.candidates,
        }
    }

    /// What the turn's grant to the frames tool names.
    pub fn scope(&self) -> SegmentingScope {
        SegmentingScope {
            meeting_id: self.capture.meeting_id,
            capture_id: self.capture.id,
            recording_name: self.recording_name.clone(),
            offset_ms: self.capture.offset_ms,
            duration_ms: Some(self.duration_ms),
            candidates: self.candidates.clone(),
        }
    }
}

/// A frame ready to pin: kept in storage, with the range it goes over.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub frame: StoredFrame,
    pub range: Range,
}

impl SegmentMeeting {
    /// `current`'s video for its segmenting to look at, or `None` when it has none to offer: no
    /// capture keeps a video, its frames have nowhere to be kept, or its recording has no picture
    /// `ffmpeg` can read. None of those fails the step; the meeting is segmented by its words.
    pub(super) async fn offer_video(
        &self,
        current: &Meeting,
        log: &StepLog,
    ) -> Result<Option<Video>, StepFailure> {
        let (captures, destination) = {
            let mut connection = self.connection().await?;
            let captures = meeting_capture::list_of(&mut connection, current.id)
                .await
                .map_err(StepFailure::retryable)?;
            let destination = user::workspace_settings(&mut connection)
                .await
                .map_err(StepFailure::retryable)?
                .page_attachment_storage_location_id;
            (captures, destination)
        };
        let Some(recording) = captures
            .into_iter()
            .filter_map(Recording::of)
            .find(|recording| recording.content_type().starts_with("video/"))
        else {
            return Ok(None);
        };
        if destination.is_none() {
            log.line("No storage location takes attachments, so no frames are pinned");
            return Ok(None);
        }
        let recording_name = self.local_recording(current, &recording).await?;
        let samples = match frames::sample(&self.inner.workspace, current.id, &recording_name).await
        {
            Ok(samples) => samples,
            Err(error) => {
                log.line(&format!(
                    "The video could not be sampled, so no frames are offered: {error}"
                ));
                return Ok(None);
            }
        };
        let candidates = frames::changes(&samples);
        let sample_count = samples.len() / (frames::SAMPLE_WIDTH * frames::SAMPLE_HEIGHT);
        // Samples come one an interval from the start, so the last one is within an interval of
        // the video's end.
        let duration_ms = i64::try_from(sample_count)
            .unwrap_or(i64::MAX)
            .saturating_add(1)
            * SAMPLE_INTERVAL_MS;
        log.line(&format!(
            "Sampled the video {sample_count} times; offered {} stills where it changed most",
            candidates.len()
        ));
        Ok(Some(Video {
            capture: recording.capture,
            recording_name,
            duration_ms,
            candidates,
        }))
    }

    /// Takes, keeps, and checks every frame `frames` names, for the transaction that pins them:
    /// each a still of its capture's recording, near duplicates of the meeting's frames and of
    /// each other dropped. A frame with no still to take, past the recording's end, is left out.
    pub(super) async fn prepare_frames(
        &self,
        current: &Meeting,
        frames: &[TakenFrame],
        log: &StepLog,
    ) -> Result<Vec<Prepared>, StepFailure> {
        let Some(first) = frames.first() else {
            return Ok(Vec::new());
        };
        let (capture, destination, mut kept) = {
            let mut connection = self.connection().await?;
            let capture = meeting_capture::find(&mut connection, first.capture_id)
                .await
                .map_err(StepFailure::retryable)?;
            let destination = user::workspace_settings(&mut connection)
                .await
                .map_err(StepFailure::retryable)?
                .page_attachment_storage_location_id;
            let kept = meeting_attachment::standing_fingerprints(&mut connection, current.id)
                .await
                .map_err(StepFailure::retryable)?;
            (capture, destination, kept)
        };
        let Some(destination) = destination else {
            log.line("No storage location takes attachments, so its frames are not pinned");
            return Ok(Vec::new());
        };
        let Some(recording) = Recording::of(capture) else {
            log.line("The video its frames were taken from keeps no recording now");
            return Ok(Vec::new());
        };
        if recording.capture.meeting_id != current.id {
            log.line("The video its frames were taken from is another meeting's now");
            return Ok(Vec::new());
        }
        let recording_name = self.local_recording(current, &recording).await?;
        let (location, access_key) = {
            let mut connection = self.connection().await?;
            crate::meetings::location(&mut connection, &self.inner.cipher, destination)
                .await
                .map_err(StepFailure::retryable)?
        };

        let mut prepared = Vec::new();
        let mut duplicates = 0;
        for taken in frames {
            let still = match frames::still(
                &self.inner.workspace,
                current.id,
                &recording_name,
                taken.capture_ms,
            )
            .await
            {
                Ok(still) => still,
                Err(error) => {
                    log.line(&format!(
                        "No still at {} ms of its capture, so that frame is left out: {error}",
                        taken.capture_ms
                    ));
                    continue;
                }
            };
            let frame = tokio::task::spawn_blocking(move || images::compress_frame(&still))
                .await
                .map_err(StepFailure::retryable)?
                .map_err(|error| {
                    StepFailure::retryable(format!("A still did not read: {error}"))
                })?;
            let is_duplicate = kept
                .iter()
                .any(|fingerprint| images::is_near_duplicate(fingerprint, &frame.fingerprint));
            if is_duplicate {
                duplicates += 1;
                continue;
            }
            kept.push(frame.fingerprint.clone());
            let destination = (&location, &access_key);
            let stored = self
                .keep_frame(destination, current, &recording.capture, taken, frame)
                .await?;
            prepared.push(Prepared {
                frame: stored,
                range: taken.range,
            });
        }
        if duplicates > 0 {
            log.line(&format!(
                "{duplicates} frames were near duplicates of ones the meeting holds, so they were \
                 left out"
            ));
        }
        Ok(prepared)
    }

    /// Keeps `frame`, `taken`'s still of `capture`, at its place in `destination`: named by its
    /// moment, so an attempt after this one keeps the same picture at the same place rather than
    /// leaving a copy no row names.
    async fn keep_frame(
        &self,
        (location, access_key): (&StorageLocation, &SecretString),
        current: &Meeting,
        capture: &MeetingCapture,
        taken: &TakenFrame,
        frame: images::Frame,
    ) -> Result<StoredFrame, StepFailure> {
        let path = meeting_attachments::frame_path(current.id, capture.id, taken.capture_ms);
        let size_bytes = i64::try_from(frame.webp.len()).map_err(StepFailure::permanent)?;
        let length = u64::try_from(size_bytes).map_err(StepFailure::permanent)?;
        let webp = bytes::Bytes::from(frame.webp);
        let body = stream::once(async move { Ok::<_, std::io::Error>(webp) });
        self.inner
            .storage
            .upload(
                location,
                access_key,
                &path,
                body,
                length,
                Some("image/webp"),
            )
            .await
            .map_err(StepFailure::retryable)?;
        Ok(StoredFrame {
            id: Uuid::now_v7(),
            meeting_id: current.id,
            storage_location_id: location.id,
            storage_path: path,
            file_name: meeting_attachments::frame_name(taken.capture_ms),
            size_bytes,
            capture_id: capture.id,
            capture_ms: taken.capture_ms,
            caption: taken.caption.clone(),
            fingerprint: frame.fingerprint,
        })
    }

    /// The name of `recording`'s copy in `current`'s workspace directory, fetched back from storage
    /// when the workspace lost it.
    async fn local_recording(
        &self,
        current: &Meeting,
        recording: &Recording,
    ) -> Result<String, StepFailure> {
        let (location, access_key) = {
            let mut connection = self.connection().await?;
            crate::meetings::location(
                &mut connection,
                &self.inner.cipher,
                recording.storage_location_id,
            )
            .await
            .map_err(StepFailure::retryable)?
        };
        let kept = media::Kept {
            storage: &self.inner.storage,
            location: &location,
            access_key: &access_key,
        };
        media::ensure_local(
            kept,
            &self.inner.workspace,
            (current.id, recording.capture.id),
            &recording.path,
        )
        .await
        .map_err(StepFailure::retryable)
    }
}

/// Pins every frame of `prepared` on `current` as Elysia, in the transaction that takes segmenting's
/// answer and under the meeting's lock it holds: each in a savepoint of its own, so a frame whose
/// words a person edited meanwhile, or one past the most a meeting holds, is left out and the rest
/// are pinned. Answers how many were pinned.
///
/// # Errors
/// Propagates any database error.
pub(super) async fn pin_frames(
    connection: &mut AsyncPgConnection,
    current: &Meeting,
    prepared: &[Prepared],
) -> diesel::QueryResult<usize> {
    let author = Author {
        user_id: ELYSIA_USER_ID,
        agent: None,
        at: Utc::now(),
    };
    let mut pinned = 0;
    for frame in prepared {
        let made = connection
            .transaction(async |connection| {
                attachments::frame_on(
                    connection,
                    current,
                    frame.frame.clone(),
                    Some(frame.range),
                    author,
                )
                .await
            })
            .await;
        match made {
            Ok(_) => pinned += 1,
            Err(
                AttachmentError::Database(error)
                | AttachmentError::Mark(MarkError::Database(error)),
            ) => return Err(error),
            Err(_left_out) => {}
        }
    }
    Ok(pinned)
}

#[cfg(test)]
mod tests {
    use diesel_async::SimpleAsyncConnection;
    use serde_json::json;

    use super::*;
    use crate::models::meeting;
    use crate::models::meeting_event::{self, EventKind};
    use crate::models::meeting_mark::{self, MarkKind};
    use crate::models::meeting_word::tests::{built_meeting, person, range, word};
    use crate::models::meeting_word::{self, Replacement};
    use crate::models::project::ProjectScope;
    use crate::models::storage_location::{
        self, BunnyStorageRegion, NewStorageLocation, StorageProvider,
    };
    use crate::storage::tests::PASSWORD;
    use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

    /// A still of the capture at `capture_ms`, kept in the location and ready to pin over `range`,
    /// its fingerprint every cell `shade`.
    fn prepared(
        current: &Meeting,
        (capture_id, location_id): (Uuid, Uuid),
        capture_ms: i64,
        range: Range,
        shade: u8,
    ) -> Prepared {
        Prepared {
            frame: StoredFrame {
                id: Uuid::now_v7(),
                meeting_id: current.id,
                storage_location_id: location_id,
                storage_path: meeting_attachments::frame_path(current.id, capture_id, capture_ms),
                file_name: meeting_attachments::frame_name(capture_ms),
                size_bytes: 1_024,
                capture_id,
                capture_ms,
                caption: format!("The slide at {capture_ms} ms"),
                fingerprint: vec![shade; 576],
            },
            range,
        }
    }

    /// #250: segmenting pins the frames it chose as Elysia, in the transaction that takes its
    /// answer, each in a savepoint of its own: one over words a person edited while the turn
    /// looked at its stills is left out, and the rest are pinned, recorded, and kept for the next
    /// segmenting to compare its stills against.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "a meeting with a video, its frames pinned, and what each left behind"
    )]
    async fn segmenting_pins_its_frames_as_elysia_leaving_out_one_over_words_edited_since() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(
            &mut connection,
            &[("Ada", "Here is the roadmap"), ("Grace", "Looks right")],
        )
        .await;
        let location = storage_location::create(
            &mut connection,
            &cipher(),
            &NewStorageLocation {
                created_by: TEST_PERSON_ID,
                name: "Files".to_owned(),
                provider: StorageProvider::Bunny {
                    zone: "files".to_owned(),
                    region: BunnyStorageRegion::Frankfurt,
                },
                path_prefix: String::new(),
                storage_limit_bytes: None,
                access_key: SecretString::from(PASSWORD),
                projects: ProjectScope::All,
            },
        )
        .await
        .expect("created");
        // Its capture keeps a video, which a frame is a still of.
        connection
            .batch_execute(&format!(
                "UPDATE meeting_captures SET storage_location_id = '{}', \
                 recording_path = 'meetings/{}/recording.mp4', \
                 recording_content_type = 'video/mp4' WHERE id = '{}'",
                location.id, capture.meeting_id, capture.id
            ))
            .await
            .expect("a recording");
        let current = meeting::find(&mut connection, capture.meeting_id)
            .await
            .expect("found");
        let roadmap = range(word(&words, "Here"), word(&words, "roadmap"));
        let agreed = range(word(&words, "Looks"), word(&words, "right"));
        let kept = (capture.id, location.id);
        let frames = [
            prepared(&current, kept, 1_000, roadmap, 0),
            prepared(&current, kept, 3_000, agreed, 255),
        ];
        meeting_word::edit(
            &mut connection,
            current.id,
            Replacement {
                range: agreed,
                text: "Looks good",
            },
            person(),
        )
        .await
        .expect("edits");

        let pinned = connection
            .transaction(async |connection| pin_frames(connection, &current, &frames).await)
            .await
            .expect("pins");

        assert_eq!(
            pinned, 1,
            "the frame over the words edited meanwhile is left out"
        );
        let attachments = meeting_attachment::of_meeting(&mut connection, current.id)
            .await
            .expect("reads");
        assert_eq!(
            attachments
                .iter()
                .map(|attachment| (attachment.id, attachment.created_by))
                .collect::<Vec<_>>(),
            vec![(frames[0].frame.id, ELYSIA_USER_ID)]
        );
        let pins: Vec<_> = meeting_mark::standing(&mut connection, current.id)
            .await
            .expect("reads")
            .into_iter()
            .filter(|mark| mark.kind == MarkKind::Pin.as_str())
            .collect();
        assert_eq!(pins.len(), 1);
        assert_eq!(
            (
                pins[0].start_word_id,
                pins[0].end_word_id,
                pins[0].created_by
            ),
            (roadmap.start_word_id, roadmap.end_word_id, ELYSIA_USER_ID)
        );
        assert_eq!(pins[0].data["attachmentId"], json!(frames[0].frame.id));
        let attached = meeting_event::of_kind(&mut connection, current.id, EventKind::Attached)
            .await
            .expect("reads");
        assert_eq!(
            attached
                .iter()
                .map(|entry| entry.created_by)
                .collect::<Vec<_>>(),
            vec![ELYSIA_USER_ID]
        );
        assert_eq!(
            meeting_attachment::standing_fingerprints(&mut connection, current.id)
                .await
                .expect("reads"),
            vec![vec![0_u8; 576]]
        );
    }
}
