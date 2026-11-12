// Copyright © 2026 Jalapeno Labs

//! Stills of a meeting's video for segmenting: where its screen changes most, and any moment it
//! asks for. See `docs/meetings.md`, Attachments and frames.
//!
//! # Where the screen changes
//!
//! A meeting's screen share changes when a slide turns or a window opens, and stays put between.
//! [`sample`] reads the video's key frames at most one every [`SAMPLE_INTERVAL_MS`], each shrunk to
//! a [`SAMPLE_WIDTH`] by [`SAMPLE_HEIGHT`] grey picture, in one `ffmpeg` pass in the workspace
//! container, where the recording already is for its clips (`super::media`). Decoding key frames
//! alone keeps a two hour recording to a few seconds of work. [`changes`] then measures how far
//! each sample's picture moved from the one before, and keeps the largest changes at least
//! [`MIN_GAP_MS`] apart, up to [`MAX_CANDIDATES`]: the stills segmenting is offered.
//!
//! # A still
//!
//! [`still`] takes the frame at one moment of the recording, scaled to at most [`STILL_WIDTH`]
//! wide, as a JPEG: what segmenting looks at through its frame tool (`crate::tools::frames`), and
//! what a frame it pins is kept from (`crate::images::compress_frame`).

use futures_util::TryStreamExt;
use uuid::Uuid;

use super::media::{self, MediaError};
use crate::runner::files::FileError;
use crate::runner::workspace::{AGENT_USER, SCRIPT_TIMEOUT, WorkspaceContainer};

/// How often the screen is sampled, at most: every five seconds, finer than slides turn, and a
/// two hour meeting is 1,440 samples.
pub const SAMPLE_INTERVAL_MS: i64 = 5_000;

/// A sample's picture: enough to see a slide turn or a window open, too coarse to read, so a
/// two hour meeting's samples are 3.3 MB.
pub const SAMPLE_WIDTH: usize = 64;
pub const SAMPLE_HEIGHT: usize = 36;

/// How far a sample must move from the one before, as a share of the brightest change there can
/// be, to be offered: a slide turning moves far more, video noise and a moving cursor far less.
pub const MIN_CHANGE: f32 = 0.04;

/// The least time between two stills offered, so a build of one slide, line by line, is offered
/// as its fullest moments rather than each.
pub const MIN_GAP_MS: i64 = 15_000;

/// The most stills segmenting is offered. Each one it looks at costs about 1,200 tokens at
/// [`STILL_WIDTH`], so it is offered the screen's largest changes and asks for any other moment.
pub const MAX_CANDIDATES: usize = 24;

/// How wide a still is, at most: a slide's words stay readable, and an image costs a model what
/// its pixels do.
pub const STILL_WIDTH: u32 = 1280;

/// Samples a recording's key frames as grey pictures, one after another, for [`changes`]. `$1` is
/// the recording, `$2` the samples, `$3` the samples a second, `$4` and `$5` their width and
/// height.
const SAMPLE_SCRIPT: &str = r#"
set -euo pipefail
ffmpeg -nostdin -hide_banner -loglevel error -y -skip_frame nokey -i "$1" -an \
  -vf "fps=$3,scale=$4:$5" -pix_fmt gray -f rawvideo "$2"
"#;

/// Takes one still. `$1` is the recording, `$2` the moment in seconds, `$3` the still, `$4` its
/// widest. `-ss` before the input seeks to the key frame before the moment and decodes forward to
/// it, so the still is exact and quick however long the recording.
const STILL_SCRIPT: &str = r#"
set -euo pipefail
ffmpeg -nostdin -hide_banner -loglevel error -y -ss "$2" -i "$1" -an -frames:v 1 \
  -vf "scale='min($4,iw)':-2" -q:v 3 "$3"
test -s "$3"
"#;

/// Removes one file of a meeting's directory. `$1` is the file.
const REMOVE_SCRIPT: &str = r#"rm -f -- "$1""#;

/// A still segmenting is offered: its name in the prompt, its moment on its capture's clock, and
/// how far the screen moved there.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// `f1`, `f2`, and on, in the order they were shown.
    pub id: String,
    pub capture_ms: i64,
    pub change: f32,
}

/// Samples `recording_name`, in meeting `meeting_id`'s workspace directory, for [`changes`]: its
/// key frames' grey pictures, one after another. The samples' file is removed once read.
///
/// # Errors
/// [`MediaError::Failed`] with `ffmpeg`'s words when the recording has no video to read, and
/// [`MediaError::Workspace`] when the workspace refuses.
pub async fn sample(
    workspace: &WorkspaceContainer,
    meeting_id: Uuid,
    recording_name: &str,
) -> Result<Vec<u8>, MediaError> {
    let directory = media::directory(meeting_id);
    let absolute = directory.absolute();
    let samples_name = format!("{recording_name}.samples");
    let rate = format!("1/{}", SAMPLE_INTERVAL_MS / 1000);
    let output = workspace
        .run_script(
            AGENT_USER,
            SAMPLE_SCRIPT,
            &[
                &format!("{absolute}/{recording_name}"),
                &format!("{absolute}/{samples_name}"),
                &rate,
                &SAMPLE_WIDTH.to_string(),
                &SAMPLE_HEIGHT.to_string(),
            ],
            Vec::new(),
            SCRIPT_TIMEOUT,
        )
        .await
        .map_err(FileError::from)?;
    let read = if output.exit_code == Some(0) {
        read_whole(workspace, &directory, &samples_name).await
    } else {
        Err(MediaError::Failed(format!(
            "ffmpeg could not sample the recording's video: {}",
            output.output.trim()
        )))
    };
    remove(workspace, &format!("{absolute}/{samples_name}")).await;
    read
}

/// The stills where `samples`, taken one every [`SAMPLE_INTERVAL_MS`] from the recording's start,
/// change most: each at least [`MIN_CHANGE`] from the sample before it, the largest first, at
/// least [`MIN_GAP_MS`] apart, at most [`MAX_CANDIDATES`], then in the order they were said, named
/// `f1` on. The first sample is offered when nothing changes before the gap, so a meeting that
/// opens on its slide has it.
pub fn changes(samples: &[u8]) -> Vec<Candidate> {
    let size = SAMPLE_WIDTH * SAMPLE_HEIGHT;
    let pictures: Vec<&[u8]> = samples.chunks_exact(size).collect();
    let mut moved: Vec<(i64, f32)> = Vec::with_capacity(pictures.len());
    for (index, pair) in pictures.windows(2).enumerate() {
        let change = difference(pair[0], pair[1]);
        let at = i64::try_from(index + 1).unwrap_or(i64::MAX) * SAMPLE_INTERVAL_MS;
        if change >= MIN_CHANGE {
            moved.push((at, change));
        }
    }
    moved.sort_by(|(left_at, left), (right_at, right)| {
        right.total_cmp(left).then(left_at.cmp(right_at))
    });
    let mut chosen: Vec<(i64, f32)> = Vec::new();
    for (at, change) in moved {
        if chosen.len() == MAX_CANDIDATES {
            break;
        }
        if chosen
            .iter()
            .all(|(kept_at, _)| (kept_at - at).abs() >= MIN_GAP_MS)
        {
            chosen.push((at, change));
        }
    }
    if !pictures.is_empty()
        && chosen.len() < MAX_CANDIDATES
        && chosen.iter().all(|(at, _)| *at >= MIN_GAP_MS)
    {
        chosen.push((0, 1.0));
    }
    chosen.sort_by_key(|(at, _)| *at);
    chosen
        .into_iter()
        .zip(1..)
        .map(|((capture_ms, change), number)| Candidate {
            id: format!("f{number}"),
            capture_ms,
            change,
        })
        .collect()
}

/// How far two grey pictures of one size differ: the mean of each pixel's change, as a share of
/// the largest a pixel can change.
fn difference(before: &[u8], after: &[u8]) -> f32 {
    let total: u64 = before
        .iter()
        .zip(after)
        .map(|(left, right)| u64::from(left.abs_diff(*right)))
        .sum();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a share of a few thousand pixels' change needs no more precision than f32 has"
    )]
    let share = total as f32 / (before.len().max(1) as f32 * 255.0);
    share
}

/// The still at `capture_ms` of `recording_name`, in meeting `meeting_id`'s workspace directory,
/// as a JPEG at most [`STILL_WIDTH`] wide. Its file is removed once read.
///
/// # Errors
/// [`MediaError::Failed`] with `ffmpeg`'s words when there is no frame there, such as past the
/// recording's end, and [`MediaError::Workspace`] when the workspace refuses.
pub async fn still(
    workspace: &WorkspaceContainer,
    meeting_id: Uuid,
    recording_name: &str,
    capture_ms: i64,
) -> Result<Vec<u8>, MediaError> {
    let directory = media::directory(meeting_id);
    let absolute = directory.absolute();
    let still_name = format!("still-{}.jpg", Uuid::now_v7().simple());
    let seconds = format!(
        "{}.{:03}",
        capture_ms.max(0) / 1000,
        capture_ms.max(0) % 1000
    );
    let output = workspace
        .run_script(
            AGENT_USER,
            STILL_SCRIPT,
            &[
                &format!("{absolute}/{recording_name}"),
                &seconds,
                &format!("{absolute}/{still_name}"),
                &STILL_WIDTH.to_string(),
            ],
            Vec::new(),
            SCRIPT_TIMEOUT,
        )
        .await
        .map_err(FileError::from)?;
    let read = if output.exit_code == Some(0) {
        read_whole(workspace, &directory, &still_name).await
    } else {
        Err(MediaError::Failed(format!(
            "ffmpeg found no frame there: {}",
            output.output.trim()
        )))
    };
    remove(workspace, &format!("{absolute}/{still_name}")).await;
    read
}

async fn read_whole(
    workspace: &WorkspaceContainer,
    directory: &crate::runner::WorkspacePath,
    name: &str,
) -> Result<Vec<u8>, MediaError> {
    let file = workspace.read_file(directory, name).await?;
    Ok(file
        .body
        .try_fold(Vec::new(), |mut read, chunk| async move {
            read.extend_from_slice(&chunk);
            Ok(read)
        })
        .await?)
}

/// Removes `path`, a scratch file, logging rather than failing when it cannot: what it held was
/// read already, and the meeting's directory goes once the meeting is done.
async fn remove(workspace: &WorkspaceContainer, path: &str) {
    let removed = workspace
        .run_script(
            AGENT_USER,
            REMOVE_SCRIPT,
            &[path],
            Vec::new(),
            SCRIPT_TIMEOUT,
        )
        .await;
    if !matches!(&removed, Ok(output) if output.exit_code == Some(0)) {
        tracing::event!(
            name: "meetings.frames.unremoved",
            tracing::Level::WARN,
            file.path = path,
            "a scratch file of a meeting's frames could not be removed",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample's picture, every pixel `shade`.
    fn picture(shade: u8) -> Vec<u8> {
        vec![shade; SAMPLE_WIDTH * SAMPLE_HEIGHT]
    }

    /// Samples one every [`SAMPLE_INTERVAL_MS`], each of `shades`.
    fn samples(shades: &[u8]) -> Vec<u8> {
        shades.iter().flat_map(|shade| picture(*shade)).collect()
    }

    #[test]
    fn a_slide_turning_is_offered_and_a_still_screen_is_not() {
        // Ten seconds on one slide, then a new slide that stays for half a minute, then a third.
        let mut shades = vec![200; 3];
        shades.extend([40; 7]);
        shades.extend([120; 4]);
        let offered = changes(&samples(&shades));

        let moments: Vec<i64> = offered
            .iter()
            .map(|candidate| candidate.capture_ms)
            .collect();
        assert_eq!(moments, [0, 15_000, 50_000]);
        assert_eq!(
            offered
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["f1", "f2", "f3"]
        );
    }

    #[test]
    fn noise_is_not_a_change_and_close_changes_keep_the_largest() {
        // A slide built line by line: two changes five seconds apart, the second the larger.
        let shades = [100, 100, 100, 100, 101, 130, 180, 180, 180, 180, 180];
        let offered = changes(&samples(&shades));

        assert_eq!(
            offered
                .iter()
                .map(|candidate| candidate.capture_ms)
                .collect::<Vec<_>>(),
            [0, 30_000],
            "the one-shade flicker is noise, and of the build only its fuller moment is offered"
        );
    }

    #[test]
    fn at_most_the_largest_changes_are_offered() {
        let shades: Vec<u8> = (0..200)
            .map(|index| if index % 4 == 0 { 250 } else { 0 })
            .collect();
        assert_eq!(changes(&samples(&shades)).len(), MAX_CANDIDATES);
        assert!(changes(&[]).is_empty());
    }
}
