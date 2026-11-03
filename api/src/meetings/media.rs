// Copyright © 2026 Jalapeno Labs

//! Moving a meeting's recording between its source, the workspace, and storage, and cutting
//! its segments' clips with `ffmpeg`.
//!
//! The workspace container is where media work happens: it has `ffmpeg`, and the API's own
//! image stays small. Each meeting gets a directory there, `.elysium/meetings/<id>`, which
//! holds the recording while its clips are cut and is removed once the meeting is done.
//! Storage, not the workspace, is where a recording and its clips are kept: the workspace
//! copy is a scratch copy, fetched back from storage when it is missing.
//!
//! Every byte streams; nothing is held in memory whole. A clip is cut with `-c copy`, which
//! is fast and lossless but starts at the keyframe nearest the segment's start, a second or
//! two early at most. That is the trade for not re-encoding an hour of video per meeting.

use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use super::source::{RecordingKind, RecordingSource};
use crate::models::storage_location::StorageLocation;
use crate::runner::WorkspacePath;
use crate::runner::files::FileError;
use crate::runner::workspace::{AGENT_USER, SCRIPT_TIMEOUT, WorkspaceContainer};
use crate::storage::Storage;
use crate::storage::transfer::{body_of, send_for_body};

/// The name a provider error carries when the recording's download fails.
const SOURCE_PROVIDER: &str = "the meeting source";

/// Media types Elysium knows the file extension of, so `ffmpeg` picks the same container
/// for a clip as its recording and a browser plays it.
const EXTENSION_BY_TYPE: [(&str, &str); 10] = [
    ("video/mp4", "mp4"),
    ("video/webm", "webm"),
    ("video/quicktime", "mov"),
    ("audio/mpeg", "mp3"),
    ("audio/mp4", "m4a"),
    ("audio/x-m4a", "m4a"),
    ("audio/wav", "wav"),
    ("audio/x-wav", "wav"),
    ("audio/webm", "webm"),
    ("audio/ogg", "ogg"),
];

/// Cuts one clip. `$1` is the recording, `$2` the start in seconds, `$3` the length in
/// seconds, `$4` the clip. Prints the clip's size.
///
/// MP4-family clips get `+faststart`, which moves their index to the front, so a browser
/// plays one as it streams instead of waiting for the whole file.
const CUT_SCRIPT: &str = r#"
set -euo pipefail
container_flags=()
case "$4" in *.mp4|*.m4a|*.mov) container_flags=(-movflags +faststart) ;; esac
ffmpeg -nostdin -hide_banner -loglevel error -y \
  -ss "$2" -i "$1" -t "$3" -map 0 -c copy -avoid_negative_ts make_zero \
  "${container_flags[@]}" "$4"
stat -c %s -- "$4"
"#;

/// Why moving or cutting media failed.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error(transparent)]
    Storage(#[from] crate::storage::StorageError),
    #[error(transparent)]
    Workspace(#[from] FileError),
    #[error("{0}")]
    Failed(String),
}

/// A recording in the workspace: its file name in the meeting's directory, and its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRecording {
    pub file_name: String,
    pub content_type: String,
}

/// The workspace directory a meeting's media is worked on in.
pub fn directory(meeting_id: Uuid) -> WorkspacePath {
    WorkspacePath::new(format!(".elysium/meetings/{meeting_id}"))
        .expect("a meeting's directory is a plain relative path")
}

/// Where a meeting's recording is kept in its storage location.
pub fn recording_path(meeting_id: Uuid, extension: &str) -> String {
    format!("meetings/{meeting_id}/recording.{extension}")
}

/// Where a segment's clip is kept in its storage location.
pub fn clip_path(meeting_id: Uuid, index: usize, extension: &str) -> String {
    format!("meetings/{meeting_id}/segments/{index}.{extension}")
}

/// The file extension for `content_type`, or the usual one for `kind` when the type is
/// missing or unknown. Answers the media type that goes with the extension, too.
pub fn extension_for(
    content_type: Option<&str>,
    kind: RecordingKind,
) -> (&'static str, &'static str) {
    let essence = content_type
        .and_then(|content_type| content_type.split(';').next())
        .map(|essence| essence.trim().to_ascii_lowercase());
    if let Some(essence) = essence
        && let Some((known_type, extension)) = EXTENSION_BY_TYPE
            .iter()
            .find(|(known_type, _)| *known_type == essence)
    {
        return (extension, known_type);
    }
    match kind {
        RecordingKind::Video => ("mp4", "video/mp4"),
        RecordingKind::Audio => ("mp3", "audio/mpeg"),
    }
}

/// The extension of a stored path, such as `mp4` for `meetings/x/recording.mp4`.
pub fn extension_of(path: &str) -> &str {
    path.rsplit_once('.').map_or("", |(_, extension)| extension)
}

/// Downloads a recording from its source into the meeting's workspace directory.
///
/// # Errors
/// [`MediaError`] when the source refuses or stalls, or the workspace cannot write it.
pub async fn download(
    http: &reqwest::Client,
    workspace: &WorkspaceContainer,
    meeting_id: Uuid,
    source: &RecordingSource,
) -> Result<LocalRecording, MediaError> {
    let response = send_for_body(http.get(source.url.expose_secret()), SOURCE_PROVIDER).await?;
    let status = response.status();
    if !status.is_success() {
        return Err(MediaError::Failed(format!(
            "the meeting source answered {status} when its recording was downloaded"
        )));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let (extension, content_type) = extension_for(content_type.as_deref(), source.kind);
    let file_name = format!("recording.{extension}");
    workspace
        .write_file(
            &directory(meeting_id),
            &file_name,
            body_of(response, SOURCE_PROVIDER),
        )
        .await?;
    Ok(LocalRecording {
        file_name,
        content_type: content_type.to_owned(),
    })
}

/// The storage location a meeting's media is kept in, with the key that opens it.
#[derive(Debug, Clone, Copy)]
pub struct Kept<'location> {
    pub storage: &'location Storage,
    pub location: &'location StorageLocation,
    pub access_key: &'location SecretString,
}

/// Uploads `file_name` from the meeting's workspace directory to `path` where `kept` says.
///
/// # Errors
/// [`MediaError`] when the file cannot be read or the provider refuses it.
pub async fn upload(
    kept: Kept<'_>,
    workspace: &WorkspaceContainer,
    meeting_id: Uuid,
    file_name: &str,
    path: &str,
    content_type: &str,
) -> Result<(), MediaError> {
    let file = workspace
        .read_file(&directory(meeting_id), file_name)
        .await?;
    kept.storage
        .upload(
            kept.location,
            kept.access_key,
            path,
            file.body,
            file.content_length,
            Some(content_type),
        )
        .await?;
    Ok(())
}

/// Makes sure the meeting's workspace directory holds its recording, fetching it back from
/// storage when the workspace lost it, and answers its file name there.
///
/// # Errors
/// [`MediaError`] when the provider or the workspace refuses.
pub async fn ensure_local(
    kept: Kept<'_>,
    workspace: &WorkspaceContainer,
    meeting_id: Uuid,
    stored_path: &str,
) -> Result<String, MediaError> {
    let file_name = format!("recording.{}", extension_of(stored_path));
    let present = workspace
        .list_files(&directory(meeting_id), "")
        .await?
        .iter()
        .any(|listed| listed.path == file_name);
    if present {
        return Ok(file_name);
    }
    let download = kept
        .storage
        .download(kept.location, kept.access_key, stored_path)
        .await?;
    workspace
        .write_file(&directory(meeting_id), &file_name, download.body)
        .await?;
    Ok(file_name)
}

/// Cuts `[start_ms, end_ms)` of the recording into `clip_name`, in the meeting's workspace
/// directory.
///
/// # Errors
/// [`MediaError::Failed`] with `ffmpeg`'s own words when the cut fails.
pub async fn cut(
    workspace: &WorkspaceContainer,
    meeting_id: Uuid,
    recording_name: &str,
    start_ms: i64,
    end_ms: i64,
    clip_name: &str,
) -> Result<(), MediaError> {
    let directory = directory(meeting_id).absolute();
    let recording = format!("{directory}/{recording_name}");
    let clip = format!("{directory}/{clip_name}");
    let start = seconds(start_ms);
    let length = seconds(end_ms - start_ms);
    let output = workspace
        .run_script(
            AGENT_USER,
            CUT_SCRIPT,
            &[&recording, &start, &length, &clip],
            Vec::new(),
            SCRIPT_TIMEOUT,
        )
        .await
        .map_err(FileError::from)?;
    if output.exit_code != Some(0) {
        return Err(MediaError::Failed(format!(
            "ffmpeg could not cut a clip: {}",
            output.output.trim()
        )));
    }
    Ok(())
}

/// Removes the meeting's workspace directory and everything in it.
///
/// # Errors
/// [`MediaError`] when the workspace refuses.
pub async fn clean_up(workspace: &WorkspaceContainer, meeting_id: Uuid) -> Result<(), MediaError> {
    workspace.remove_directory(&directory(meeting_id)).await?;
    Ok(())
}

/// Milliseconds as the decimal seconds `ffmpeg` takes.
fn seconds(milliseconds: i64) -> String {
    let milliseconds = milliseconds.max(0);
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types_keep_their_extension_and_unknown_ones_follow_the_kind() {
        assert_eq!(
            extension_for(Some("audio/mpeg"), RecordingKind::Audio),
            ("mp3", "audio/mpeg")
        );
        assert_eq!(
            extension_for(Some("Video/MP4; codecs=avc1"), RecordingKind::Audio),
            ("mp4", "video/mp4")
        );
        assert_eq!(
            extension_for(Some("application/octet-stream"), RecordingKind::Video),
            ("mp4", "video/mp4")
        );
        assert_eq!(
            extension_for(None, RecordingKind::Audio),
            ("mp3", "audio/mpeg")
        );
    }

    #[test]
    fn paths_name_the_meeting_and_the_segment() {
        let id = Uuid::nil();
        assert_eq!(
            recording_path(id, "mp4"),
            "meetings/00000000-0000-0000-0000-000000000000/recording.mp4"
        );
        assert_eq!(
            clip_path(id, 3, "mp3"),
            "meetings/00000000-0000-0000-0000-000000000000/segments/3.mp3"
        );
        assert_eq!(extension_of("meetings/x/recording.m4a"), "m4a");
        assert_eq!(
            directory(id).absolute(),
            "/workspace/.elysium/meetings/00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn seconds_keep_their_milliseconds() {
        assert_eq!(seconds(0), "0.000");
        assert_eq!(seconds(65_432), "65.432");
        assert_eq!(seconds(-5), "0.000");
    }
}
