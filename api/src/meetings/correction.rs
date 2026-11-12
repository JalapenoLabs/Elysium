// Copyright © 2026 Jalapeno Labs

//! Settling a meeting's final transcript: what the correcting agent reads, and the checks its
//! answer has to pass. Pure; the step that runs it is `super::correct`.
//!
//! Correction is one text turn on the runner (`crate::runner::TurnMode::Text`), however long the
//! meeting: no tools, no settings, only the Transcription doc, the company doc, and the meeting's
//! whole final transcript, every capture's own version of each line beside it, and a JSON answer
//! back flagging the words it is unsure of or would change.
//!
//! # What it reads
//!
//! The final transcript is already lined up from every capture (`super::final_transcript`), and the
//! prompt shows it as numbered lines ([`Transcript`]): each a run of one speaker's words. Under a
//! line, every capture's own transcript of the same moment that heard it otherwise, its source's
//! and Whisper's alike ([`Version`]): agreement is not trust, since a name every capture mishears
//! the same way is still wrong, and Whisper is one capture among the others, never the source of
//! truth.
//!
//! # What it answers
//!
//! Flags, each over a run of a line's words, which it names by copying them: the text it decided
//! on, its confidence from 0 to 100, and up to three alternatives, or 0 with none when it cannot
//! tell what was said ([`Flag`]). A run whose text the final transcript changed from what a capture
//! heard, with no flag over it, is marked changed at [`CHANGED_CONFIDENCE`], with what the captures
//! heard as its alternatives ([`unflagged_changes`]), so nothing the transcript changed goes
//! unseen.
//!
//! # Who reads it
//!
//! The same turn judges whether the meeting is sensitive, and so who reads it
//! (`docs/meeting-review.md`, Who sees a meeting): beside the transcript it is told whose meeting it
//! is and the people it can be shared with, those who took part first ([`Person`]), and it answers
//! `visibility` beside its flags. [`parse_judgment`] reads that apart from the flags, so a garbled
//! judgment never costs the settlement, and anything it cannot read is
//! [`Judgment::Undecided`], which keeps the meeting private: a meeting is never opened on a guess.
//! Only people it was offered are shared with.
//!
//! # Nothing the model answers is trusted
//!
//! A flag is kept only over words of a line it was given, found there once, overlapping no flag
//! kept before it, and its text replaces only its words. The text must only fix words
//! ([`check_fix`]): compared word by word, without regard to case or the punctuation around a word,
//! every change replaces at most [`FIX_WORDS_MAX`] words with at most as many (a name heard as
//! several words is one fix), never adds words where there were none or drops words without a
//! replacement, and leaves at least half of a longer run as it was. Its confidence is clamped to 0
//! to 100, and its alternatives must each pass the same check. A refused flag leaves its words as
//! they were, and the step logs why, by line number alone.
//!
//! # One turn
//!
//! The runner hands `claude` its prompt on standard input, which no argument limit bounds, so the
//! prompt is the whole meeting: the background once ([`background`]: the Transcription doc's
//! entries, the company doc whole, then the names speakers went by), then every line and every
//! capture's version of it ([`prompt`]). Nothing of the meeting is cut to fit; the Transcription
//! doc, which grows with every meeting, gives its entries heard in the most meetings first, within
//! [`TRANSCRIPTION_BUDGET_BYTES`]. A meeting too long for the model's context
//! fails the step for good, with the CLI's reason, since every attempt would read the same prompt;
//! skipping the step lets segmenting go on from the transcript as heard.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::ops::Range;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use similar::{Algorithm, DiffOp};
use uuid::Uuid;

use super::alignment;
use super::segmentation::clock;
use super::source::TranscriptLine;
use crate::company_doc::{CompanyDocResponse, SectionResponse};
use crate::meeting_uploads;
use crate::meetings::visibility::{self, Judgment};
use crate::models::company_doc::Section;
use crate::models::meeting_correction::BasedOn;
use crate::models::meeting_word::{BuiltWord, Origin, Word};
use crate::transcription_doc::Entry;
use crate::whisper::{WhisperTranscript, WhisperWord};

/// The most bytes of the meeting's participants one prompt carries: a meeting with hundreds of
/// invitees names them all, and the transcript names who spoke.
const PARTICIPANTS_BUDGET_BYTES: usize = 2_000;

/// The most bytes of the names other meetings' speakers went by: the company's recurring names,
/// without crowding the meeting itself.
const SPEAKERS_BUDGET_BYTES: usize = 4_000;

/// The most bytes of the people a private meeting can be shared with: a few hundred people, those
/// who took part listed first, so a large workspace leaves out only people the meeting never named.
const PEOPLE_BUDGET_BYTES: usize = 12_000;

/// The most people a private meeting is shared with on its correction's word: more than any
/// meeting that needs keeping private has taking part.
const SHARE_MAX: usize = 50;

/// The most bytes of the Transcription doc's entries one prompt carries: several hundred entries,
/// those heard in the most meetings first. The doc grows with every meeting, and a prompt past the
/// model's context fails the step for good, so the rarest are left out rather than the meeting.
pub const TRANSCRIPTION_BUDGET_BYTES: usize = 60_000;

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

/// The confidence a run is marked at when the final transcript says other than a capture heard
/// and the correcting agent flagged nothing there: neither sure nor unsure, so a person sees it
/// changed.
pub const CHANGED_CONFIDENCE: u8 = 50;

/// The most alternatives a flag keeps, as a person reviewing it picks from them in one click.
const ALTERNATIVES_MAX: usize = 3;

/// The longest alternative kept, in characters: a name or a few words, never a paragraph.
const ALTERNATIVE_MAX_CHARACTERS: usize = 200;

/// The longest reason a flag keeps, in characters: one sentence, which the Transcription doc
/// shows beside what it learned.
const WHY_MAX_CHARACTERS: usize = 300;

/// A pause this long between two words starts a new line of the prompt: about the gap between
/// two sentences.
const LINE_PAUSE_MS: i64 = 2_000;

/// The most words one line of the prompt holds, so a long monologue still reads in lines a flag
/// can name.
const LINE_WORDS_MAX: usize = 60;

pub const SYSTEM_PROMPT: &str = "\
You settle the transcript of a company's meeting. Every recording and transcription of the meeting \
(its captures) has been lined up into one transcript of numbered lines; under a line is what any \
capture heard otherwise at the same moment. Transcription mishears names, products, and jargon, \
and each capture mishears them its own way. Agreement is not proof: a name every capture mishears \
the same way is still wrong. Whisper, an automatic transcription, is one capture among the others \
and never the truth. You are given the company's Transcription doc, which records how its speech \
is misheard and what it really is, and its reference doc, which spells its people, products, \
projects, and terms. \
Flag each run of words you would change or are unsure of: its line, its words copied exactly as \
the line has them, the text you decide on, how sure you are of that text from 0 to 100, and up to \
three other things it may have said. Change a run only by replacing a misheard or misspelled word, \
or the few words a name was heard as, with the right one, keeping every other word as it is. Never \
add or drop words, reword, summarize, or correct grammar; never fill in what nobody said; never \
change who spoke. Flag a run you cannot make out at 0 with no alternatives, for a person to listen \
to. What you are sure of as it stands needs no flag. \
Then judge who should read the meeting. Everyone in the company reads its meetings unless one is \
sensitive: a one-to-one between a manager and someone they manage, a discussion of someone's \
performance, pay, hiring, or leaving, or a health, family, or other personal matter. Answer \
\"visibility\": private true for a sensitive meeting, with shareWith the ids of the people listed \
who should read it besides its owner, such as those who took part, and why in one short sentence; \
private false for every other meeting, with shareWith empty. \
Answer with one JSON object and nothing else: {\"flags\": [{\"line\": number, \"heard\": string, \
\"text\": string, \"confidence\": number, \"alternatives\": [string], \"why\": string}], \
\"visibility\": {\"private\": boolean, \"shareWith\": [string], \"why\": string}}, \"heard\" found \
exactly once in its line and each \"why\" one short sentence. Answer \"flags\": [] when nothing \
needs a flag.";

/// The transcript a capture's lines follow: the source's lines, with their speakers, or Whisper's
/// segments when the source sent none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    pub based_on: BasedOn,
    pub lines: Vec<TranscriptLine>,
}

/// The transcript a capture's lines follow: the source's lines, which name their speakers, or
/// Whisper's segments when the source sent none. `None` when nothing was heard at all.
///
/// A source transcript with no times at all, such as one uploaded beside its recording, follows
/// Whisper's segments too when Whisper heard any: they carry the recording's clock, which clips,
/// the player, and every span need, where the source's lines would make a meeting in words alone
/// of a meeting that has a recording. The source's lines stay with their capture, as heard.
pub fn base(source: Vec<TranscriptLine>, whisper: Option<&WhisperTranscript>) -> Option<Base> {
    let heard = whisper.map(whisper_lines).unwrap_or_default();
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

/// Whisper's segments as lines, naming no speaker, since Whisper tells nobody apart.
pub fn whisper_lines(whisper: &WhisperTranscript) -> Vec<TranscriptLine> {
    whisper
        .segments
        .iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .map(|segment| TranscriptLine {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            speaker: String::new(),
            text: segment.text.trim().to_owned(),
        })
        .collect()
}

/// One word or few, as a transcript heard them and as a correction wrote them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fix {
    pub heard: String,
    pub written: String,
}

/// Someone on Elysium a private meeting can be shared with, as the correction is shown them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: Uuid,
    pub name: String,
    pub email: Option<String>,
}

/// `people` in the order the correction is shown them: those who took part in the meeting first,
/// the ones a participant or speaker in `took_part` names by their email or name, regardless of
/// case, then the rest, each part in its own order.
pub fn people_in_order(people: Vec<Person>, took_part: &[String]) -> Vec<Person> {
    let named: HashSet<String> = took_part
        .iter()
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    let (mut in_it, rest): (Vec<Person>, Vec<Person>) = people.into_iter().partition(|person| {
        named.contains(&person.name.trim().to_lowercase())
            || person
                .email
                .as_deref()
                .is_some_and(|email| named.contains(&email.trim().to_lowercase()))
    });
    in_it.extend(rest);
    in_it
}

/// What the correction reads besides the meeting's transcript.
#[derive(Debug, Clone, Copy)]
pub struct Background<'context> {
    pub title: &'context str,
    pub participants: &'context [String],
    /// Whose meeting it is, by name.
    pub owner: &'context str,
    /// Who it can be shared with, in [`people_in_order`]'s order, its owner left out.
    pub people: &'context [Person],
    /// The Transcription doc's entries.
    pub transcription: &'context [Entry],
    pub doc: &'context CompanyDocResponse,
    /// The names other meetings' speakers went by, most often heard first.
    pub speakers: &'context [String],
}

/// The part of the prompt that is not the transcript: the meeting, the Transcription doc's entries
/// ([`transcription_text`]), the company doc whole with the sections that spell names first, and
/// the names other meetings' speakers went by.
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

    text.push_str(
        "\nThe Transcription doc, how the company's speech is misheard and what it really is:\n",
    );
    text.push_str(&transcription_text(background.transcription));

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

    text.push_str("\nNames speakers went by in the company's other meetings:\n");
    let mut names = String::new();
    for speaker in background.speakers {
        if names.len() + speaker.len() + 2 > SPEAKERS_BUDGET_BYTES {
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

    let _ = writeln!(
        text,
        "\nThe meeting belongs to {}, who reads it whatever you decide. The people on Elysium a \
         private meeting can be shared with, as id: name <email>, those who took part first:",
        background.owner
    );
    text.push_str(&people_text(background.people));
    text
}

/// `people` as the prompt lists them, one a line in their order, within [`PEOPLE_BUDGET_BYTES`],
/// saying how many were left out.
fn people_text(people: &[Person]) -> String {
    if people.is_empty() {
        return "(nobody else)\n".to_owned();
    }
    let mut text = String::new();
    let mut shown = 0;
    for person in people {
        let line = match &person.email {
            Some(email) => format!("{}: {} <{email}>\n", person.id, person.name),
            None => format!("{}: {}\n", person.id, person.name),
        };
        if text.len() + line.len() > PEOPLE_BUDGET_BYTES {
            break;
        }
        text.push_str(&line);
        shown += 1;
    }
    let left_out = people.len() - shown;
    if left_out > 0 {
        let _ = writeln!(text, "({left_out} more people are left out)");
    }
    text
}

/// The Transcription doc's `entries` as the prompt shows them, one a line: those heard in the most
/// meetings first, within [`TRANSCRIPTION_BUDGET_BYTES`], saying how many rarer ones were left out.
fn transcription_text(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return "(empty)\n".to_owned();
    }
    let mut by_strength: Vec<&Entry> = entries.iter().collect();
    // A stable sort keeps the doc's own order among entries heard as often.
    by_strength.sort_by_key(|entry| std::cmp::Reverse(entry.heard_in));
    let mut text = String::new();
    let mut shown = 0;
    for entry in &by_strength {
        let mut line = format!("- \"{}\" is {}", entry.heard, entry.written);
        if !entry.sources.is_empty() {
            let _ = write!(line, ", heard by {}", entry.sources.join(", "));
        }
        if !entry.speakers.is_empty() {
            let _ = write!(line, ", when {} spoke", entry.speakers.join(" or "));
        }
        let _ = writeln!(line, ", in {} meetings", entry.heard_in);
        if text.len() + line.len() > TRANSCRIPTION_BUDGET_BYTES {
            break;
        }
        text.push_str(&line);
        shown += 1;
    }
    let left_out = by_strength.len() - shown;
    if left_out > 0 {
        let _ = writeln!(
            text,
            "({left_out} entries heard in fewer meetings are left out)"
        );
    }
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

/// One capture's own transcript, its source's lines or Whisper's, on the meeting's clock: what the
/// prompt sets beside each line of the final transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// What the prompt calls it, such as `Teams` or `Capture 2, Whisper`.
    pub label: String,
    /// Its words in the order it heard them.
    pub words: Vec<BuiltWord>,
}

impl Version {
    /// `lines`, a capture's own transcript `offset_ms` from the meeting's clock, as words: each
    /// timed by Whisper's words where Whisper heard it (`whisper`), as the final transcript's are.
    pub fn of(
        label: String,
        lines: &[TranscriptLine],
        whisper: &[WhisperWord],
        offset_ms: i64,
    ) -> Self {
        let lines = lines.iter().map(|line| (line, Origin::Source));
        Self {
            label,
            words: alignment::capture_words(lines, whisper, offset_ms),
        }
    }
}

/// The final transcript as the prompt reads it: its words in lines, and what each capture's own
/// transcript heard over each line.
#[derive(Debug, Clone)]
pub struct Transcript<'words> {
    /// The words that stand, in order.
    pub words: &'words [Word],
    /// Each line's words, as a range of `words`.
    pub lines: Vec<Range<usize>>,
    versions: Vec<Version>,
    /// For each version, for each line, the indices of the version's words heard over it.
    heard: Vec<Vec<Vec<usize>>>,
}

impl<'words> Transcript<'words> {
    /// `words` in lines ([`lines_of`]), with each of `versions`' words placed on the line it was
    /// heard over.
    pub fn new(words: &'words [Word], versions: Vec<Version>) -> Self {
        let lines = lines_of(words);
        let spans: Vec<(i64, i64)> = lines
            .iter()
            .map(|line| {
                let start = words[line.start].start_ms;
                (start, words[line.end - 1].end_ms.max(start))
            })
            .collect();
        let heard = versions
            .iter()
            .map(|version| heard_over(&spans, &version.words))
            .collect();
        Self {
            words,
            lines,
            versions,
            heard,
        }
    }

    /// Line `line`'s text, its words with a space between each.
    fn text_of(&self, line: usize) -> String {
        words_text(&self.words[self.lines[line].clone()])
    }

    /// The words version `version` heard over line `line`.
    fn heard_over(&self, version: usize, line: usize) -> Vec<&BuiltWord> {
        self.heard[version][line]
            .iter()
            .map(|index| &self.versions[version].words[*index])
            .collect()
    }
}

/// `words` in lines: a new line where the speaker changes, after a pause of [`LINE_PAUSE_MS`], and
/// every [`LINE_WORDS_MAX`] words.
pub fn lines_of(words: &[Word]) -> Vec<Range<usize>> {
    let mut lines: Vec<Range<usize>> = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let continues = lines.last().is_some_and(|line| {
            let previous = &words[index - 1];
            line.len() < LINE_WORDS_MAX
                && previous.speaker == word.speaker
                && word.start_ms - previous.end_ms < LINE_PAUSE_MS
        });
        match lines.last_mut() {
            Some(line) if continues => line.end = index + 1,
            _ => lines.push(index..index + 1),
        }
    }
    lines
}

/// For each of the lines spanning `spans` (in order), the indices of `words` heard over it: each
/// word goes to the line its middle falls in, or the nearest one when it falls between two.
fn heard_over(spans: &[(i64, i64)], words: &[BuiltWord]) -> Vec<Vec<usize>> {
    let mut heard = vec![Vec::new(); spans.len()];
    if spans.is_empty() {
        return heard;
    }
    for (index, word) in words.iter().enumerate() {
        let middle = word.start_ms + (word.end_ms - word.start_ms) / 2;
        let after = spans.partition_point(|(start, _end)| *start <= middle);
        let line = match after {
            0 => 0,
            after if after == spans.len() => after - 1,
            after => {
                let (_start, before_end) = spans[after - 1];
                let (next_start, _end) = spans[after];
                if middle > before_end && next_start - middle < middle - before_end {
                    after
                } else {
                    after - 1
                }
            }
        };
        heard[line].push(index);
    }
    heard
}

/// What heads the transcript's lines in the prompt.
const LINES_HEADING: &str = "\nThe meeting's transcript, lined up from every capture, each line \
    [number] [minutes:seconds] Speaker: words. Under a line, each capture that heard it otherwise, \
    and what it heard; a capture that heard a line the same way is not repeated:\n";

/// The prompt: `background`, then every line of `transcript`, numbered, each with every capture's
/// version of it that heard it otherwise. Nothing is cut to fit.
pub fn prompt(background: &str, transcript: &Transcript<'_>) -> String {
    let mut text = background.to_owned();
    text.push_str(LINES_HEADING);
    for (number, range) in transcript.lines.iter().enumerate() {
        let first = &transcript.words[range.start];
        let said = transcript.text_of(number);
        if first.speaker.is_empty() {
            let _ = writeln!(text, "[{number}] [{}] {said}", clock(first.start_ms));
        } else {
            let _ = writeln!(
                text,
                "[{number}] [{}] {}: {said}",
                clock(first.start_ms),
                first.speaker
            );
        }
        let said_key = keys_text(&said);
        for (version_index, version) in transcript.versions.iter().enumerate() {
            let heard = transcript.heard_over(version_index, number);
            if heard.is_empty() {
                continue;
            }
            let heard_text = heard
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if keys_text(&heard_text) != said_key {
                let _ = writeln!(text, "    {}: {heard_text}", version.label);
            }
        }
    }
    text
}

/// Why an answer could not be used at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerError {
    #[error("the answer holds no JSON object")]
    NoJson,
    /// Where in the answer it went wrong, and never what was there: serde's own message quotes
    /// the value it refused, which may be the meeting's words, and the step's error is every
    /// member's. The message itself is logged to the server's own log.
    #[error("the answer is not the expected shape, at line {line}, column {column}")]
    Shape { line: usize, column: usize },
}

/// Why one flag was refused. Each says what was wrong and never what the line says, since every
/// member reads the step's log and a meeting is its owner's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("it names a line the transcript does not have")]
    UnknownLine,
    #[error("its words are not in its line")]
    NotInLine,
    #[error("its words are in its line more than once")]
    Ambiguous,
    #[error("it overlaps an earlier flag")]
    Overlaps,
    #[error("its text is empty")]
    Empty,
    #[error("it adds words that were not said")]
    AddsWords,
    #[error("it drops words that were said")]
    DropsWords,
    #[error("it rewrites more than a name or a term")]
    Rewrites,
}

/// A flag the model answered that holds up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    /// The words it covers, as a range of the transcript's words.
    pub words: Range<usize>,
    /// The text it decided on.
    pub text: String,
    /// How sure it is of `text`, from 0 to 100.
    pub confidence: u8,
    /// Up to [`ALTERNATIVES_MAX`] other things it may have said.
    pub alternatives: Vec<String>,
    /// Why, in a sentence, when it said.
    pub why: Option<String>,
    /// What it changed, word by word: none for a flag over words it left as they were.
    pub fixes: Vec<Fix>,
    /// Whether its text differs from its words at all, if only in case or punctuation, and so
    /// replaces them.
    pub changes_text: bool,
}

impl Flag {
    /// Its confidence mark's data, as the meeting's page reads it.
    pub fn mark(&self) -> Value {
        serde_json::json!({ "confidence": self.confidence, "alternatives": self.alternatives })
    }
}

/// One turn's answer, checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    pub flags: Vec<Flag>,
    /// Each refused flag's line, when the model named one by number, and why. Only a number is
    /// kept: whatever else the model wrote there may quote the meeting, and the step's log is every
    /// member's.
    pub refused: Vec<(Option<u64>, Refusal)>,
}

#[derive(Debug, Deserialize)]
struct Answer {
    flags: Vec<AnswerFlag>,
}

#[derive(Debug, Deserialize)]
struct AnswerFlag {
    line: Value,
    #[serde(default)]
    heard: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    confidence: Option<Value>,
    #[serde(default)]
    alternatives: Vec<Value>,
    #[serde(default)]
    why: Option<String>,
}

/// The flags in the model's answer that hold up over `transcript`, in its order (see the module's
/// docs). A flag that changes nothing and says nothing of its confidence is dropped without a
/// word; one that changes its words without a confidence is held at [`CHANGED_CONFIDENCE`].
///
/// # Errors
/// [`AnswerError`] when the answer holds no JSON object or is not the asked-for shape.
pub fn parse_answer(answer: &str, transcript: &Transcript<'_>) -> Result<Checked, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let parsed: Answer = serde_json::from_str(json).map_err(|error| {
        tracing::debug!(%error, "a correction's answer is not the expected shape");
        AnswerError::Shape {
            line: error.line(),
            column: error.column(),
        }
    })?;

    let mut checked = Checked::default();
    let mut taken: Vec<Range<usize>> = Vec::new();
    for candidate in parsed.flags {
        let named = candidate.line.as_u64();
        match check_flag(candidate, transcript, &taken) {
            Ok(Some(flag)) => {
                taken.push(flag.words.clone());
                checked.flags.push(flag);
            }
            Ok(None) => {}
            Err(refusal) => checked.refused.push((named, refusal)),
        }
    }
    checked.flags.sort_by_key(|flag| flag.words.start);
    Ok(checked)
}

/// `candidate` as a flag over `transcript`, unless it overlaps one of `taken`. `None` for a flag
/// that would change nothing and says nothing.
fn check_flag(
    candidate: AnswerFlag,
    transcript: &Transcript<'_>,
    taken: &[Range<usize>],
) -> Result<Option<Flag>, Refusal> {
    let line = candidate
        .line
        .as_u64()
        .and_then(|line| usize::try_from(line).ok())
        .filter(|line| *line < transcript.lines.len())
        .ok_or(Refusal::UnknownLine)?;
    let line_range = transcript.lines[line].clone();
    let words = find_run(
        &transcript.words[line_range.clone()],
        &candidate.heard,
        line_range.start,
    )?;
    if taken
        .iter()
        .any(|kept| kept.start < words.end && words.start < kept.end)
    {
        return Err(Refusal::Overlaps);
    }
    let text = candidate
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return Err(Refusal::Empty);
    }
    let heard = words_text(&transcript.words[words.clone()]);
    let fixes = check_fix(&heard, &text)?;
    let changes_text = text != heard;
    let confidence = candidate.confidence.as_ref().and_then(confidence_of);
    let confidence = match (confidence, changes_text) {
        (Some(confidence), _) => confidence,
        (None, true) => CHANGED_CONFIDENCE,
        (None, false) => return Ok(None),
    };
    let alternatives = alternatives_of(&candidate.alternatives, &heard, &text);
    let why = candidate
        .why
        .map(|why| why.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|why| !why.is_empty())
        .map(|why| why.chars().take(WHY_MAX_CHARACTERS).collect());
    Ok(Some(Flag {
        words,
        text,
        confidence,
        alternatives,
        why,
        fixes,
        changes_text,
    }))
}

/// Where `heard` runs among `line` (whose first word is word `offset` of the transcript), as a
/// range of the transcript's words, compared word by word as fixes are.
fn find_run(line: &[Word], heard: &str, offset: usize) -> Result<Range<usize>, Refusal> {
    let wanted: Vec<String> = heard.split_whitespace().map(comparable).collect();
    if wanted.is_empty() || wanted.len() > line.len() {
        return Err(Refusal::NotInLine);
    }
    let keys: Vec<String> = line.iter().map(|word| comparable(&word.text)).collect();
    let mut found = keys
        .windows(wanted.len())
        .enumerate()
        .filter(|(_start, window)| *window == wanted.as_slice())
        .map(|(start, _window)| start);
    let start = found.next().ok_or(Refusal::NotInLine)?;
    if found.next().is_some() {
        return Err(Refusal::Ambiguous);
    }
    Ok(offset + start..offset + start + wanted.len())
}

/// A confidence as the model answered it, a number rounded and clamped to 0 to 100; `None` for
/// anything that is no number.
fn confidence_of(value: &Value) -> Option<u8> {
    let number = value.as_f64().filter(|number| number.is_finite())?;
    let clamped = number.round().clamp(0.0, 100.0);
    // A whole number from 0 to 100 converts exactly.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=100 and rounded, so it fits a u8 exactly"
    )]
    let confidence = clamped as u8;
    Some(confidence)
}

/// The alternatives the model answered that hold up for words that read `heard` and that it
/// settled as `text`: text, each once, none what it decided on, each only fixing words of
/// `heard`, and at most [`ALTERNATIVES_MAX`].
fn alternatives_of(answered: &[Value], heard: &str, text: &str) -> Vec<String> {
    let mut seen = HashSet::from([keys_text(text)]);
    let mut kept = Vec::new();
    for alternative in answered.iter().filter_map(Value::as_str) {
        let alternative = alternative.split_whitespace().collect::<Vec<_>>().join(" ");
        if alternative.is_empty()
            || alternative.chars().count() > ALTERNATIVE_MAX_CHARACTERS
            || check_fix(heard, &alternative).is_err()
            || !seen.insert(keys_text(&alternative))
        {
            continue;
        }
        kept.push(alternative);
        if kept.len() == ALTERNATIVES_MAX {
            break;
        }
    }
    kept
}

/// A run of the transcript whose text differs from what a capture heard over it, which no flag
/// covers: marked changed at [`CHANGED_CONFIDENCE`], with what the captures heard there. A capture
/// that heard nothing there gives no alternative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// As a range of the transcript's words.
    pub words: Range<usize>,
    /// What the captures heard over it, each once, at most [`ALTERNATIVES_MAX`].
    pub alternatives: Vec<String>,
}

impl Changed {
    /// Its confidence mark's data, as the meeting's page reads it.
    pub fn mark(&self) -> Value {
        serde_json::json!({ "confidence": CHANGED_CONFIDENCE, "alternatives": self.alternatives })
    }
}

/// One run of a line whose words differ from what one capture heard over it.
#[derive(Debug, Clone)]
struct Differing {
    /// As a range of the transcript's words.
    words: Range<usize>,
    /// The capture's version that heard it otherwise.
    version: usize,
    /// What that version heard there: empty for words it heard nothing in place of.
    heard: String,
}

/// Every run of `transcript` whose words differ from what one of its captures heard over the same
/// line, compared word by word as fixes are, that none of `flags` overlaps and no person's edit
/// wrote: words the capture heard as other words, and words it heard nothing in place of between
/// words it agreed on. Runs that overlap, as two captures that heard a name differently give, are
/// one, and each capture's reading of it is one of its alternatives.
///
/// Words a capture left out at a line's start or end are not counted, since the capture may have
/// placed them on the line beside it, or not yet or no longer been recording. Words a capture heard
/// that the transcript does not hold are not either: there is no word of the transcript to mark,
/// and they are mostly the fillers a source's own transcript tidies away. The prompt shows the
/// model both, under each line.
pub fn unflagged_changes(transcript: &Transcript<'_>, flags: &[Flag]) -> Vec<Changed> {
    let mut changed = Vec::new();
    for (number, line) in transcript.lines.iter().enumerate() {
        let said_keys: Vec<String> = transcript.words[line.clone()]
            .iter()
            .map(|word| comparable(&word.text))
            .collect();
        let mut differing = Vec::new();
        for version in 0..transcript.versions.len() {
            let heard = transcript.heard_over(version, number);
            if heard.is_empty() {
                continue;
            }
            let heard_keys: Vec<String> = heard.iter().map(|word| comparable(&word.text)).collect();
            let operations =
                similar::capture_diff_slices(Algorithm::Myers, &said_keys, &heard_keys);
            let agreed_around = |index: usize| {
                index > 0
                    && matches!(operations[index - 1], DiffOp::Equal { .. })
                    && matches!(operations.get(index + 1), Some(DiffOp::Equal { .. }))
            };
            for (index, operation) in operations.iter().enumerate() {
                let (old_index, old_len, heard_run) = match *operation {
                    DiffOp::Replace {
                        old_index,
                        old_len,
                        new_index,
                        new_len,
                    } => (old_index, old_len, new_index..new_index + new_len),
                    DiffOp::Delete {
                        old_index,
                        old_len,
                        new_index,
                    } if agreed_around(index) => (old_index, old_len, new_index..new_index),
                    _ => continue,
                };
                let words = line.start + old_index..line.start + old_index + old_len;
                let flagged = flags
                    .iter()
                    .any(|flag| flag.words.start < words.end && words.start < flag.words.end);
                let edited = transcript.words[words.clone()]
                    .iter()
                    .any(|word| word.origin == Origin::Edit.as_str());
                if flagged || edited {
                    continue;
                }
                let heard = heard[heard_run]
                    .iter()
                    .map(|word| word.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                differing.push(Differing {
                    words,
                    version,
                    heard,
                });
            }
        }
        changed.extend(merged(transcript, differing));
    }
    changed
}

/// `differing` runs of one line merged where they overlap, each merged run's alternatives the run as
/// each capture heard it: the transcript's words with that capture's runs in place of its own.
fn merged(transcript: &Transcript<'_>, mut differing: Vec<Differing>) -> Vec<Changed> {
    differing.sort_by_key(|run| (run.words.start, run.version));
    let mut groups: Vec<(Range<usize>, Vec<Differing>)> = Vec::new();
    for run in differing {
        match groups.last_mut() {
            Some((span, members)) if run.words.start < span.end => {
                span.end = span.end.max(run.words.end);
                members.push(run);
            }
            _ => groups.push((run.words.clone(), vec![run])),
        }
    }
    groups
        .into_iter()
        .map(|(span, members)| {
            let said = words_text(&transcript.words[span.clone()]);
            let mut seen = HashSet::from([keys_text(&said)]);
            let mut alternatives = Vec::new();
            let mut versions: Vec<usize> = members.iter().map(|run| run.version).collect();
            versions.sort_unstable();
            versions.dedup();
            for version in versions {
                let own: Vec<&Differing> = members
                    .iter()
                    .filter(|run| run.version == version)
                    .collect();
                let mut reading: Vec<String> = Vec::new();
                let mut index = span.start;
                while index < span.end {
                    if let Some(run) = own.iter().find(|run| run.words.start == index) {
                        if !run.heard.is_empty() {
                            reading.push(run.heard.clone());
                        }
                        index = run.words.end;
                    } else {
                        reading.push(transcript.words[index].text.clone());
                        index += 1;
                    }
                }
                let reading = reading.join(" ");
                // A capture that heard nothing over the whole run offers nothing to put there.
                if !reading.is_empty()
                    && alternatives.len() < ALTERNATIVES_MAX
                    && seen.insert(keys_text(&reading))
                {
                    alternatives.push(reading);
                }
            }
            Changed {
                words: span,
                alternatives,
            }
        })
        .collect()
}

/// A word as fixes are compared: lowercase, without the punctuation around it, so a fix of
/// case or a comma is no change of words.
pub(super) fn comparable(word: &str) -> String {
    word.trim_matches(|character: char| !character.is_alphanumeric())
        .to_lowercase()
}

/// `text`'s words as fixes compare them, joined.
fn keys_text(text: &str) -> String {
    text.split_whitespace()
        .map(comparable)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `words` read as text, a space between each.
fn words_text(words: &[Word]) -> String {
    words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
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

/// Where each word of the first capture stands in that capture's own lines: for every standing
/// word of `words` the capture `capture_id` heard (its source's or Whisper's, as its lines `base`
/// give them on the meeting's clock, `offset_ms` from it), its line's index and its place in the
/// line. A word two of the capture's lines would build alike is left out, since it could be either.
pub fn places_in_lines(
    base: &Base,
    whisper: &[WhisperWord],
    offset_ms: i64,
    capture_id: Uuid,
    words: &[Word],
) -> HashMap<Uuid, (usize, usize)> {
    /// A word as the capture's lines build it: its text and its times.
    type Key = (String, i64, i64);

    let origin = match base.based_on {
        BasedOn::Source => Origin::Source,
        BasedOn::Whisper => Origin::Whisper,
    };
    let mut built: HashMap<Key, Option<(usize, usize)>> = HashMap::new();
    for (line_index, line) in base.lines.iter().enumerate() {
        let line_words = alignment::capture_words([(line, origin)], whisper, offset_ms);
        for (word_index, word) in line_words.into_iter().enumerate() {
            let key = (word.text, word.start_ms, word.end_ms);
            built
                .entry(key)
                .and_modify(|place| *place = None)
                .or_insert(Some((line_index, word_index)));
        }
    }
    words
        .iter()
        .filter(|word| word.capture_id == Some(capture_id) && word.origin == origin.as_str())
        .filter_map(|word| {
            let key = (word.text.clone(), word.start_ms, word.end_ms);
            built
                .get(&key)
                .copied()
                .flatten()
                .map(|place| (word.id, place))
        })
        .collect()
}

/// The first capture's lines `base` with every flag of `flags` that changes text and lies inside
/// one of them applied: the corrected copy the steps after correction read, every line kept, in
/// order, with its speaker and times, only `text` differing. `places` is where each of `words`
/// (the transcript the flags were made over) of the capture stands in its lines
/// ([`places_in_lines`]); a flag over words of another capture, or across two lines, leaves the
/// copy as it was. Answers the copy and how many lines it changed.
pub fn corrected_copy<'flags>(
    base: &Base,
    places: &HashMap<Uuid, (usize, usize)>,
    words: &[Word],
    flags: impl IntoIterator<Item = &'flags Flag>,
) -> (Vec<TranscriptLine>, usize) {
    let mut line_words: Vec<Option<Vec<String>>> = vec![None; base.lines.len()];
    // Applied from each line's end, so an earlier flag's place in it still holds.
    let mut in_lines: Vec<(usize, usize, usize, &Flag)> = flags
        .into_iter()
        .filter(|flag| flag.changes_text)
        .filter_map(|flag| {
            let spots: Vec<(usize, usize)> = words[flag.words.clone()]
                .iter()
                .map(|word| places.get(&word.id).copied())
                .collect::<Option<_>>()?;
            let (line, first) = spots[0];
            let consecutive = spots
                .iter()
                .zip(0..)
                .all(|((spot_line, spot), step)| *spot_line == line && *spot == first + step);
            consecutive.then_some((line, first, spots.len(), flag))
        })
        .collect();
    in_lines.sort_by_key(|(line, first, _count, _flag)| std::cmp::Reverse((*line, *first)));
    for (line, first, count, flag) in in_lines {
        let words = line_words[line].get_or_insert_with(|| {
            base.lines[line]
                .text
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect()
        });
        if first + count > words.len() {
            continue;
        }
        words.splice(
            first..first + count,
            flag.text.split_whitespace().map(ToOwned::to_owned),
        );
    }
    let mut changed = 0;
    let copy = base
        .lines
        .iter()
        .zip(line_words)
        .map(|(line, words)| {
            let Some(words) = words else {
                return line.clone();
            };
            let text = words.join(" ");
            if text != line.text {
                changed += 1;
            }
            TranscriptLine {
                text,
                ..line.clone()
            }
        })
        .collect();
    (copy, changed)
}

/// Every distinct fix among `flags`, in the order they were made.
pub fn distinct_fixes<'flags>(flags: impl IntoIterator<Item = &'flags Flag>) -> Vec<Fix> {
    let mut seen = HashSet::new();
    let mut distinct = Vec::new();
    for fix in flags.into_iter().flat_map(|flag| &flag.fixes) {
        if seen.insert(fix) {
            distinct.push(fix.clone());
        }
    }
    distinct
}

/// The SHA-256 of `lines` serialized as they are stored, in lowercase hex, as the source's
/// transcript is hashed (`super::fetch`).
pub fn transcript_hash(lines: &[TranscriptLine]) -> String {
    let serialized = serde_json::to_vec(lines).expect("transcript lines are plain fields");
    hex::encode(Sha256::digest(serialized))
}

#[derive(Debug, Deserialize)]
struct JudgmentAnswer {
    #[serde(default)]
    visibility: Option<VisibilityAnswer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisibilityAnswer {
    #[serde(default)]
    private: Option<bool>,
    #[serde(default)]
    share_with: Vec<Value>,
    #[serde(default)]
    why: Option<String>,
}

/// Who the answer judged should read the meeting, of `people`, those it was offered. Anything that
/// cannot be read as a judgment is [`Judgment::Undecided`], which keeps the meeting private; a
/// person it names is kept only when offered, once, and at most [`SHARE_MAX`] of them, and its
/// reason is cut to [`visibility::WHY_MAX_CHARACTERS`].
pub fn parse_judgment(answer: &str, people: &[Person]) -> Judgment {
    let Some(json) = json_object(answer) else {
        return Judgment::Undecided;
    };
    let Ok(parsed) = serde_json::from_str::<JudgmentAnswer>(json) else {
        return Judgment::Undecided;
    };
    let Some(visibility) = parsed.visibility else {
        return Judgment::Undecided;
    };
    match visibility.private {
        Some(false) => Judgment::Open,
        Some(true) => {
            let offered: HashSet<Uuid> = people.iter().map(|person| person.id).collect();
            let mut share_with: Vec<Uuid> = Vec::new();
            for named in &visibility.share_with {
                let Some(id) = named.as_str().and_then(|id| id.trim().parse::<Uuid>().ok()) else {
                    continue;
                };
                if offered.contains(&id) && !share_with.contains(&id) {
                    share_with.push(id);
                }
            }
            share_with.truncate(SHARE_MAX);
            let why = visibility
                .why
                .map(|why| why.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|why| !why.is_empty())
                .map(|why| why.chars().take(visibility::WHY_MAX_CHARACTERS).collect());
            Judgment::Private { share_with, why }
        }
        None => Judgment::Undecided,
    }
}

/// The first `{` to the last `}`: models sometimes wrap JSON in a sentence or a code fence.
fn json_object(answer: &str) -> Option<&str> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    (end > start).then(|| &answer[start..=end])
}

#[cfg(test)]
mod tests;
