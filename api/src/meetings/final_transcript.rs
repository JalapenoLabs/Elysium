// Copyright © 2026 Jalapeno Labs

//! Lining a meeting's final transcript, its words with stable ids (`models::meeting_word`), up
//! from every capture of it. See `docs/meetings.md`, The final transcript.
//!
//! [`rebuild`] reads each capture on the meeting's clock (its own clock plus its offset), and the
//! lines its words come from: its source's, or Whisper's segments when its source sent none
//! (`correction::base`). Each line's words are timed by Whisper's words where Whisper heard them
//! and spread evenly where it did not, Whisper's words take their speakers from the sources'
//! lines, and the captures are laid out by time, a moment two of them heard reading once
//! (`super::alignment`).
//!
//! A transcript is lined up by the align step, and again by everything that moves a capture: a
//! hand-set offset, a merge, a split. Every one keeps what did not change, every edit, and every
//! range the meeting's correction settled (`meeting_word::rebuild`), so the transcript is settled
//! once (`meeting_word::settle`) and a rebuild never undoes it.
//!
//! A capture Whisper never heard has each line's time spread evenly across its words, exactly as
//! the migration that split meetings into captures wrote every uncorrected transcript in SQL, and
//! a test holds the two to the same words.

use anyhow::anyhow;
use diesel_async::AsyncPgConnection;

use super::alignment::{self, Speaking};
use super::correction;
use super::source::TranscriptLine;
use crate::models::agent::Author;
use crate::models::meeting;
use crate::models::meeting_capture::{self, MeetingCapture, OffsetSource};
use crate::models::meeting_correction::BasedOn;
use crate::models::meeting_event::MeetingEvent;
use crate::models::meeting_transcription;
use crate::models::meeting_word::{self, BuiltCapture, BuiltFrom, BuiltWord, Origin, Rebuild};
use crate::whisper::{WhisperTranscript, WhisperWord};

/// Lines meeting `meeting_id`'s final transcript up from every capture it has, as `author`, keeping
/// what did not change, every edit, and what its correction settled. Answers the `built` history
/// entry, or `None` when the transcript already read so.
///
/// # Errors
/// Propagates any database error, and a stored transcript that does not read as its shape.
pub async fn rebuild(
    connection: &mut AsyncPgConnection,
    meeting_id: uuid::Uuid,
    author: Author<'_>,
) -> anyhow::Result<Option<MeetingEvent>> {
    let captures = meeting_capture::list_of(connection, meeting_id).await?;

    let mut heard_by_capture: Vec<(uuid::Uuid, Vec<BuiltWord>)> =
        Vec::with_capacity(captures.len());
    let mut speaking: Vec<Speaking> = Vec::new();
    let mut built_from = BuiltFrom::Source;
    for (index, capture) in captures.iter().enumerate() {
        let (source, whisper) = heard_by(connection, capture).await?;
        speaking.extend(
            source
                .iter()
                .filter(|line| !line.speaker.trim().is_empty())
                .map(|line| Speaking {
                    start_ms: capture.offset_ms + line.start_ms.max(0),
                    end_ms: capture.offset_ms + line.end_ms.max(line.start_ms.max(0)),
                    speaker: line.speaker.clone(),
                }),
        );
        let whisper_words = words_of(whisper.as_ref());
        let Some(base) = correction::base(source, whisper.as_ref()) else {
            continue;
        };
        let (from, origin) = match base.based_on {
            BasedOn::Source => (BuiltFrom::Source, Origin::Source),
            BasedOn::Whisper => (BuiltFrom::Whisper, Origin::Whisper),
        };
        if index == 0 {
            built_from = from;
        }
        let lines = base.lines.iter().map(|line| (line, origin));
        let words = alignment::capture_words(lines, &whisper_words, capture.offset_ms);
        heard_by_capture.push((capture.id, words));
    }
    speaking.sort_by_key(|line| (line.start_ms, line.end_ms));
    for (_capture_id, words) in &mut heard_by_capture {
        alignment::name_speakers(words, &speaking);
    }

    let built = Rebuild {
        meeting_id,
        words: alignment::line_up(heard_by_capture),
        captures: captures
            .iter()
            .map(|capture| BuiltCapture {
                capture_id: capture.id,
                offset_ms: capture.offset_ms,
            })
            .collect(),
        built_from,
        provisional: captures
            .iter()
            .skip(1)
            .filter(|capture| capture.offset_source() == OffsetSource::Listed)
            .map(|capture| capture.id)
            .collect(),
    };
    let rebuilt = meeting_word::rebuild(connection, built, author).await?;
    // Lining the words up again can move them, and the comment threads anchored to them, under
    // the meeting's excerpts, which read their words and index the threads over them.
    if rebuilt.is_some() {
        let current = meeting::find(connection, meeting_id).await?;
        super::excerpts::refresh(connection, &current, None).await?;
    }
    Ok(rebuilt)
}

/// What `capture` heard: its source's lines, and Whisper's transcript when it has one.
///
/// # Errors
/// Propagates any database error, and a stored transcript that does not read as its shape.
pub async fn heard_by(
    connection: &mut AsyncPgConnection,
    capture: &MeetingCapture,
) -> anyhow::Result<(Vec<TranscriptLine>, Option<WhisperTranscript>)> {
    let source: Vec<TranscriptLine> = serde_json::from_value(capture.transcript.clone())
        .map_err(|error| anyhow!("a capture's transcript is not in the stored shape: {error}"))?;
    let whisper = meeting_transcription::find(connection, capture.id)
        .await?
        .map(|stored| serde_json::from_value::<WhisperTranscript>(stored.transcript))
        .transpose()
        .map_err(|error| {
            anyhow!("a capture's Whisper transcript is not in the stored shape: {error}")
        })?;
    Ok((source, whisper))
}

/// Every word Whisper heard in `whisper`, in the order it heard them.
pub fn words_of(whisper: Option<&WhisperTranscript>) -> Vec<WhisperWord> {
    whisper
        .into_iter()
        .flat_map(|heard| &heard.segments)
        .flat_map(|segment| segment.words.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start_ms: i64, end_ms: i64, speaker: &str, text: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms,
            end_ms,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        }
    }

    fn word(text: &str, speaker: &str, start_ms: i64, end_ms: i64, origin: Origin) -> BuiltWord {
        BuiltWord {
            text: text.to_owned(),
            speaker: speaker.to_owned(),
            start_ms,
            end_ms,
            origin,
        }
    }

    #[test]
    fn a_line_becomes_its_words_with_its_time_spread_evenly_across_them() {
        let heard = line(1_000, 4_000, "Ada", "  Launch is\tFriday. ");
        assert_eq!(
            alignment::capture_words([(&heard, Origin::Source)], &[], 0),
            vec![
                word("Launch", "Ada", 1_000, 2_000, Origin::Source),
                word("is", "Ada", 2_000, 3_000, Origin::Source),
                word("Friday.", "Ada", 3_000, 4_000, Origin::Source),
            ]
        );
    }

    #[test]
    fn a_captures_offset_places_its_words_on_the_meetings_clock() {
        let heard = line(0, 1_000, "Ada", "Hello there");
        let words = alignment::capture_words([(&heard, Origin::Whisper)], &[], 90_000);
        assert_eq!(
            words
                .iter()
                .map(|word| (word.start_ms, word.end_ms))
                .collect::<Vec<_>>(),
            vec![(90_000, 90_500), (90_500, 91_000)]
        );
    }

    #[test]
    fn a_line_with_nothing_said_or_odd_times_never_leaves_its_span() {
        let silent = line(0, 1_000, "Ada", "   ");
        assert!(alignment::capture_words([(&silent, Origin::Source)], &[], 0).is_empty());

        // A source that times a line backwards gives its words no time, and one before the
        // capture's start reads as its start.
        let backwards = line(5_000, 4_000, "Ada", "Yes no");
        let early = line(-300, 300, "Grace", "Hm");
        let words = alignment::capture_words(
            [(&backwards, Origin::Source), (&early, Origin::Source)],
            &[],
            0,
        );
        assert_eq!(
            words
                .iter()
                .map(|word| (word.start_ms, word.end_ms))
                .collect::<Vec<_>>(),
            vec![(5_000, 5_000), (5_000, 5_000), (0, 300)]
        );
        // Integer spreading truncates like Postgres does, so the migration's words match.
        let uneven = line(0, 1_000, "Ada", "a b c");
        assert_eq!(
            alignment::capture_words([(&uneven, Origin::Source)], &[], 0)
                .iter()
                .map(|word| (word.start_ms, word.end_ms))
                .collect::<Vec<_>>(),
            vec![(0, 333), (333, 666), (666, 1_000)]
        );
    }
}
