// Copyright © 2026 Jalapeno Labs

//! Building a meeting's final transcript, its words with stable ids (`models::meeting_word`),
//! from its one capture. See `docs/meetings.md`, The final transcript.
//!
//! Until captures are aligned (#238), a meeting's final transcript is built from the one capture
//! it has, from the lines its correction followed: the source's, or Whisper's segments when the
//! source sent none (`correction::base`). When the meeting was corrected, each line is the
//! corrected copy's, and a line that differs from the one it follows is the correction's; when it
//! was not, each line is as its capture heard it.
//!
//! A line becomes its words, split at whitespace, its span of time spread evenly across them, on
//! the meeting's clock (the capture's own clock plus its offset), each word keeping the line's
//! speaker and where it came from. Aligning against Whisper's word timings replaces the even
//! spread (#238). The migration that split meetings into captures built every meeting's transcript
//! the same way, in SQL, and a test holds the two to the same words.
//!
//! A transcript is built once ([`ensure_built`] writes only for a meeting with no words), by the
//! correction step once its copy is stored, and by segmenting for a meeting whose correction was
//! skipped, so every meeting segmented has one.

use chrono::Utc;
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;

use super::correction::Base;
use super::source::TranscriptLine;
use crate::models::agent::Author;
use crate::models::meeting_capture::MeetingCapture;
use crate::models::meeting_correction::BasedOn;
use crate::models::meeting_event::MeetingEvent;
use crate::models::meeting_word::{self, Build, BuiltFrom, BuiltWord, Origin};
use crate::models::user::SYSTEM_USER_ID;

/// The words of `lines`, each line with where it came from, on the meeting's clock for a capture
/// `offset_ms` from it. A line's times before the capture's start read as its start, and an end
/// before its start as no time at all, so every word lies inside its line.
pub fn words_of<'line>(
    lines: impl IntoIterator<Item = (&'line TranscriptLine, Origin)>,
    offset_ms: i64,
) -> Vec<BuiltWord> {
    let mut words = Vec::new();
    for (line, origin) in lines {
        let texts: Vec<&str> = line.text.split_whitespace().collect();
        let Ok(count) = i64::try_from(texts.len()) else {
            continue;
        };
        let start_ms = line.start_ms.max(0);
        let end_ms = line.end_ms.max(start_ms);
        for (text, number) in texts.into_iter().zip(1_i64..) {
            words.push(BuiltWord {
                text: text.to_owned(),
                speaker: line.speaker.clone(),
                start_ms: offset_ms + start_ms + (end_ms - start_ms) * (number - 1) / count,
                end_ms: offset_ms + start_ms + (end_ms - start_ms) * number / count,
                origin,
            });
        }
    }
    words
}

/// What a transcript built from `heard` is built from, and each of its lines with where it came
/// from: the corrected copy's lines when there is one, a line that differs from the one it
/// follows being the correction's, or else `heard`'s own.
pub fn lines_to_build<'lines>(
    heard: &'lines Base,
    corrected: Option<&'lines [TranscriptLine]>,
) -> (BuiltFrom, Vec<(&'lines TranscriptLine, Origin)>) {
    let heard_from = match heard.based_on {
        BasedOn::Source => Origin::Source,
        BasedOn::Whisper => Origin::Whisper,
    };
    let Some(corrected) = corrected else {
        let built_from = match heard.based_on {
            BasedOn::Source => BuiltFrom::Source,
            BasedOn::Whisper => BuiltFrom::Whisper,
        };
        let lines = heard.lines.iter().map(|line| (line, heard_from)).collect();
        return (built_from, lines);
    };
    let lines = corrected
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let as_heard = heard
                .lines
                .get(index)
                .is_some_and(|heard_line| heard_line.text == line.text);
            let origin = if as_heard {
                heard_from
            } else {
                Origin::Correction
            };
            (line, origin)
        })
        .collect();
    (BuiltFrom::Correction, lines)
}

/// Builds meeting `capture.meeting_id`'s final transcript from `capture`, which heard `heard`,
/// and its corrected copy `corrected` when it has one, unless the meeting has a transcript
/// already. Built by the system, as the meeting's workflow is run. Answers the history entry, or
/// `None` when nothing was built: the meeting had a transcript, or nothing was said.
///
/// # Errors
/// Propagates any database error.
pub async fn ensure_built(
    connection: &mut AsyncPgConnection,
    capture: &MeetingCapture,
    heard: &Base,
    corrected: Option<&[TranscriptLine]>,
) -> QueryResult<Option<MeetingEvent>> {
    let (built_from, lines) = lines_to_build(heard, corrected);
    let words = words_of(lines, capture.offset_ms);
    let author = Author {
        user_id: SYSTEM_USER_ID,
        agent: None,
        at: Utc::now(),
    };
    let build = Build {
        meeting_id: capture.meeting_id,
        capture_id: capture.id,
        built_from,
        words,
    };
    meeting_word::insert_built(connection, build, author).await
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
            words_of([(&heard, Origin::Source)], 0),
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
        let words = words_of([(&heard, Origin::Whisper)], 90_000);
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
        assert!(words_of([(&silent, Origin::Source)], 0).is_empty());

        // A source that times a line backwards gives its words no time, and one before the
        // capture's start reads as its start.
        let backwards = line(5_000, 4_000, "Ada", "Yes no");
        let early = line(-300, 300, "Grace", "Hm");
        let words = words_of([(&backwards, Origin::Source), (&early, Origin::Source)], 0);
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
            words_of([(&uneven, Origin::Source)], 0)
                .iter()
                .map(|word| (word.start_ms, word.end_ms))
                .collect::<Vec<_>>(),
            vec![(0, 333), (333, 666), (666, 1_000)]
        );
    }

    #[test]
    fn a_corrected_line_is_the_corrections_and_the_rest_are_as_heard() {
        let heard = Base {
            based_on: BasedOn::Source,
            lines: vec![
                line(0, 1_000, "Ada", "Jon ships Friday"),
                line(1_000, 2_000, "Grace", "Agreed"),
            ],
        };
        let corrected = vec![
            line(0, 1_000, "Ada", "John ships Friday"),
            line(1_000, 2_000, "Grace", "Agreed"),
        ];

        let (built_from, lines) = lines_to_build(&heard, Some(&corrected));
        assert_eq!(built_from, BuiltFrom::Correction);
        assert_eq!(
            lines
                .iter()
                .map(|(line, origin)| (line.text.as_str(), *origin))
                .collect::<Vec<_>>(),
            vec![
                ("John ships Friday", Origin::Correction),
                ("Agreed", Origin::Source),
            ]
        );

        let whisper = Base {
            based_on: BasedOn::Whisper,
            lines: heard.lines.clone(),
        };
        let (built_from, lines) = lines_to_build(&whisper, None);
        assert_eq!(built_from, BuiltFrom::Whisper);
        assert!(
            lines
                .iter()
                .all(|(_line, origin)| *origin == Origin::Whisper)
        );
    }
}
