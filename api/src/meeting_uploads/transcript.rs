// Copyright © 2026 Jalapeno Labs

//! Reading an uploaded transcript into lines said, as every meeting source's transcript is
//! stored (`crate::meetings::source::TranscriptLine`).
//!
//! The file is the person's to choose, so it is read defensively, never trusted:
//!
//! - **Text.** UTF-8, with or without its byte order mark, or UTF-16 with one, as Windows tools
//!   save it. Bytes that are not text in any of those, a NUL among them, refuse it.
//! - **WebVTT and SRT** are cues: each cue's times, then what was said. A WebVTT voice tag
//!   (`<v Ada Lovelace>`) names the speaker, as Teams writes them; otherwise a leading
//!   `Speaker:` does, as Zoom writes them. Every other tag and the entities WebVTT allows are read
//!   past, and comments, styles, and regions are skipped.
//! - **Plain text and Markdown** are one line said per line of the file. `[00:01:23]`, `(1:23)`,
//!   or a bare `00:01:23` at a line's start times it, and `Speaker:` names who said it. A line
//!   with no speaker of its own was said by the last one named, so a speaker's hard-wrapped
//!   paragraph stays theirs. A line that is only a speaker, a time, or both (`Ada Lovelace
//!   (00:01:23)`) heads the lines that follow, as many exports lay a transcript out. Markdown's
//!   headings, lists, quotes, and emphasis are read past; a heading that is not such a line is a
//!   title, not something said.
//!
//! A line with no time of its own starts when the line before it did. A transcript with no time
//! anywhere is a meeting in words alone: every line is at `0`, which is how the steps after know
//! to read it by its lines rather than its clock.

use std::sync::LazyLock;

use regex::Regex;

use super::TranscriptFormat;
use crate::meetings::source::TranscriptLine;

/// The most lines a transcript may hold: a day of speech at a line every second.
pub const MAX_LINES: usize = 100_000;

/// The longest a speaker's name may be, in characters, before a line's start is not read as one.
const MAX_SPEAKER_CHARACTERS: usize = 64;

/// The most words a speaker's name may have. More is a sentence with a colon in it.
const MAX_SPEAKER_WORDS: usize = 6;

/// Why a transcript could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranscriptError {
    #[error("this file is not text")]
    NotText,
    #[error("this transcript holds nothing that was said")]
    Empty,
    #[error("this transcript holds more than {MAX_LINES} lines")]
    TooLong,
}

/// A time as transcripts write it: `1:23`, `01:23.456`, or `1:02:03,456`.
const TIME: &str = r"(?:\d{1,3}:)?\d{1,2}:\d{2}(?:[.,]\d{1,3})?";

/// A cue's timing line: its start, `-->`, its end, and any settings after.
static CUE_TIMING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^\s*({TIME})\s*-->\s*({TIME})")).expect("a static pattern")
});

/// A time leading a line of text, bracketed, in parentheses, or bare before a space.
static LEADING_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^\s*(?:\[({TIME})\]|\(({TIME})\)|({TIME})(?:\s*[-|]\s+|\s+|$))\s*"
    ))
    .expect("a static pattern")
});

/// A line that is a speaker followed by when they spoke: `Name (t)`, `Name [t]`, or `Name - t`.
static SPEAKER_THEN_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(.+?)\s*(?:\(({TIME})\)|\[({TIME})\]|-\s+({TIME}))\s*:?\s*$"
    ))
    .expect("a static pattern")
});

/// A WebVTT voice tag's speaker: `<v Name>` or `<v.class Name>`.
static VOICE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<v(?:\.[^\s>]*)?\s+([^>]+)>").expect("a static pattern"));

/// Any tag, and SRT's `{\an8}` style overrides.
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]*>|\{\\[^}]*\}").expect("a static pattern"));

/// A Markdown line's block marker: a heading, a quote, or a list item.
static BLOCK_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:#{1,6}\s+|>\s*|[-*+]\s+|\d{1,3}[.)]\s+)").expect("a static pattern")
});

/// Reads `bytes`, a transcript laid out as `format`, into the lines said in it, in order.
///
/// # Errors
/// [`TranscriptError`] when it is not text, holds nothing said, or holds too many lines.
pub fn read(
    bytes: &[u8],
    format: TranscriptFormat,
) -> Result<Vec<TranscriptLine>, TranscriptError> {
    let text = decode(bytes)?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines = match format {
        TranscriptFormat::WebVtt | TranscriptFormat::Srt => read_cues(&text),
        TranscriptFormat::Text => read_text(&text, false),
        TranscriptFormat::Markdown => read_text(&text, true),
    };
    if lines.len() > MAX_LINES {
        return Err(TranscriptError::TooLong);
    }
    if lines.is_empty() {
        return Err(TranscriptError::Empty);
    }
    Ok(lines)
}

/// Whether `lines` carry no time at all: a meeting in words alone.
pub fn is_untimed(lines: &[TranscriptLine]) -> bool {
    lines
        .iter()
        .all(|line| line.start_ms == 0 && line.end_ms == 0)
}

/// `bytes` as text: UTF-8 with or without its byte order mark, or UTF-16 with one.
fn decode(bytes: &[u8]) -> Result<String, TranscriptError> {
    let text = if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        std::str::from_utf8(rest)
            .map_err(|_invalid| TranscriptError::NotText)?
            .to_owned()
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        decode_utf16(rest, u16::from_le_bytes)?
    } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        decode_utf16(rest, u16::from_be_bytes)?
    } else {
        std::str::from_utf8(bytes)
            .map_err(|_invalid| TranscriptError::NotText)?
            .to_owned()
    };
    if text.contains('\0') {
        return Err(TranscriptError::NotText);
    }
    Ok(text)
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Result<String, TranscriptError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(TranscriptError::NotText);
    }
    let units = bytes.as_chunks::<2>().0.iter().map(|pair| unit(*pair));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_invalid| TranscriptError::NotText)
}

/// WebVTT's or SRT's cues, each a line: blocks apart by blank lines, each holding one timing line
/// and what was said after it. A block with no timing line (WebVTT's header, comments, styles,
/// regions, or SRT's stray numbers) holds nothing said.
fn read_cues(text: &str) -> Vec<TranscriptLine> {
    let mut lines = Vec::new();
    for block in text.split("\n\n") {
        let mut rows = block.lines().skip_while(|row| !CUE_TIMING.is_match(row));
        let Some(timing) = rows.next().and_then(|row| CUE_TIMING.captures(row)) else {
            continue;
        };
        let (Some(start_ms), Some(end_ms)) = (milliseconds(&timing[1]), milliseconds(&timing[2]))
        else {
            continue;
        };
        let payload = rows.collect::<Vec<_>>().join(" ");
        let voice = VOICE.captures(&payload).map(|voice| plain(&voice[1]));
        let said = plain(&TAG.replace_all(&payload, ""));
        let (speaker, said) = match voice {
            Some(voice) => (voice, said),
            None => match split_speaker(&said) {
                Some((speaker, rest)) => (speaker.to_owned(), rest.to_owned()),
                None => (String::new(), said),
            },
        };
        if said.is_empty() {
            continue;
        }
        lines.push(TranscriptLine {
            start_ms,
            end_ms: end_ms.max(start_ms),
            speaker,
            text: said,
        });
    }
    lines
}

/// A line of text before its times are filled in.
struct Said {
    start_ms: Option<i64>,
    speaker: String,
    text: String,
}

/// Plain text's or Markdown's lines, each said once (the module's docs).
fn read_text(text: &str, is_markdown: bool) -> Vec<TranscriptLine> {
    let mut said: Vec<Said> = Vec::new();
    let mut speaker = String::new();
    // A time a heading line gave, for the next line said.
    let mut heading_time: Option<i64> = None;
    for row in text.lines() {
        let (row, is_heading) = if is_markdown {
            markdown_text(row)
        } else {
            (row.trim().to_owned(), false)
        };
        if row.is_empty() {
            continue;
        }

        // `Ada Lovelace (00:01:23)`: a heading for the lines after it.
        if let Some(head) = SPEAKER_THEN_TIME.captures(&row)
            && let Some(name) = speaker_name(&head[1])
        {
            let time = head.get(2).or_else(|| head.get(3)).or_else(|| head.get(4));
            name.clone_into(&mut speaker);
            heading_time = time.and_then(|time| milliseconds(time.as_str()));
            continue;
        }

        let (time, rest) = match LEADING_TIME.captures(&row) {
            Some(leading) => {
                let time = leading
                    .get(1)
                    .or_else(|| leading.get(2))
                    .or_else(|| leading.get(3))
                    .and_then(|time| milliseconds(time.as_str()));
                (time, &row[leading.get(0).map_or(0, |whole| whole.end())..])
            }
            None => (None, row.as_str()),
        };
        let rest = rest.trim();

        // `Ada:` alone, or `[00:01:23] Ada:`: a heading too.
        if let Some(name) = rest.strip_suffix(':').and_then(speaker_name) {
            name.clone_into(&mut speaker);
            heading_time = time.or(heading_time);
            continue;
        }
        if rest.is_empty() {
            // A time alone, for the line said next.
            heading_time = time.or(heading_time);
            continue;
        }
        if is_heading {
            // A heading that names nobody is a title, not something said.
            continue;
        }

        let text = match split_speaker(rest) {
            Some((name, text)) => {
                name.clone_into(&mut speaker);
                text
            }
            None => rest,
        };
        if text.is_empty() {
            continue;
        }
        said.push(Said {
            start_ms: time.or(heading_time.take()),
            speaker: speaker.clone(),
            text: text.to_owned(),
        });
    }
    timed(said)
}

/// `row` as Markdown means it, as text: its block marker and emphasis read past, and whether it was
/// a heading.
fn markdown_text(row: &str) -> (String, bool) {
    let trimmed = row.trim();
    // A rule or a table's separator holds nothing said.
    if trimmed
        .chars()
        .all(|character| "-*_=|: ".contains(character))
    {
        return (String::new(), false);
    }
    let is_heading = trimmed.starts_with('#');
    let unmarked = BLOCK_MARKER.replace(trimmed, "");
    let plain_text = unmarked
        .replace("**", "")
        .replace("__", "")
        .replace('`', "");
    (plain_text.trim().to_owned(), is_heading)
}

/// Fills each line's times: one with no time of its own starts when the line before it did, and
/// each ends when a later line starts later. With no time anywhere, every line is at `0`.
fn timed(said: Vec<Said>) -> Vec<TranscriptLine> {
    let starts: Vec<i64> = said
        .iter()
        .scan(0, |previous, line| {
            *previous = line.start_ms.unwrap_or(*previous);
            Some(*previous)
        })
        .collect();
    // From the last line back, so lines sharing a start share the end of the run they make, in one
    // pass however many there are. A start that goes back, which only a broken file has, ends
    // where it starts.
    let mut ends = starts.clone();
    for index in (0..starts.len().saturating_sub(1)).rev() {
        let next = starts[index + 1];
        if next > starts[index] {
            ends[index] = next;
        } else if next == starts[index] {
            ends[index] = ends[index + 1];
        }
    }
    said.into_iter()
        .zip(starts.into_iter().zip(ends))
        .map(|(line, (start_ms, end_ms))| TranscriptLine {
            start_ms,
            end_ms,
            speaker: line.speaker,
            text: line.text,
        })
        .collect()
}

/// `Speaker: what they said` as its speaker and what they said, when the line opens with a name.
fn split_speaker(line: &str) -> Option<(&str, &str)> {
    let (label, text) = line.split_once(": ")?;
    let name = speaker_name(label)?;
    Some((name, text.trim()))
}

/// `label` as a speaker's name, when it reads as one: from a letter, short, without a sentence's
/// punctuation or an address, and with any emphasis Markdown left around it read past.
fn speaker_name(label: &str) -> Option<&str> {
    let name = label.trim().trim_matches(['*', '_']).trim();
    let starts_with_letter = name.chars().next().is_some_and(char::is_alphabetic);
    let is_short = name.chars().count() <= MAX_SPEAKER_CHARACTERS
        && name.split_whitespace().count() <= MAX_SPEAKER_WORDS;
    let reads_as_a_name = !name.contains(['.', '?', '!', ':', '[', ']', '(', ')']);
    (starts_with_letter && is_short && reads_as_a_name).then_some(name)
}

/// What a cue's or a voice tag's text says once WebVTT's entities are read and its spacing folded.
fn plain(text: &str) -> String {
    let decoded = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&lrm;", "")
        .replace("&rlm;", "")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `1:23`, `01:23.456`, or `1:02:03,456` in milliseconds.
fn milliseconds(time: &str) -> Option<i64> {
    let (clock, fraction) = match time.split_once(['.', ',']) {
        Some((clock, fraction)) => (clock, fraction),
        None => (time, ""),
    };
    let mut seconds: i64 = 0;
    for part in clock.split(':') {
        seconds = seconds.checked_mul(60)?.checked_add(part.parse().ok()?)?;
    }
    // A fraction of `5` is half a second, as `.5` reads; pad it to milliseconds.
    let millis: i64 = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<3}").parse().ok()?
    };
    seconds.checked_mul(1000)?.checked_add(millis)
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

    #[test]
    fn a_teams_webvtt_names_its_speakers_by_their_voice_tags() {
        let vtt = "WEBVTT\n\
            \n\
            NOTE written by Teams\n\
            \n\
            0f6a1b2c-1/12-0\n\
            00:00:01.250 --> 00:00:04.000\n\
            <v Ada Lovelace>Launch is on Friday,</v>\n\
            \n\
            0f6a1b2c-1/13-0\n\
            00:00:04.000 --> 00:00:06.5\n\
            <v.loud Grace Hopper>Agreed &amp; <i>signed</i> off.</v>\n";

        assert_eq!(
            read(vtt.as_bytes(), TranscriptFormat::WebVtt).expect("reads"),
            vec![
                line(1_250, 4_000, "Ada Lovelace", "Launch is on Friday,"),
                line(4_000, 6_500, "Grace Hopper", "Agreed & signed off."),
            ]
        );
    }

    #[test]
    fn a_zoom_webvtt_names_its_speakers_before_a_colon() {
        let vtt = "WEBVTT\r\n\r\n1\r\n00:01.000 --> 00:03.000\r\nAda Lovelace: Hello there\r\n\
            and welcome.\r\n\r\n2\r\n00:03.000 --> 00:05.000\r\nJust a note\r\n";

        assert_eq!(
            read(vtt.as_bytes(), TranscriptFormat::WebVtt).expect("reads"),
            vec![
                line(1_000, 3_000, "Ada Lovelace", "Hello there and welcome."),
                line(3_000, 5_000, "", "Just a note"),
            ]
        );
    }

    #[test]
    fn an_srt_reads_its_cues_and_drops_its_styles() {
        let srt = "1\n00:00:01,000 --> 00:00:02,500\n{\\an8}<i>Grace:</i> Ship it.\n\n\
            2\n01:02:03,040 --> 01:02:04,000\nOn Friday.\n\n3\nno timing here\n";

        assert_eq!(
            read(srt.as_bytes(), TranscriptFormat::Srt).expect("reads"),
            vec![
                line(1_000, 2_500, "Grace", "Ship it."),
                line(3_723_040, 3_724_000, "", "On Friday."),
            ]
        );
    }

    #[test]
    fn plain_text_reads_its_times_and_speakers_when_they_are_there() {
        let text = "[00:00:05] Ada: Launch is Friday.\n\
            I'll send the notes after.\n\
            \n\
            (1:30) Grace: Agreed.\n\
            00:02:10 Hopper - who owns the deck?\n";

        assert_eq!(
            read(text.as_bytes(), TranscriptFormat::Text).expect("reads"),
            vec![
                line(5_000, 90_000, "Ada", "Launch is Friday."),
                // No time of its own: it starts when the line before it did, and is still Ada's.
                line(5_000, 90_000, "Ada", "I'll send the notes after."),
                line(90_000, 130_000, "Grace", "Agreed."),
                line(130_000, 130_000, "Grace", "Hopper - who owns the deck?"),
            ]
        );
    }

    #[test]
    fn a_heading_line_names_the_speaker_of_the_lines_after_it() {
        let text = "Ada Lovelace (00:00:03)\nLaunch is Friday.\n\nGrace Hopper - 0:09\n\
            Agreed.\nShip it.\n\n[00:00:20]\nSam:\nOne more thing.\n";

        assert_eq!(
            read(text.as_bytes(), TranscriptFormat::Text).expect("reads"),
            vec![
                line(3_000, 9_000, "Ada Lovelace", "Launch is Friday."),
                line(9_000, 20_000, "Grace Hopper", "Agreed."),
                line(9_000, 20_000, "Grace Hopper", "Ship it."),
                line(20_000, 20_000, "Sam", "One more thing."),
            ]
        );
    }

    #[test]
    fn markdown_reads_past_its_marks_and_its_titles() {
        let markdown = "# Weekly sync\n\n---\n\n## Ada Lovelace (00:01:00)\n\n\
            - **Grace:** The deck is _ready_.\n> **Ada**: Ship it.\n";

        assert_eq!(
            read(markdown.as_bytes(), TranscriptFormat::Markdown).expect("reads"),
            vec![
                line(60_000, 60_000, "Grace", "The deck is _ready_."),
                line(60_000, 60_000, "Ada", "Ship it."),
            ]
        );
    }

    #[test]
    fn a_transcript_with_no_times_is_in_words_alone() {
        let text = "Ada: Launch is Friday.\nGrace: Agreed.\nWho owns the deck?\n";
        let lines = read(text.as_bytes(), TranscriptFormat::Text).expect("reads");

        assert!(is_untimed(&lines));
        assert_eq!(
            lines,
            vec![
                line(0, 0, "Ada", "Launch is Friday."),
                line(0, 0, "Grace", "Agreed."),
                line(0, 0, "Grace", "Who owns the deck?"),
            ]
        );
        // Only the first colon can name a speaker, and a time is not one.
        let sentence = read(
            b"Ada: We met at 10:30 and decided this: ship it.",
            TranscriptFormat::Text,
        )
        .expect("reads");
        assert_eq!(
            sentence,
            vec![line(
                0,
                0,
                "Ada",
                "We met at 10:30 and decided this: ship it."
            )]
        );
        assert!(!is_untimed(&[line(0, 1_000, "Ada", "Hi")]));
    }

    #[test]
    fn text_saved_by_windows_tools_reads_as_text() {
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Ada: Hello".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let bom = b"\xEF\xBB\xBFAda: Hello";

        for bytes in [utf16.as_slice(), bom.as_slice()] {
            assert_eq!(
                read(bytes, TranscriptFormat::Text).expect("reads"),
                vec![line(0, 0, "Ada", "Hello")]
            );
        }
    }

    #[test]
    fn what_is_not_a_transcript_is_refused() {
        assert_eq!(
            read(b"\0\0\0\x20ftypisom", TranscriptFormat::Text),
            Err(TranscriptError::NotText)
        );
        assert_eq!(
            read(b"\xC3\x28 broken", TranscriptFormat::Text),
            Err(TranscriptError::NotText)
        );
        assert_eq!(
            read(b"WEBVTT\n\nNOTE nothing here\n", TranscriptFormat::WebVtt),
            Err(TranscriptError::Empty)
        );
        assert_eq!(
            read(b"# Title\n\n---\n", TranscriptFormat::Markdown),
            Err(TranscriptError::Empty)
        );
        let too_long = "Ada: hi\n".repeat(MAX_LINES + 1);
        assert_eq!(
            read(too_long.as_bytes(), TranscriptFormat::Text),
            Err(TranscriptError::TooLong)
        );
    }

    #[test]
    fn times_read_as_transcripts_write_them() {
        assert_eq!(milliseconds("1:23"), Some(83_000));
        assert_eq!(milliseconds("01:23.4"), Some(83_400));
        assert_eq!(milliseconds("1:02:03,045"), Some(3_723_045));
        assert_eq!(milliseconds("x:00"), None);
    }
}
