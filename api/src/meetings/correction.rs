// Copyright © 2026 Jalapeno Labs

//! Correcting a meeting's transcript: what the model reads, and the checks its answer has to
//! pass. Pure; the step that runs it is `super::correct`.
//!
//! Correction is one text turn on the runner (`crate::runner::TurnMode::Text`), however long the
//! meeting: no tools, no settings, only the company doc, what other meetings taught, and the
//! transcript's numbered lines beside everything Whisper heard, and a JSON answer back naming
//! the lines it would change.
//!
//! # The corrected copy keeps the transcript's structure
//!
//! A corrected copy follows one transcript line by line ([`base`]): the source's, with its
//! speakers, or Whisper's segments when the source sent no lines. The model never writes the
//! copy; it names lines by number and gives each one's corrected text, and the copy is built
//! from the transcript with only those texts replaced ([`apply`]). So the copy has every line
//! the transcript has, in its order, with its speakers and times, whatever the model answers.
//!
//! # Nothing the model answers is trusted
//!
//! An edit is kept only when it fixes words and does nothing else ([`check_fix`]): compared
//! word by word, without regard to case or the punctuation around a word, every change must
//! replace at most [`FIX_WORDS_MAX`] words with at most as many (a name heard as several words
//! is one fix), never add words where there were none or drop words without a replacement, and
//! leave at least half of a longer line as it was. An edit for a line the transcript does not
//! have, a second edit for one line, or an empty text is refused too. A refused edit leaves its
//! line as it was, and the step logs why, by line number alone.
//!
//! # One turn
//!
//! The runner hands `claude` its prompt on standard input, which no argument limit bounds, so
//! the prompt is the whole meeting: the background once ([`background`]: the company doc whole,
//! the sections that spell names first, then what other meetings taught), every line of the
//! transcript, and every segment Whisper heard ([`prompt`]). Nothing is cut to fit. A meeting
//! too long for the model's context fails the step for good, with the CLI's reason, since every
//! attempt would read the same prompt; skipping the step lets segmenting go on from the
//! transcript as heard.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use similar::{Algorithm, DiffOp};

use super::segmentation::clock;
use super::source::TranscriptLine;
use crate::company_doc::{CompanyDocResponse, SectionResponse};
use crate::meeting_uploads;
use crate::models::company_doc::Section;
use crate::models::meeting_correction::BasedOn;
use crate::whisper::WhisperTranscript;

/// The most bytes of what other meetings taught one prompt carries. They come most often made
/// and most often heard first, so the tail past this is the rare fix and the passing name, which
/// would only crowd the doc and the meeting itself.
const OTHER_MEETINGS_BUDGET_BYTES: usize = 8_000;

/// The most bytes of the meeting's participants one prompt carries: a meeting with hundreds of
/// invitees names them all, and the transcript names who spoke.
const PARTICIPANTS_BUDGET_BYTES: usize = 2_000;

/// The doc's sections in the order a correction needs them: how people's names are spelled,
/// then the company's terms, then what its products, projects, and stakeholders are called.
const SECTION_ORDER: [Section; 7] = [
    Section::People,
    Section::Glossary,
    Section::Products,
    Section::Projects,
    Section::Stakeholders,
    Section::Leadership,
    Section::Company,
];

/// The most words one fix replaces, and the most it replaces them with: a name heard as
/// several words ("Ely see um" for "Elysium") is one fix, a clause rewritten is not.
pub const FIX_WORDS_MAX: usize = 4;

pub const SYSTEM_PROMPT: &str = "\
You correct the transcript of a company's meeting. Transcription mishears names, products, and \
jargon, and you fix those words and nothing else. You are given the company's reference doc, \
which spells its people, products, projects, and terms; corrections made to its other meetings; \
the names its meetings' speakers go by; and the transcript's numbered lines, beside a second, \
independent transcript of the same audio when there is one, timed the same way. Where the two \
transcripts disagree on a name or a term, prefer the spelling the doc or the other meetings \
support. \
Fix a line only by replacing a misheard or misspelled word, or the few words a name was heard \
as, with the right one, keeping every other word as it is. Never add or drop words, reword, \
summarize, or correct grammar; never fill in what nobody said; never change who spoke. When you \
are not sure, leave the line alone. \
Answer with one JSON object and nothing else: \
{\"edits\": [{\"line\": number, \"text\": string}]}, one edit for each line you change, its text \
the whole corrected line. Answer {\"edits\": []} when nothing needs fixing.";

/// The transcript a corrected copy follows, line by line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    pub based_on: BasedOn,
    pub lines: Vec<TranscriptLine>,
}

/// The transcript a meeting's corrected copy follows: the source's lines, which name their
/// speakers, or Whisper's segments when the source sent none. `None` when nothing was heard at
/// all, so there is nothing to correct.
///
/// A source transcript with no times at all, such as one uploaded beside its recording, follows
/// Whisper's segments too when Whisper heard any: they carry the recording's clock, which clips,
/// the player, and every span need, where the source's lines would make a meeting in words alone
/// of a meeting that has a recording. The source's lines stay with their capture, as heard.
pub fn base(source: Vec<TranscriptLine>, whisper: Option<&WhisperTranscript>) -> Option<Base> {
    let heard: Vec<TranscriptLine> = whisper
        .into_iter()
        .flat_map(|transcript| &transcript.segments)
        .filter(|segment| !segment.text.trim().is_empty())
        .map(|segment| TranscriptLine {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            speaker: String::new(),
            text: segment.text.trim().to_owned(),
        })
        .collect();
    let source_is_timed = !meeting_uploads::transcript::is_untimed(&source);
    if !source.is_empty() && (source_is_timed || heard.is_empty()) {
        return Some(Base {
            based_on: BasedOn::Source,
            lines: source,
        });
    }
    if heard.is_empty() {
        return None;
    }
    Some(Base {
        based_on: BasedOn::Whisper,
        lines: heard,
    })
}

/// One word or few, as a transcript heard them and as a correction wrote them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fix {
    pub heard: String,
    pub written: String,
}

/// What the correction reads besides the meeting's transcripts.
#[derive(Debug, Clone, Copy)]
pub struct Background<'context> {
    pub title: &'context str,
    pub participants: &'context [String],
    pub doc: &'context CompanyDocResponse,
    /// What other meetings' corrections fixed, most often first ([`learned_fixes`]).
    pub fixes: &'context [Fix],
    /// The names other meetings' speakers went by, most often heard first.
    pub speakers: &'context [String],
}

/// The part of the prompt that is not the transcript: the meeting, the company doc whole with
/// the sections that spell names first, and what other meetings taught.
pub fn background(background: Background<'_>) -> String {
    let mut text = format!("The meeting: {}\n", background.title);
    if !background.participants.is_empty() {
        let participants = background.participants.join(", ");
        let _ = writeln!(
            text,
            "Participants: {}",
            cut_to_bytes(&participants, PARTICIPANTS_BUDGET_BYTES)
        );
    }

    text.push_str("\nThe company doc, the sections that spell names first:\n");
    for key in SECTION_ORDER {
        let section = background
            .doc
            .sections
            .iter()
            .find(|section| section.section == key);
        if let Some(section) = section {
            text.push_str(&section_text(section));
        }
    }

    text.push_str("\nCorrections made to the company's other meetings, as heard → as written:\n");
    let mut other_bytes = 0;
    let mut wrote_fix = false;
    for fix in background.fixes {
        let line = format!("- {} → {}\n", fix.heard, fix.written);
        other_bytes += line.len();
        if other_bytes > OTHER_MEETINGS_BUDGET_BYTES / 2 {
            break;
        }
        text.push_str(&line);
        wrote_fix = true;
    }
    if !wrote_fix {
        text.push_str("(none yet)\n");
    }

    text.push_str("\nNames speakers went by in the company's other meetings:\n");
    let mut names = String::new();
    for speaker in background.speakers {
        if other_bytes + names.len() + speaker.len() + 2 > OTHER_MEETINGS_BUDGET_BYTES {
            break;
        }
        if !names.is_empty() {
            names.push_str(", ");
        }
        names.push_str(speaker);
    }
    if names.is_empty() {
        names.push_str("(none yet)");
    }
    text.push_str(&names);
    text.push('\n');
    text
}

/// One section as the prompt shows it: its heading and what belongs there, its text, and the
/// lines proposals waiting for review would add, marked as proposed.
fn section_text(section: &SectionResponse) -> String {
    let mut text = format!(
        "\n## {}\nWhat belongs here: {}\n",
        section.section.heading(),
        section.section.purpose()
    );
    if section.content.trim().is_empty() {
        text.push_str("(empty)\n");
    } else {
        let _ = writeln!(text, "{}", section.content.trim_end());
    }
    for proposal in &section.proposals {
        let existing: HashSet<&str> = proposal.base_content.lines().collect();
        for added in proposal.content.lines() {
            if added.trim().is_empty() || existing.contains(added) {
                continue;
            }
            let _ = writeln!(text, "(proposed, waiting for review) {added}");
        }
    }
    text
}

/// `text` cut to at most `limit` bytes, at a character's boundary.
fn cut_to_bytes(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// One line of the transcript as the prompt numbers it.
fn line_text(index: usize, line: &TranscriptLine) -> String {
    if line.speaker.is_empty() {
        return format!("[{index}] [{}] {}\n", clock(line.start_ms), line.text);
    }
    format!(
        "[{index}] [{}] {}: {}\n",
        clock(line.start_ms),
        line.speaker,
        line.text
    )
}

/// What heads the transcript's lines in the prompt, and Whisper's text after them.
const LINES_HEADING: &str =
    "\nThe transcript's lines to correct, each [number] [minutes:seconds]:\n";
const WHISPER_HEADING: &str =
    "\nWhat a second transcription heard, each [minutes:seconds], for comparison only:\n";

/// The prompt: `background`, then every line of the transcript, numbered, then every segment
/// Whisper heard, timed on the same clock so the model can set the two side by side. Nothing is
/// cut to fit.
pub fn prompt(
    background: &str,
    lines: &[TranscriptLine],
    whisper: Option<&WhisperTranscript>,
) -> String {
    let mut text = background.to_owned();
    text.push_str(LINES_HEADING);
    for (index, line) in lines.iter().enumerate() {
        text.push_str(&line_text(index, line));
    }
    let Some(whisper) = whisper.filter(|whisper| !whisper.segments.is_empty()) else {
        return text;
    };
    text.push_str(WHISPER_HEADING);
    for segment in &whisper.segments {
        let _ = writeln!(
            text,
            "[{}] {}",
            clock(segment.start_ms),
            segment.text.trim()
        );
    }
    text
}

/// Why an answer could not be used at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerError {
    #[error("the answer holds no JSON object")]
    NoJson,
    #[error("the answer is not the expected shape: {0}")]
    Shape(String),
}

/// Why one edit was refused. Each says what was wrong and never what the line says, since
/// every member reads the step's log and a meeting is its owner's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("it names a line the transcript does not have")]
    UnknownLine,
    #[error("an earlier edit changed the same line")]
    Repeated,
    #[error("its text is empty")]
    Empty,
    #[error("it adds words that were not said")]
    AddsWords,
    #[error("it drops words that were said")]
    DropsWords,
    #[error("it rewrites more than a name or a term")]
    Rewrites,
}

/// An edit the model answered that holds up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The line's index in the transcript.
    pub line: usize,
    pub text: String,
    /// What it fixed, word by word.
    pub fixes: Vec<Fix>,
}

/// One turn's answer, checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    pub edits: Vec<Edit>,
    /// Each refused edit's line as the model named it, and why.
    pub refused: Vec<(String, Refusal)>,
}

#[derive(Debug, Deserialize)]
struct Answer {
    edits: Vec<AnswerEdit>,
}

#[derive(Debug, Deserialize)]
struct AnswerEdit {
    line: Value,
    #[serde(default)]
    text: String,
}

/// The edits in the model's answer that hold up: each for one of `lines`, the first for that
/// line, and only fixing words ([`check_fix`]). An edit whose text is the line as it is changes
/// nothing and is dropped without a word.
///
/// # Errors
/// [`AnswerError`] when the answer holds no JSON object or is not the asked-for shape.
pub fn parse_answer(answer: &str, lines: &[TranscriptLine]) -> Result<Checked, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let parsed: Answer =
        serde_json::from_str(json).map_err(|error| AnswerError::Shape(error.to_string()))?;

    let mut checked = Checked::default();
    let mut edited = HashSet::new();
    for candidate in parsed.edits {
        let named = candidate.line.to_string();
        let index = candidate
            .line
            .as_u64()
            .and_then(|line| usize::try_from(line).ok())
            .filter(|line| *line < lines.len());
        let Some(index) = index else {
            checked.refused.push((named, Refusal::UnknownLine));
            continue;
        };
        if !edited.insert(index) {
            checked.refused.push((named, Refusal::Repeated));
            continue;
        }
        let text = candidate.text.trim();
        if text.is_empty() {
            checked.refused.push((named, Refusal::Empty));
            continue;
        }
        let original = &lines[index].text;
        if text == original.trim() {
            continue;
        }
        match check_fix(original, text) {
            Ok(fixes) => checked.edits.push(Edit {
                line: index,
                text: text.to_owned(),
                fixes,
            }),
            Err(refusal) => checked.refused.push((named, refusal)),
        }
    }
    Ok(checked)
}

/// A word as fixes are compared: lowercase, without the punctuation around it, so a fix of
/// case or a comma is no change of words.
pub(super) fn comparable(word: &str) -> String {
    word.trim_matches(|character: char| !character.is_alphanumeric())
        .to_lowercase()
}

/// `words` as a fix names them: joined, without the punctuation at either end.
fn phrase(words: &[&str]) -> String {
    words
        .join(" ")
        .trim_matches(|character: char| !character.is_alphanumeric())
        .to_owned()
}

/// What correcting `original` to `corrected` fixed, when it only fixes words: compared word by
/// word, every change replaces at most [`FIX_WORDS_MAX`] words with at most as many, none adds
/// or drops words alone, and the words replaced are at most half of the line's (or
/// [`FIX_WORDS_MAX`], for a short line).
///
/// # Errors
/// The [`Refusal`] naming the first rule the change breaks.
pub fn check_fix(original: &str, corrected: &str) -> Result<Vec<Fix>, Refusal> {
    let original_words: Vec<&str> = original.split_whitespace().collect();
    let corrected_words: Vec<&str> = corrected.split_whitespace().collect();
    let original_keys: Vec<String> = original_words.iter().map(|word| comparable(word)).collect();
    let corrected_keys: Vec<String> = corrected_words
        .iter()
        .map(|word| comparable(word))
        .collect();

    let mut fixes = Vec::new();
    let mut replaced = 0;
    for operation in similar::capture_diff_slices(Algorithm::Myers, &original_keys, &corrected_keys)
    {
        match operation {
            DiffOp::Equal { .. } => {}
            DiffOp::Insert { .. } => return Err(Refusal::AddsWords),
            DiffOp::Delete { .. } => return Err(Refusal::DropsWords),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                if old_len > FIX_WORDS_MAX || new_len > FIX_WORDS_MAX {
                    return Err(Refusal::Rewrites);
                }
                replaced += old_len;
                fixes.push(Fix {
                    heard: phrase(&original_words[old_index..old_index + old_len]),
                    written: phrase(&corrected_words[new_index..new_index + new_len]),
                });
            }
        }
    }
    if replaced > FIX_WORDS_MAX.max(original_words.len() / 2) {
        return Err(Refusal::Rewrites);
    }
    Ok(fixes)
}

/// `lines` with each of `edits`' texts in place of its line's: every line kept, in order, with
/// its speaker and times.
pub fn apply(lines: &[TranscriptLine], edits: &[Edit]) -> Vec<TranscriptLine> {
    let by_line: HashMap<usize, &str> = edits
        .iter()
        .map(|edit| (edit.line, edit.text.as_str()))
        .collect();
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| TranscriptLine {
            text: by_line
                .get(&index)
                .map_or_else(|| line.text.clone(), |text| (*text).to_owned()),
            ..line.clone()
        })
        .collect()
}

/// Every distinct fix among `edits`, in the order they were made.
pub fn distinct_fixes(edits: &[Edit]) -> Vec<Fix> {
    let mut seen = HashSet::new();
    let mut distinct = Vec::new();
    for fix in edits.iter().flat_map(|edit| &edit.fixes) {
        if seen.insert(fix) {
            distinct.push(fix.clone());
        }
    }
    distinct
}

/// What other meetings' corrections fixed, as stored (`meeting_corrections.fixes`, one array
/// each), merged: each distinct fix once, those made in the most meetings first. A value that
/// does not read as fixes is passed over.
pub fn learned_fixes(stored: &[Value]) -> Vec<Fix> {
    let mut counts: HashMap<Fix, usize> = HashMap::new();
    let mut order = Vec::new();
    for value in stored {
        let Ok(fixes) = serde_json::from_value::<Vec<Fix>>(value.clone()) else {
            continue;
        };
        for fix in fixes {
            let count = counts.entry(fix.clone()).or_insert(0);
            if *count == 0 {
                order.push(fix);
            }
            *count += 1;
        }
    }
    // A stable sort keeps the newest meetings' fixes first among those made as often.
    order.sort_by_key(|fix| std::cmp::Reverse(counts[fix]));
    order
}

/// The SHA-256 of `lines` serialized as they are stored, in lowercase hex, as the source's
/// transcript is hashed (`super::fetch`).
pub fn transcript_hash(lines: &[TranscriptLine]) -> String {
    let serialized = serde_json::to_vec(lines).expect("transcript lines are plain fields");
    hex::encode(Sha256::digest(serialized))
}

/// The first `{` to the last `}`: models sometimes wrap JSON in a sentence or a code fence.
fn json_object(answer: &str) -> Option<&str> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    (end > start).then(|| &answer[start..=end])
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::company_doc::ProposalResponse;
    use crate::whisper::{WhisperSegment, WhisperWord};

    fn line(start_ms: i64, speaker: &str, text: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms,
            end_ms: start_ms + 3_000,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        }
    }

    fn whisper(segments: &[(i64, &str)]) -> WhisperTranscript {
        WhisperTranscript {
            model: "Whisper large-v3".to_owned(),
            language: "en".to_owned(),
            language_probability: 0.99,
            duration_ms: 3_600_000,
            segments: segments
                .iter()
                .map(|(start_ms, text)| WhisperSegment {
                    start_ms: *start_ms,
                    end_ms: start_ms + 3_000,
                    text: (*text).to_owned(),
                    words: vec![WhisperWord {
                        start_ms: *start_ms,
                        end_ms: start_ms + 3_000,
                        text: format!(" {text}"),
                        probability: 0.9,
                    }],
                })
                .collect(),
        }
    }

    fn doc(content: &str) -> CompanyDocResponse {
        CompanyDocResponse {
            sections: Section::ALL
                .into_iter()
                .map(|section| SectionResponse {
                    section,
                    purpose: section.purpose(),
                    content: format!("{content} for {}", section.as_str()),
                    updated_at: Utc::now(),
                    proposals: Vec::new(),
                })
                .collect(),
        }
    }

    fn background_of(doc: &CompanyDocResponse) -> String {
        background(Background {
            title: "Weekly sync",
            participants: &["ada@example.com".to_owned()],
            doc,
            fixes: &[],
            speakers: &[],
        })
    }

    fn fix(heard: &str, written: &str) -> Fix {
        Fix {
            heard: heard.to_owned(),
            written: written.to_owned(),
        }
    }

    #[test]
    fn a_copy_follows_the_sources_lines_or_whispers_when_the_source_sent_none() {
        let heard = whisper(&[(0, "Hello there"), (4_000, "  ")]);
        let source = vec![line(0, "Ada", "Hello there")];

        let from_source = base(source.clone(), Some(&heard)).expect("lines");
        assert_eq!(from_source.based_on, BasedOn::Source);
        assert_eq!(from_source.lines, source);

        let from_whisper = base(Vec::new(), Some(&heard)).expect("segments");
        assert_eq!(from_whisper.based_on, BasedOn::Whisper);
        assert_eq!(
            from_whisper.lines.len(),
            1,
            "a silent segment is not a line"
        );
        assert_eq!(from_whisper.lines[0].speaker, "");

        assert_eq!(base(Vec::new(), None), None);
    }

    /// #233: a transcript uploaded with no times beside its recording follows the recording's
    /// clock, so the meeting is not taken for one in words alone.
    #[test]
    fn an_untimed_source_with_a_recording_follows_whispers_clock() {
        let untimed = vec![TranscriptLine {
            start_ms: 0,
            end_ms: 0,
            speaker: "Ada".to_owned(),
            text: "Hello there".to_owned(),
        }];
        let heard = whisper(&[(2_000, "Hello there")]);

        let followed = base(untimed.clone(), Some(&heard)).expect("lines");
        assert_eq!(followed.based_on, BasedOn::Whisper);
        assert_eq!(followed.lines[0].start_ms, 2_000);

        // With no recording heard, its lines are the meeting, in words alone.
        let alone = base(untimed.clone(), None).expect("lines");
        assert_eq!((alone.based_on, alone.lines), (BasedOn::Source, untimed));
    }

    #[test]
    fn a_misheard_name_or_term_is_a_fix() {
        assert_eq!(
            check_fix(
                "Jon will demo Ely see um on Friday.",
                "John will demo Elysium on Friday."
            ),
            Ok(vec![fix("Jon", "John"), fix("Ely see um", "Elysium")])
        );
        // Case and the punctuation around a word change no words.
        assert_eq!(check_fix("ask jira, then", "Ask Jira then"), Ok(Vec::new()));
    }

    #[test]
    fn an_edit_that_adds_drops_or_rewrites_words_is_refused() {
        assert_eq!(
            check_fix("Launch is Friday", "Launch is Friday at noon"),
            Err(Refusal::AddsWords)
        );
        assert_eq!(
            check_fix("Launch is maybe Friday", "Launch is Friday"),
            Err(Refusal::DropsWords)
        );
        assert_eq!(
            check_fix(
                "we should talk about the budget later today",
                "we agreed the budget doubles next quarter now"
            ),
            Err(Refusal::Rewrites)
        );
        // Small replacements of more than half a line add up to a rewrite too.
        assert_eq!(
            check_fix(
                "one two three four five six seven eight nine ten",
                "uno two tres four cinco six siete ocho nueve ten"
            ),
            Err(Refusal::Rewrites)
        );
        let half = check_fix(
            "one two three four five six seven eight nine ten",
            "uno two tres four cinco six siete eight nueve ten",
        );
        assert!(
            matches!(half, Ok(fixes) if fixes.len() == 5),
            "half a line is the most a correction replaces"
        );
    }

    #[test]
    fn the_answer_keeps_only_edits_to_lines_the_transcript_has_once_each() {
        let lines = vec![
            line(0, "Ada", "Jon is here"),
            line(3_000, "Grace", "Ship it"),
            line(6_000, "Ada", "Later"),
        ];
        let answer = r#"Here you go: {"edits": [
            {"line": 0, "text": "John is here"},
            {"line": 0, "text": "Johnny is here"},
            {"line": 1, "text": "Ship it"},
            {"line": 2, "text": "Later on"},
            {"line": 3, "text": "Invented"},
            {"line": "one", "text": "Jon"},
            {"line": 1, "text": "  "}
        ]}"#;

        let checked = parse_answer(answer, &lines).expect("parses");

        assert_eq!(checked.edits.len(), 1);
        assert_eq!(checked.edits[0].line, 0);
        assert_eq!(checked.edits[0].text, "John is here");
        assert_eq!(checked.edits[0].fixes, vec![fix("Jon", "John")]);
        let refusals: Vec<Refusal> = checked
            .refused
            .iter()
            .map(|(_, refusal)| *refusal)
            .collect();
        // Line 1 given back as it is changes nothing, and is dropped without a word; line 2
        // adds a word; there is no line 3.
        assert_eq!(
            refusals,
            vec![
                Refusal::Repeated,
                Refusal::AddsWords,
                Refusal::UnknownLine,
                Refusal::UnknownLine,
                Refusal::Repeated,
            ]
        );
        assert_eq!(parse_answer("no", &lines), Err(AnswerError::NoJson));
        assert!(matches!(
            parse_answer("{\"lines\": []}", &lines),
            Err(AnswerError::Shape(_))
        ));
    }

    #[test]
    fn an_edit_with_no_text_is_refused() {
        let lines = vec![line(0, "Ada", "Jon is here")];
        let checked =
            parse_answer(r#"{"edits": [{"line": 0, "text": " "}]}"#, &lines).expect("parses");
        assert_eq!(checked.edits, Vec::new());
        assert_eq!(checked.refused, vec![("0".to_owned(), Refusal::Empty)]);
    }

    #[test]
    fn the_copy_keeps_every_line_speaker_and_time_and_changes_only_text() {
        let lines = vec![
            line(0, "Ada", "Jon is here"),
            line(3_000, "Grace", "Ship it"),
        ];
        let edits = vec![Edit {
            line: 0,
            text: "John is here".to_owned(),
            fixes: vec![fix("Jon", "John")],
        }];

        let corrected = apply(&lines, &edits);

        assert_eq!(corrected.len(), lines.len());
        for (original, copy) in lines.iter().zip(&corrected) {
            assert_eq!(
                (original.start_ms, original.end_ms, &original.speaker),
                (copy.start_ms, copy.end_ms, &copy.speaker)
            );
        }
        assert_eq!(corrected[0].text, "John is here");
        assert_eq!(corrected[1], lines[1]);
        assert_ne!(transcript_hash(&corrected), transcript_hash(&lines));
        assert_eq!(
            distinct_fixes(&[edits[0].clone(), edits[0].clone()]),
            vec![fix("Jon", "John")]
        );
    }

    #[test]
    fn a_long_meeting_is_one_prompt_with_every_line_and_everything_whisper_heard() {
        // Three hours of meeting, several times what one argument could carry.
        let lines: Vec<TranscriptLine> = (0..3_600)
            .map(|index| {
                line(
                    index * 3_000,
                    "Ada",
                    &format!("line {index} {}", "word ".repeat(20)),
                )
            })
            .collect();
        let texts: Vec<(i64, String)> = (0..3_600)
            .map(|index| {
                (
                    index * 3_000,
                    format!("heard {index} {}", "word ".repeat(20)),
                )
            })
            .collect();
        let segments: Vec<(i64, &str)> = texts
            .iter()
            .map(|(start_ms, text)| (*start_ms, text.as_str()))
            .collect();
        let heard = whisper(&segments);
        let background = background_of(&doc("- Ada Lovelace"));

        let text = prompt(&background, &lines, Some(&heard));

        assert!(text.len() > 3 * 128 * 1024, "{} bytes", text.len());
        assert_eq!(
            text.matches("## People").count(),
            1,
            "the doc is read once, however long the meeting"
        );
        for (index, line) in lines.iter().enumerate() {
            assert!(
                text.contains(&format!(
                    "[{index}] [{}] Ada: {}\n",
                    clock(line.start_ms),
                    line.text
                )),
                "line {index} is whole"
            );
        }
        for (start_ms, said) in &texts {
            assert!(text.contains(&format!("[{}] {}\n", clock(*start_ms), said.trim())));
        }
        let lines_at = text.find("[0] [0:00] Ada").expect("the lines");
        let whisper_at = text
            .find("What a second transcription heard")
            .expect("Whisper's");
        assert!(
            lines_at < whisper_at,
            "the transcript to correct comes first"
        );
    }

    #[test]
    fn a_meeting_with_no_whisper_transcript_is_its_lines_alone() {
        let lines = vec![line(0, "Ada", "Jon is here"), line(3_000, "", "Ship it")];

        let text = prompt("Background\n", &lines, None);

        assert!(text.starts_with("Background\n"));
        assert!(text.contains("[0] [0:00] Ada: Jon is here\n"));
        assert!(
            text.contains("[1] [0:03] Ship it\n"),
            "a line nobody is named for"
        );
        assert!(!text.contains("second transcription"));
    }

    #[test]
    fn the_doc_spells_people_and_terms_first_and_marks_what_is_only_proposed() {
        let mut current = doc("- Entry");
        let glossary = current
            .sections
            .iter_mut()
            .find(|section| section.section == Section::Glossary)
            .expect("a glossary");
        glossary.proposals.push(ProposalResponse {
            id: uuid::Uuid::now_v7(),
            section: "glossary".to_owned(),
            base_content: "- Entry for glossary".to_owned(),
            content: "- Elysium\n- Entry for glossary".to_owned(),
            reason: "Heard in a meeting".to_owned(),
            quote: None,
            source: None,
            timeline_record_id: None,
            workflow_run_id: None,
            state: "pending".to_owned(),
            decided_by: None,
            decided_at: None,
            created_by: uuid::Uuid::now_v7(),
            agent: None,
            created_at: Utc::now(),
        });

        let shared = background_of(&current);

        let people = shared.find("## People").expect("people");
        let glossary = shared.find("## Glossary").expect("glossary");
        let company = shared.find("## Company").expect("company");
        assert!(people < glossary && glossary < company);
        assert!(shared.contains("(proposed, waiting for review) - Elysium"));
        assert!(!shared.contains("(proposed, waiting for review) - Entry"));
    }

    #[test]
    fn a_long_doc_goes_whole() {
        let long = doc(&"- A name spelled out at length\n".repeat(400));

        let shared = background_of(&long);

        for section in &long.sections {
            assert!(
                shared.contains(section.content.trim_end()),
                "{} is whole",
                section.section.as_str()
            );
        }
        assert!(!shared.contains("did not fit"));
    }

    #[test]
    fn fixes_other_meetings_made_most_often_come_first() {
        let stored = vec![
            serde_json::to_value(vec![fix("Jon", "John"), fix("Ely see um", "Elysium")])
                .expect("serializes"),
            serde_json::to_value(vec![fix("Ely see um", "Elysium")]).expect("serializes"),
            serde_json::json!("not fixes"),
        ];

        assert_eq!(
            learned_fixes(&stored),
            vec![fix("Ely see um", "Elysium"), fix("Jon", "John")]
        );
    }
}
