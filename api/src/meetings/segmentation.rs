// Copyright © 2026 Jalapeno Labs

//! Reading what matters out of a meeting's transcript, and cutting a conversation's messages into
//! topic segments, each attributed to the projects it is about. Pure: the prompts Elysia reads,
//! the checks her answers have to pass, and where a clip is cut.
//!
//! Segmentation is one text turn on the runner (`crate::runner::TurnMode::Text`): no tools,
//! no settings, only the transcript and the workspace's projects in the prompt and a JSON
//! answer back. A meeting is answered by the numbers of its lines ([`parse_excerpts_answer`]):
//! the excerpts it takes into projects, as tight as they can be, overlapping and nesting as they
//! need to, and the black-outs, the lines that matter to nobody. A meeting in words alone is read
//! the same way, without its times. A conversation is answered by the numbers of its messages
//! ([`parse_conversation_answer`]), since a chat's topics are runs of messages rather than
//! stretches of time. A conversation's answer also names the promises its person made in it
//! ("I'll let you know here when it's live"), each by the message it was made in, for its Project
//! Manager to follow up (`docs/engine.md`, Follow-ups).
//! An action item that arrived in no project is read the same way, without segments: its answer
//! names the projects it belongs to, surest first ([`parse_item_answer`]), the surest to lead it.
//! Nothing either answer says is taken on trust: only lines and messages that were shown are
//! kept, only attributions to projects that were offered, and only promises made in a message that
//! was shown, so a model's mistake can drop an excerpt or a promise but never invent a project, a
//! line, or a message.
//!
//! # Size
//!
//! The runner hands `claude` its prompt on standard input, which no argument limit bounds, so
//! a meeting's transcript goes whole, one numbered line per line of it ([`excerpts_prompt`]), and
//! a conversation's messages go whole too ([`messages_text`]), as do the projects they may be
//! about. Nothing is merged or shortened to fit. A transcript too long for the model's context
//! fails the step for good, with the CLI's reason, since every attempt would read the same prompt.
//!
//! # Clips
//!
//! An excerpt's span comes from its words. When Whisper heard the recording, its word timings are
//! exact to the audio, so a clip is cut at the words nearest the excerpt's ends ([`clip_span`]) and
//! starts and stops between words rather than inside one.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::source::TranscriptLine;
use crate::models::promise::{RECIPIENT_MAX_CHARACTERS, SUMMARY_MAX_CHARACTERS};
use crate::whisper::WhisperWord;

/// How much of a project's description the prompt shows: its start says what the project is,
/// which is all placing a topic on it needs.
const DESCRIPTION_CHARACTERS: usize = 200;

/// The most segments one conversation is cut into. A segment is a topic, not a message.
pub const MAX_SEGMENTS: usize = 40;

/// The most excerpts one meeting's answer keeps. An excerpt is what matters to a project, which a
/// long meeting may say several times over for each of its projects, but not hundreds of times.
pub const MAX_EXCERPTS: usize = 80;

/// The most black-outs one meeting's answer keeps.
pub const MAX_BLACK_OUTS: usize = 200;

/// The longest title and summary kept.
const TITLE_CHARACTERS: usize = 200;
const SUMMARY_CHARACTERS: usize = 1_000;

/// The most promises one conversation's answer keeps. A person promises a handful of things in a
/// conversation, not dozens; more is the model reading every reply as one.
pub const MAX_PROMISES: usize = 10;

/// The least confidence an attribution needs to be kept. Below it the model is guessing.
const MIN_CONFIDENCE: f32 = 0.3;

/// How far a clip's end may move to meet a word Whisper heard. A source's line times and
/// Whisper's word times agree to a second or two; a word farther away than this belongs to
/// speech on the other side of a silence Whisper skipped, so the transcript's time is kept.
const SNAP_TOLERANCE_MS: i64 = 3_000;

/// The meeting's segmentation turn's instructions: excerpts and black-outs as runs of its
/// numbered lines.
pub const EXCERPTS_SYSTEM_PROMPT: &str = "\
You read a meeting's transcript and take from it only what matters to the listed projects, and \
black out what matters to nobody. Its lines are numbered from 1. Answer with one JSON object and \
nothing else: \
{\"excerpts\": [{\"firstLine\": number, \"lastLine\": number, \"title\": string, \
\"summary\": string, \"projects\": [{\"projectId\": string, \"confidence\": number}]}], \
\"blackOuts\": [{\"firstLine\": number, \"lastLine\": number}]}. \
An excerpt is the lines that matter to a project, as few as still make sense on their own: a \
decision, a request, a problem, a plan, or news of the project's work, not the stretch of \
meeting around it. Excerpts may overlap and nest: one line can be an excerpt on one project \
inside a longer excerpt on another. Hellos, small talk, and asides belong to no excerpt. The \
title is a short phrase. The summary is one to three plain sentences of what was said and \
decided, with any action items and who owns them. Name only projects from the list, by their \
id, with a confidence from 0 to 1; what no listed project is about is no excerpt. \
A black-out is lines that truly matter to nobody, such as \"is my mic on?\", \"can you see my \
screen?\", or \"how was your weekend?\"; anything someone might search for later is not one, \
and when unsure, leave it. A black-out never covers a line of an excerpt. A line reading \
[blacked out] was blacked out by a person; leave it as it is. Answer with empty lists when \
nothing in the meeting matters to a project.";

/// A conversation's segmentation turn's instructions: segments as runs of numbered messages, and
/// the promises its person made in them.
pub const CONVERSATION_SYSTEM_PROMPT: &str = "\
You divide a conversation's messages into its topics, say which of the listed projects each \
topic is about, and find the promises the person you work for made in it. Answer with one \
JSON object and nothing else: \
{\"segments\": [{\"firstMessage\": number, \"lastMessage\": number, \"title\": string, \
\"summary\": string, \"projects\": [{\"projectId\": string, \"confidence\": number}]}], \
\"promises\": [{\"message\": number, \"recipient\": string, \"promise\": string, \
\"projectId\": string}]}. \
Messages are numbered from 1. Segments are in order and do not overlap; a conversation about \
one thing is one segment. The title is a short phrase. The summary is one to three plain \
sentences of what was asked, said, and decided, with any action items and who owns them. \
Name only projects from the list, by their id, with a confidence from 0 to 1, and leave \
projects empty for small talk or anything no project is about. \
A promise is the person telling someone they will let them know once some work is done, such \
as \"I'll ping you here when the fix is live\" or \"will let you know once it ships\". Only the \
person's own messages hold their promises; a request to them, or a promise someone else made, \
is not one. message is the number of the message that makes it, recipient who it is for, by \
the name the conversation uses, promise what they will tell them, as a short phrase such as \
\"when the login fix is live\", and projectId the listed project whose work it waits on. Leave \
promises empty when the person promised nothing, or when no listed project's work keeps it.";

/// A project the model may attribute a segment to.
#[derive(Debug, Clone)]
pub struct ProjectChoice {
    pub id: Uuid,
    pub name: String,
    pub description: String,
}

/// A run of a transcript's lines, by their place from 0, first and last included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRun {
    pub first: usize,
    pub last: usize,
}

impl LineRun {
    /// Whether this run shares a line with `other`.
    const fn overlaps(self, other: Self) -> bool {
        self.first <= other.last && other.first <= self.last
    }
}

/// One excerpt a meeting's answer found: a run of its lines, named, on its projects.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundExcerpt {
    pub lines: LineRun,
    pub title: String,
    pub summary: String,
    pub projects: Vec<SegmentProject>,
}

/// What a meeting's answer holds that holds up: its excerpts, and its black-outs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeetingReading {
    pub excerpts: Vec<FoundExcerpt>,
    pub black_outs: Vec<LineRun>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentProject {
    pub project_id: Uuid,
    pub confidence: f32,
}

/// One topic of a conversation: a run of its messages, by their place in the conversation from
/// 0, first and last included.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationSegment {
    pub first: usize,
    pub last: usize,
    pub title: String,
    pub summary: String,
    pub projects: Vec<SegmentProject>,
}

/// A promise a conversation's answer found: the person will tell `recipient` something once the
/// work of project `project_id` is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundPromise {
    /// The message it was made in, by its place in the conversation from 0.
    pub message: usize,
    pub recipient: String,
    /// What they will tell them, as a short phrase.
    pub summary: String,
    pub project_id: Uuid,
}

/// What a conversation's answer holds that holds up: its topics, and the promises made in it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationReading {
    pub segments: Vec<ConversationSegment>,
    pub promises: Vec<FoundPromise>,
}

/// The meeting as the prompt describes it.
#[derive(Debug, Clone, Copy)]
pub struct MeetingContext<'meeting> {
    pub title: &'meeting str,
    pub duration_ms: i64,
    pub participants: &'meeting [String],
}

/// One line of a meeting's transcript as its segmentation turn reads it: when it starts on the
/// meeting's clock, who said it, and what, its blacked-out words already left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptLine {
    pub start_ms: i64,
    pub speaker: String,
    pub text: String,
}

/// The meeting's segmentation turn's prompt: its lines numbered from 1, each with its time when
/// the meeting has a clock (`timed`), for [`parse_excerpts_answer`] to read.
pub fn excerpts_prompt(
    meeting: MeetingContext<'_>,
    lines: &[PromptLine],
    timed: bool,
    projects: &[ProjectChoice],
) -> String {
    let mut prompt = String::new();
    let _ = writeln!(prompt, "Meeting: {}", meeting.title);
    if timed {
        let _ = writeln!(prompt, "Length: {}", clock(meeting.duration_ms));
    }
    let _ = writeln!(prompt, "Lines: {}", lines.len());
    if !meeting.participants.is_empty() {
        let _ = writeln!(prompt, "Participants: {}", meeting.participants.join(", "));
    }
    push_projects(&mut prompt, projects);
    if timed {
        prompt.push_str("\nTranscript, each line numbered and starting at [minutes:seconds]:\n");
    } else {
        prompt.push_str("\nTranscript, each line numbered:\n");
    }
    for (line, number) in lines.iter().zip(1..) {
        let speaker = if line.speaker.trim().is_empty() {
            "Someone"
        } else {
            line.speaker.as_str()
        };
        let _ = if timed {
            writeln!(
                prompt,
                "[{number}] [{}] {speaker}: {}",
                clock(line.start_ms),
                line.text
            )
        } else {
            writeln!(prompt, "[{number}] {speaker}: {}", line.text)
        };
    }
    prompt
}

/// A conversation's segmentation turn's prompt: `title` is what the conversation is called
/// where it was had, `person` the name of whose timeline it is on, the one whose promises the
/// answer names, and `messages` are its messages in order, each timed from the first.
pub fn conversation_prompt(
    title: &str,
    person: &str,
    messages: &[TranscriptLine],
    projects: &[ProjectChoice],
) -> String {
    let mut participants: Vec<&str> = Vec::new();
    for message in messages {
        let named = !message.speaker.is_empty();
        if named && !participants.contains(&message.speaker.as_str()) {
            participants.push(&message.speaker);
        }
    }
    let mut prompt = String::new();
    let _ = writeln!(prompt, "Conversation: {title}");
    let _ = writeln!(
        prompt,
        "The person you work for: {person}, who may go by another name in the conversation"
    );
    let _ = writeln!(prompt, "Messages: {}", messages.len());
    if !participants.is_empty() {
        let _ = writeln!(prompt, "Participants: {}", participants.join(", "));
    }
    push_projects(&mut prompt, projects);
    prompt
        .push_str("\nMessages, each numbered and starting at [minutes:seconds] after the first:\n");
    prompt.push_str(&messages_text(messages));
    prompt
}

/// Every project a segment may be attributed to, each as its id, name, and the start of its
/// description. A project left off the list could never be attributed anything.
fn push_projects(prompt: &mut String, projects: &[ProjectChoice]) {
    prompt.push_str("\nProjects:\n");
    for project in projects {
        let description = truncated(project.description.trim(), DESCRIPTION_CHARACTERS);
        let _ = if description.is_empty() {
            writeln!(prompt, "- {}: {}", project.id, project.name)
        } else {
            writeln!(prompt, "- {}: {} ({description})", project.id, project.name)
        };
    }
    if projects.is_empty() {
        prompt.push_str("(none)\n");
    }
}

/// A conversation's messages as `[n] [m:ss] Speaker: text` lines, numbered from 1 and never
/// merged, since an answer names them by number, each whole on one line.
pub fn messages_text(messages: &[TranscriptLine]) -> String {
    numbered_text(messages, true)
}

/// Lines as `[n] [m:ss] Speaker: text`, numbered from 1, never merged, and each whole on one line;
/// without the `[m:ss]` when `with_clock` is false, for a meeting in words alone.
fn numbered_text(messages: &[TranscriptLine], with_clock: bool) -> String {
    let mut text = String::new();
    for (message, number) in messages.iter().zip(1..) {
        let speaker = if message.speaker.is_empty() {
            "Someone"
        } else {
            message.speaker.as_str()
        };
        // A message's own line breaks would read as lines of the conversation.
        let body = message
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let _ = if with_clock {
            writeln!(
                text,
                "[{number}] [{}] {speaker}: {body}",
                clock(message.start_ms)
            )
        } else {
            writeln!(text, "[{number}] {speaker}: {body}")
        };
    }
    text
}

/// How much of an action item's notes its prompt shows. An item's notes are the person's or a
/// provider's, and its title and issue carry most of what places it.
const ITEM_NOTES_CHARACTERS: usize = 2_000;

/// The most issues an item's prompt names. An item follows one or two; more are pull requests.
const ITEM_ISSUES_MAX: usize = 10;

/// An action item's attribution turn's instructions: the projects it belongs to, surest first.
pub const ITEM_SYSTEM_PROMPT: &str = "\
You say which of the listed projects an action item belongs to: the work it asks for, and the \
issue it follows when it has one. Answer with one JSON object and nothing else: \
{\"projects\": [{\"projectId\": string, \"confidence\": number}]}. \
Name only projects from the list, by their id, with a confidence from 0 to 1, surest first. The \
surest leads the item, so name first the project whose people would do the work. An issue in the \
place a project files its issues, or in one of its repositories, is very likely that project's. \
Leave projects empty when no listed project is about it.";

/// An action item as its attribution prompt describes it.
#[derive(Debug, Clone, Copy)]
pub struct ItemContext<'item> {
    pub title: &'item str,
    pub notes: &'item str,
    /// The issues and pull requests it follows, its primary first.
    pub issues: &'item [ItemIssue],
}

/// An issue an item follows, as the prompt names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemIssue {
    /// The provider's own name, such as `Jira`.
    pub provider: String,
    /// `ELY-12`, or `owner/name#12`.
    pub key: String,
    pub title: String,
    pub url: String,
    /// Where it was filed: a Jira project key, a GitHub repository, a YouTrack project.
    pub place: Option<String>,
}

/// Where a project's work is, which places an issue best: where it files its issues, such as
/// `Jira ELY`, and the repositories its workers clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWork {
    pub project_id: Uuid,
    pub issues_in: Option<String>,
    pub repositories: Vec<String>,
}

/// An action item's attribution turn's prompt: the item, the issues it follows, every project,
/// and where each project's work is.
pub fn item_prompt(
    item: ItemContext<'_>,
    projects: &[ProjectChoice],
    work: &[ProjectWork],
) -> String {
    let mut prompt = String::new();
    let _ = writeln!(
        prompt,
        "Action item: {}",
        one_line(item.title, TITLE_CHARACTERS)
    );
    let notes = truncated(item.notes.trim(), ITEM_NOTES_CHARACTERS);
    if !notes.is_empty() {
        let _ = writeln!(prompt, "Notes:\n{notes}");
    }
    if !item.issues.is_empty() {
        prompt.push_str("\nIt follows:\n");
    }
    for issue in item.issues.iter().take(ITEM_ISSUES_MAX) {
        let title = one_line(&issue.title, TITLE_CHARACTERS);
        let _ = match &issue.place {
            Some(place) => writeln!(
                prompt,
                "- {} {} in {place}: {title} ({})",
                issue.provider, issue.key, issue.url
            ),
            None => writeln!(
                prompt,
                "- {} {}: {title} ({})",
                issue.provider, issue.key, issue.url
            ),
        };
    }
    push_projects(&mut prompt, projects);

    let mut work_lines = String::new();
    for project in work {
        let mut whereabouts: Vec<String> = Vec::new();
        if let Some(issues_in) = &project.issues_in {
            whereabouts.push(format!("files issues in {issues_in}"));
        }
        if !project.repositories.is_empty() {
            whereabouts.push(format!("repositories {}", project.repositories.join(", ")));
        }
        if whereabouts.is_empty() {
            continue;
        }
        let _ = writeln!(
            work_lines,
            "- {}: {}",
            project.project_id,
            whereabouts.join("; ")
        );
    }
    if !work_lines.is_empty() {
        prompt.push_str("\nWhere each project's work is:\n");
        prompt.push_str(&work_lines);
    }
    prompt
}

/// Why an answer could not be used at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerError {
    #[error("the answer holds no JSON object")]
    NoJson,
    #[error("the answer is not the expected shape: {0}")]
    Shape(String),
    #[error("the answer holds no usable segment")]
    Empty,
}

/// A meeting's answer: its excerpts and black-outs as runs of its lines. Either may be left out,
/// as a meeting of small talk takes nothing.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExcerptsAnswer {
    #[serde(default)]
    excerpts: Vec<AnswerRun>,
    #[serde(default)]
    black_outs: Vec<AnswerRun>,
}

/// A run of numbered lines the answer names, with what an excerpt says of it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnswerRun {
    first_line: i64,
    last_line: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    projects: Vec<AnswerProject>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemAnswer {
    /// Optional, so an answer that names no project reads as an item no project is about.
    #[serde(default)]
    projects: Vec<AnswerProject>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnswerProject {
    project_id: String,
    confidence: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationAnswer {
    segments: Vec<ConversationAnswerSegment>,
    /// Optional, so an answer that names no promise still reads.
    #[serde(default)]
    promises: Vec<AnswerPromise>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnswerPromise {
    message: i64,
    #[serde(default)]
    recipient: String,
    #[serde(default)]
    promise: String,
    #[serde(default)]
    project_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationAnswerSegment {
    first_message: i64,
    last_message: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    projects: Vec<AnswerProject>,
}

/// The segments and promises in a conversation's answer that hold up. Segments are runs of its
/// messages, in order and apart, attributed only to projects in `offered`: a run starting before
/// the last one ended starts after it, and one reaching past the last message ends there. A
/// promise is kept when it names one of the messages, once each, someone it is for, what was
/// promised, and an offered project; at most [`MAX_PROMISES`] are.
///
/// # Errors
/// [`AnswerError`] when the answer holds no JSON object, is not the asked-for shape, or
/// leaves no segment standing.
pub fn parse_conversation_answer(
    answer: &str,
    messages: &[TranscriptLine],
    offered: &[ProjectChoice],
) -> Result<ConversationReading, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let parsed: ConversationAnswer =
        serde_json::from_str(json).map_err(|error| AnswerError::Shape(error.to_string()))?;
    let segments = kept_runs(parsed.segments, messages.len(), offered, |first| {
        format!("From {}", clock(messages[first].start_ms))
    });
    if segments.is_empty() {
        return Err(AnswerError::Empty);
    }
    let promises = kept_promises(parsed.promises, messages.len(), offered);
    Ok(ConversationReading { segments, promises })
}

/// The runs among `candidates`, of a sequence of `count` messages numbered from 1, that
/// hold up: in order and apart, a run starting before the last one ended starting after it, one
/// reaching past the end ending there, at most [`MAX_SEGMENTS`], and attributed only to projects
/// in `offered`. A run with no title takes `untitled` of its first, by its place from 0.
fn kept_runs(
    candidates: Vec<ConversationAnswerSegment>,
    count: usize,
    offered: &[ProjectChoice],
    untitled: impl Fn(usize) -> String,
) -> Vec<ConversationSegment> {
    let mut segments: Vec<ConversationSegment> = Vec::new();
    // The first message the next segment may start at, from 0.
    let mut next_free = 0;
    for candidate in candidates {
        if segments.len() == MAX_SEGMENTS || next_free >= count {
            break;
        }
        // Numbered from 1 in the answer; anything below that names no message.
        let (Ok(first), Ok(last)) = (
            usize::try_from(candidate.first_message - 1),
            usize::try_from(candidate.last_message - 1),
        ) else {
            continue;
        };
        let first = first.max(next_free);
        let last = last.min(count - 1);
        if last < first {
            continue;
        }
        let title = one_line(&candidate.title, TITLE_CHARACTERS);
        segments.push(ConversationSegment {
            first,
            last,
            title: if title.is_empty() {
                untitled(first)
            } else {
                title
            },
            summary: one_line(&candidate.summary, SUMMARY_CHARACTERS),
            projects: kept_projects(candidate.projects, offered),
        });
        next_free = last + 1;
    }
    segments
}

/// The excerpts and black-outs in a meeting's answer that hold up, among `line_count` numbered
/// lines: runs of lines that were shown (a run reaching past the last line ends there), each
/// excerpt once and on at least one of the projects in `offered`, at most [`MAX_EXCERPTS`]; each
/// black-out apart from every excerpt kept, those that overlap or touch joined into one, in the
/// order they lie, at most [`MAX_BLACK_OUTS`]. An excerpt with no title takes `untitled` of its first line, by its place
/// from 0. An answer taking nothing reads, as a meeting of small talk.
///
/// # Errors
/// [`AnswerError`] when the answer holds no JSON object or is not the asked-for shape.
pub fn parse_excerpts_answer(
    answer: &str,
    line_count: usize,
    offered: &[ProjectChoice],
    untitled: impl Fn(usize) -> String,
) -> Result<MeetingReading, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let parsed: ExcerptsAnswer =
        serde_json::from_str(json).map_err(|error| AnswerError::Shape(error.to_string()))?;
    let mut reading = MeetingReading::default();
    for candidate in parsed.excerpts {
        if reading.excerpts.len() == MAX_EXCERPTS {
            break;
        }
        let Some(lines) = line_run(candidate.first_line, candidate.last_line, line_count) else {
            continue;
        };
        if reading.excerpts.iter().any(|kept| kept.lines == lines) {
            continue;
        }
        let projects = kept_projects(candidate.projects, offered);
        if projects.is_empty() {
            continue;
        }
        let title = one_line(&candidate.title, TITLE_CHARACTERS);
        reading.excerpts.push(FoundExcerpt {
            lines,
            title: if title.is_empty() {
                untitled(lines.first)
            } else {
                title
            },
            summary: one_line(&candidate.summary, SUMMARY_CHARACTERS),
            projects,
        });
    }
    let mut black_outs: Vec<LineRun> = parsed
        .black_outs
        .iter()
        .filter_map(|candidate| line_run(candidate.first_line, candidate.last_line, line_count))
        .filter(|lines| {
            !reading
                .excerpts
                .iter()
                .any(|excerpt| excerpt.lines.overlaps(*lines))
        })
        .collect();
    // A stretch is blacked out once: runs that overlap or touch become one, which covers no line
    // that neither covered, so it still lies outside every excerpt.
    black_outs.sort_by_key(|lines| lines.first);
    for lines in black_outs {
        if let Some(kept) = reading.black_outs.last_mut()
            && lines.first <= kept.last + 1
        {
            kept.last = kept.last.max(lines.last);
            continue;
        }
        if reading.black_outs.len() == MAX_BLACK_OUTS {
            break;
        }
        reading.black_outs.push(lines);
    }
    Ok(reading)
}

/// The run from line `first` to line `last`, numbered from 1, of `count` lines, by their places
/// from 0: one reaching past the last line ends there, and one naming no line shown is `None`.
fn line_run(first: i64, last: i64, count: usize) -> Option<LineRun> {
    let first = usize::try_from(first.checked_sub(1)?).ok()?;
    let last = usize::try_from(last.checked_sub(1)?).ok()?;
    let last = last.min(count.checked_sub(1)?);
    (first <= last).then_some(LineRun { first, last })
}

/// The promises in `answered` that hold up, among a conversation of `message_count` messages.
fn kept_promises(
    answered: Vec<AnswerPromise>,
    message_count: usize,
    offered: &[ProjectChoice],
) -> Vec<FoundPromise> {
    let mut promises: Vec<FoundPromise> = Vec::new();
    for candidate in answered {
        if promises.len() == MAX_PROMISES {
            break;
        }
        // Numbered from 1 in the answer; anything outside the conversation names no message.
        let Ok(message) = usize::try_from(candidate.message - 1) else {
            continue;
        };
        let repeated = promises.iter().any(|kept| kept.message == message);
        if message >= message_count || repeated {
            continue;
        }
        let Ok(project_id) = Uuid::parse_str(candidate.project_id.trim()) else {
            continue;
        };
        if !offered.iter().any(|choice| choice.id == project_id) {
            continue;
        }
        // A name is never this long, so one that is is dropped rather than cut. A long promise
        // is cut to fit, its ellipsis included.
        let recipient = candidate
            .recipient
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let summary = one_line(&candidate.promise, SUMMARY_MAX_CHARACTERS - 1);
        let named = !recipient.is_empty() && recipient.chars().count() <= RECIPIENT_MAX_CHARACTERS;
        if !named || summary.is_empty() {
            continue;
        }
        promises.push(FoundPromise {
            message,
            recipient,
            summary,
            project_id,
        });
    }
    promises
}

/// The projects in an action item's answer that hold up, surest first, as [`kept_projects`] keeps
/// a segment's: offered, each once, sure enough. An answer naming none reads, as an item no project
/// is about.
///
/// # Errors
/// [`AnswerError`] when the answer holds no JSON object or is not the asked-for shape.
pub fn parse_item_answer(
    answer: &str,
    offered: &[ProjectChoice],
) -> Result<Vec<SegmentProject>, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let parsed: ItemAnswer =
        serde_json::from_str(json).map_err(|error| AnswerError::Shape(error.to_string()))?;
    let mut projects = kept_projects(parsed.projects, offered);
    // Stable, so the answer's own order breaks a tie.
    projects.sort_by(|left, right| right.confidence.total_cmp(&left.confidence));
    Ok(projects)
}

/// The attributions in `answered` that hold up: to projects in `offered`, each once, with a
/// confidence clamped to 0 to 1 and no less than [`MIN_CONFIDENCE`].
fn kept_projects(answered: Vec<AnswerProject>, offered: &[ProjectChoice]) -> Vec<SegmentProject> {
    let mut projects: Vec<SegmentProject> = Vec::new();
    for project in answered {
        let Ok(project_id) = Uuid::parse_str(project.project_id.trim()) else {
            continue;
        };
        let known = offered.iter().any(|choice| choice.id == project_id);
        let repeated = projects.iter().any(|kept| kept.project_id == project_id);
        if !known || repeated || !project.confidence.is_finite() {
            continue;
        }
        let confidence = project.confidence.clamp(0.0, 1.0);
        if confidence < MIN_CONFIDENCE {
            continue;
        }
        projects.push(SegmentProject {
            project_id,
            confidence,
        });
    }
    projects
}

/// Where an excerpt's clip is cut, from `start_ms` to `end_ms` of the recording: at the start of
/// the word Whisper heard nearest the excerpt's start and the end of the word nearest its end,
/// each within [`SNAP_TOLERANCE_MS`]. `words` are every word of the Whisper transcript in
/// order; with none, or none near an end, that end stays where the transcript put it. A span the
/// words would turn around is kept as the transcript gave it.
pub fn clip_span(start_ms: i64, end_ms: i64, words: &[WhisperWord]) -> (i64, i64) {
    let starts = nearest(words, start_ms, |word| word.start_ms);
    let ends = nearest(words, end_ms, |word| word.end_ms);
    let clip_start = starts.unwrap_or(start_ms);
    let clip_end = ends.unwrap_or(end_ms);
    if clip_end <= clip_start {
        return (start_ms, end_ms);
    }
    (clip_start, clip_end)
}

/// The time `at` of the word in `words` (in order) nearest `target`, when one is within
/// [`SNAP_TOLERANCE_MS`].
fn nearest(words: &[WhisperWord], target: i64, at: impl Fn(&WhisperWord) -> i64) -> Option<i64> {
    // Words are in order, so their starts and their ends both are: the nearest is on one side
    // or the other of where `target` would go.
    let after = words.partition_point(|word| at(word) < target);
    let candidates = [after.checked_sub(1), Some(after)];
    candidates
        .into_iter()
        .flatten()
        .filter_map(|index| words.get(index).map(&at))
        .filter(|time| (time - target).abs() <= SNAP_TOLERANCE_MS)
        .min_by_key(|time| (time - target).abs())
}

/// The first `{` to the last `}`: models sometimes wrap JSON in a sentence or a code fence.
fn json_object(answer: &str) -> Option<&str> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    (end > start).then(|| &answer[start..=end])
}

/// `milliseconds` as `m:ss`, or `h:mm:ss` from an hour.
pub fn clock(milliseconds: i64) -> String {
    let total = milliseconds.max(0) / 1000;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `text` on one line, its first `limit` characters.
fn one_line(text: &str, limit: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncated(&line, limit)
}

/// `text`'s first `limit` characters, with an ellipsis when it was cut.
fn truncated(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start_seconds: i64, speaker: &str, text: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms: start_seconds * 1000,
            end_ms: start_seconds * 1000 + 2000,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        }
    }

    fn project(name: &str) -> ProjectChoice {
        ProjectChoice {
            id: Uuid::now_v7(),
            name: name.to_owned(),
            description: format!("{name} description"),
        }
    }

    fn meeting_line(start_seconds: i64, speaker: &str, text: &str) -> PromptLine {
        PromptLine {
            start_ms: start_seconds * 1000,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        }
    }

    fn weekly_sync() -> MeetingContext<'static> {
        MeetingContext {
            title: "Weekly sync",
            duration_ms: 3_725_000,
            participants: &[],
        }
    }

    #[test]
    fn the_prompt_names_the_meeting_its_projects_and_every_line_numbered() {
        let launch = project("Launch site");
        let participants = ["Ada".to_owned(), "Grace".to_owned()];
        let prompt = excerpts_prompt(
            MeetingContext {
                participants: &participants,
                ..weekly_sync()
            },
            &[
                meeting_line(0, "Ada", "Hi."),
                meeting_line(65, "Grace", "Launch is Friday."),
                meeting_line(70, "", "[blacked out] agreed."),
            ],
            true,
            std::slice::from_ref(&launch),
        );

        assert!(prompt.contains("Meeting: Weekly sync"));
        assert!(prompt.contains("Length: 1:02:05\n"));
        assert!(prompt.contains("Lines: 3\n"));
        assert!(prompt.contains("Participants: Ada, Grace"));
        assert!(prompt.contains(&format!(
            "- {}: Launch site (Launch site description)",
            launch.id
        )));
        assert!(prompt.contains("[1] [0:00] Ada: Hi.\n"), "{prompt}");
        assert!(
            prompt.contains("[2] [1:05] Grace: Launch is Friday.\n"),
            "{prompt}"
        );
        assert!(
            prompt.contains("[3] [1:10] Someone: [blacked out] agreed.\n"),
            "{prompt}"
        );
    }

    #[test]
    fn a_meeting_in_words_alone_is_numbered_without_times() {
        let prompt = excerpts_prompt(
            weekly_sync(),
            &[
                meeting_line(0, "Ada", "Hello everyone."),
                meeting_line(0, "Ada", "Launch is Friday."),
            ],
            false,
            &[project("Launch")],
        );
        assert!(
            prompt.contains("[1] Ada: Hello everyone.\n[2] Ada: Launch is Friday.\n"),
            "{prompt}"
        );
        assert!(!prompt.contains("0:00"), "{prompt}");
        assert!(!prompt.contains("Length"), "{prompt}");
    }

    #[test]
    fn a_long_meetings_transcript_goes_whole() {
        // Two hours, every line long: several times what one argument could carry.
        let lines: Vec<PromptLine> = (0..2_400)
            .zip(["Ada", "Grace"].iter().cycle())
            .map(|(index, speaker)| {
                meeting_line(
                    index * 3,
                    speaker,
                    &format!("{index} {}", "word ".repeat(40)),
                )
            })
            .collect();

        let prompt = excerpts_prompt(weekly_sync(), &lines, true, &[project("Launch")]);

        assert!(prompt.len() > 3 * 128 * 1024, "{} bytes", prompt.len());
        for (line, number) in lines.iter().zip(1..) {
            assert!(
                prompt.contains(&format!(
                    "[{number}] [{}] {}: {}\n",
                    clock(line.start_ms),
                    line.speaker,
                    line.text
                )),
                "line {number} is whole"
            );
        }
    }

    #[test]
    fn every_project_is_offered_however_many_there_are() {
        let projects: Vec<ProjectChoice> = (0..300)
            .map(|index| project(&format!("Project {index}")))
            .collect();

        let prompt = excerpts_prompt(
            weekly_sync(),
            &[meeting_line(0, "Ada", "Hi.")],
            true,
            &projects,
        );

        for project in &projects {
            assert!(prompt.contains(&project.id.to_string()), "{}", project.name);
        }
    }

    fn numbered(first: usize) -> String {
        format!("From line {}", first + 1)
    }

    #[test]
    fn an_answer_keeps_only_excerpts_and_black_outs_that_hold_up() {
        let launch = project("Launch");
        let budget = project("Budget");
        let answer = format!(
            "Here you go:\n```json\n{{\"excerpts\": [\
              {{\"firstLine\": 2, \"lastLine\": 5, \"title\": \"  Launch moves \", \
                \"summary\": \"Friday.\", \
                \"projects\": [{{\"projectId\": \"{launch}\", \"confidence\": 1.4}}, \
                               {{\"projectId\": \"{stranger}\", \"confidence\": 0.9}}, \
                               {{\"projectId\": \"{launch}\", \"confidence\": 0.5}}]}}, \
              {{\"firstLine\": 3, \"lastLine\": 3, \"title\": \"\", \
                \"projects\": [{{\"projectId\": \"{budget}\", \"confidence\": 0.8}}]}}, \
              {{\"firstLine\": 2, \"lastLine\": 5, \"title\": \"Again\", \
                \"projects\": [{{\"projectId\": \"{budget}\", \"confidence\": 0.8}}]}}, \
              {{\"firstLine\": 7, \"lastLine\": 7, \"title\": \"Unsure\", \
                \"projects\": [{{\"projectId\": \"{launch}\", \"confidence\": 0.1}}]}}, \
              {{\"firstLine\": 0, \"lastLine\": 1, \"title\": \"Before the first\", \
                \"projects\": [{{\"projectId\": \"{launch}\", \"confidence\": 0.9}}]}}, \
              {{\"firstLine\": 8, \"lastLine\": 99, \"title\": \"Wrap up\", \
                \"projects\": [{{\"projectId\": \"{budget}\", \"confidence\": 0.6}}]}}\
            ], \"blackOuts\": [\
              {{\"firstLine\": 1, \"lastLine\": 1}}, \
              {{\"firstLine\": 4, \"lastLine\": 6}}, \
              {{\"firstLine\": 6, \"lastLine\": 7}}, \
              {{\"firstLine\": 7, \"lastLine\": 7}}\
            ]}}\n```",
            launch = launch.id,
            budget = budget.id,
            stranger = Uuid::now_v7(),
        );
        let offered = [launch.clone(), budget.clone()];
        let reading = parse_excerpts_answer(&answer, 10, &offered, numbered).expect("reads");

        assert_eq!(
            reading.excerpts,
            vec![
                // Clamped to 1, the stranger and the repeat dropped.
                FoundExcerpt {
                    lines: LineRun { first: 1, last: 4 },
                    title: "Launch moves".to_owned(),
                    summary: "Friday.".to_owned(),
                    projects: vec![SegmentProject {
                        project_id: launch.id,
                        confidence: 1.0,
                    }],
                },
                // Nested inside the first, on another project.
                FoundExcerpt {
                    lines: LineRun { first: 2, last: 2 },
                    title: "From line 3".to_owned(),
                    summary: String::new(),
                    projects: vec![SegmentProject {
                        project_id: budget.id,
                        confidence: 0.8,
                    }],
                },
                // Reaching past the last line, it ends there.
                FoundExcerpt {
                    lines: LineRun { first: 7, last: 9 },
                    title: "Wrap up".to_owned(),
                    summary: String::new(),
                    projects: vec![SegmentProject {
                        project_id: budget.id,
                        confidence: 0.6,
                    }],
                },
            ],
            "the same lines twice, a guess too unsure to keep, and a line 0 are dropped"
        );
        // One over an excerpt is dropped, and one over another black-out joins it.
        assert_eq!(
            reading.black_outs,
            vec![LineRun { first: 0, last: 0 }, LineRun { first: 5, last: 6 }]
        );
    }

    #[test]
    fn black_outs_that_overlap_or_touch_are_one() {
        let answer = "{\"excerpts\": [], \"blackOuts\": [\
            {\"firstLine\": 3, \"lastLine\": 3}, \
            {\"firstLine\": 5, \"lastLine\": 6}, \
            {\"firstLine\": 1, \"lastLine\": 2}, \
            {\"firstLine\": 2, \"lastLine\": 2}\
        ]}";
        let reading = parse_excerpts_answer(answer, 6, &[], numbered).expect("reads");

        assert_eq!(
            reading.black_outs,
            vec![LineRun { first: 0, last: 2 }, LineRun { first: 4, last: 5 }],
            "lines 1 to 3 are one black-out, and lines 5 and 6 another"
        );
    }

    #[test]
    fn an_answer_taking_nothing_reads_and_one_without_json_does_not() {
        assert_eq!(
            parse_excerpts_answer("{\"excerpts\": [], \"blackOuts\": []}", 4, &[], numbered),
            Ok(MeetingReading::default())
        );
        assert_eq!(
            parse_excerpts_answer("{}", 4, &[], numbered),
            Ok(MeetingReading::default())
        );
        assert_eq!(
            parse_excerpts_answer("Nothing here.", 4, &[], numbered),
            Err(AnswerError::NoJson)
        );
        assert!(matches!(
            parse_excerpts_answer("{\"excerpts\": \"none\"}", 4, &[], numbered),
            Err(AnswerError::Shape(_))
        ));
        // A meeting with no lines takes nothing, whatever the answer names.
        let launch = project("Launch");
        let answer = format!(
            "{{\"excerpts\": [{{\"firstLine\": 1, \"lastLine\": 1, \
                \"projects\": [{{\"projectId\": \"{}\", \"confidence\": 1}}]}}]}}",
            launch.id
        );
        assert_eq!(
            parse_excerpts_answer(&answer, 0, std::slice::from_ref(&launch), numbered),
            Ok(MeetingReading::default())
        );
    }

    fn word(start_ms: i64, end_ms: i64) -> WhisperWord {
        WhisperWord {
            start_ms,
            end_ms,
            text: " word".to_owned(),
            probability: 0.9,
        }
    }

    #[test]
    fn a_clip_is_cut_at_the_words_nearest_its_ends() {
        let words = [
            word(58_200, 58_900),
            word(60_400, 60_800),
            word(299_100, 299_700),
            word(301_900, 302_300),
        ];
        // The transcript's 60 s starts nearest the word at 60.4 s; its 300 s ends nearest the
        // word ending at 299.7 s, not the one after the pause.
        assert_eq!(clip_span(60_000, 300_000, &words), (60_400, 299_700));
    }

    #[test]
    fn a_clip_keeps_the_transcripts_times_where_whisper_heard_nothing_near() {
        assert_eq!(clip_span(60_000, 300_000, &[]), (60_000, 300_000));
        // Silence Whisper skipped around the start: the nearest word is ten seconds off.
        let words = [word(70_000, 70_500), word(299_000, 299_800)];
        assert_eq!(clip_span(60_000, 300_000, &words), (60_000, 299_800));
        // A span inside a pause: the nearest start is after the nearest end, which would turn it
        // around, so it stays as the transcript gave it.
        let around = [word(3_000, 3_100), word(3_400, 3_500)];
        assert_eq!(clip_span(3_250, 3_260, &around), (3_250, 3_260));
    }

    #[test]
    fn a_conversations_messages_are_numbered_and_never_merged() {
        let launch = project("Launch");
        let messages = [
            line(0, "Ada", "Can we ship\nFriday?"),
            line(40, "Ada", "Or Monday."),
            line(95, "Grace", "Friday works."),
        ];
        let prompt = conversation_prompt(
            "#launch in Mooreslab",
            "Alex Navarro",
            &messages,
            std::slice::from_ref(&launch),
        );

        assert!(prompt.contains("Conversation: #launch in Mooreslab"));
        assert!(prompt.contains("The person you work for: Alex Navarro"));
        assert!(prompt.contains("Messages: 3"));
        assert!(prompt.contains("Participants: Ada, Grace"));
        assert!(prompt.contains(&format!("- {}: Launch", launch.id)));
        assert!(prompt.contains("[1] [0:00] Ada: Can we ship Friday?\n"));
        assert!(prompt.contains("[2] [0:40] Ada: Or Monday.\n"));
        assert!(prompt.contains("[3] [1:35] Grace: Friday works.\n"));
    }

    #[test]
    fn a_long_conversations_messages_go_whole() {
        let messages: Vec<TranscriptLine> = (0..1_000)
            .map(|index| line(index, "Ada", &format!("{index} {}", "word ".repeat(80))))
            .collect();

        let text = messages_text(&messages);

        assert!(text.len() > 3 * 128 * 1024, "{} bytes", text.len());
        for (message, number) in messages.iter().zip(1..) {
            let body = message.text.trim_end();
            assert!(
                text.contains(&format!(
                    "[{number}] [{}] Ada: {body}\n",
                    clock(message.start_ms)
                )),
                "message {number} is whole"
            );
        }
    }

    #[test]
    fn a_conversations_answer_keeps_only_runs_of_its_messages() {
        let launch = project("Launch");
        let messages = [
            line(0, "Ada", "Hi."),
            line(5, "Ada", "Can we ship Friday?"),
            line(9, "Grace", "Yes."),
            line(30, "Grace", "Lunch?"),
        ];
        let answer = format!(
            "{{\"segments\": [\
              {{\"firstMessage\": 0, \"lastMessage\": 0, \"title\": \"Nothing\"}}, \
              {{\"firstMessage\": 1, \"lastMessage\": 3, \"title\": \"Launch\", \
                \"summary\": \"Ships Friday.\", \
                \"projects\": [{{\"projectId\": \"{launch}\", \"confidence\": 0.8}}]}}, \
              {{\"firstMessage\": 2, \"lastMessage\": 9, \"title\": \"\", \"summary\": \"\"}}, \
              {{\"firstMessage\": 4, \"lastMessage\": 4, \"title\": \"Late\"}}\
            ]}}",
            launch = launch.id,
        );
        let reading = parse_conversation_answer(&answer, &messages, std::slice::from_ref(&launch))
            .expect("segments");
        assert!(
            reading.promises.is_empty(),
            "an answer naming no promise reads"
        );
        let segments = reading.segments;

        assert_eq!(
            segments,
            vec![
                // Message 0 names nothing, so that run is dropped.
                ConversationSegment {
                    first: 0,
                    last: 2,
                    title: "Launch".to_owned(),
                    summary: "Ships Friday.".to_owned(),
                    projects: vec![SegmentProject {
                        project_id: launch.id,
                        confidence: 0.8,
                    }],
                },
                // It overlapped the first, so it starts after it, and ends with the last message;
                // the run after it has no message left to start at.
                ConversationSegment {
                    first: 3,
                    last: 3,
                    title: "From 0:30".to_owned(),
                    summary: String::new(),
                    projects: Vec::new(),
                },
            ]
        );
        assert_eq!(
            parse_conversation_answer("{\"segments\": []}", &messages, &[]),
            Err(AnswerError::Empty)
        );
    }

    #[test]
    fn a_conversations_promises_are_kept_only_when_they_hold_up() {
        let launch = project("Launch");
        let messages = [
            line(
                0,
                "Ada",
                "Can you fix the login bug? Let me know when it's live.",
            ),
            line(30, "Alex", "On it, I'll ping you here once it's deployed."),
            line(60, "Ada", "Thanks!"),
        ];
        let answer = format!(
            "{{\"segments\": [{{\"firstMessage\": 1, \"lastMessage\": 3, \"title\": \"Login\"}}], \
              \"promises\": [\
              {{\"message\": 2, \"recipient\": \"  Ada \", \"promise\": \"when the login fix is live\", \
                \"projectId\": \"{launch}\"}}, \
              {{\"message\": 2, \"recipient\": \"Ada\", \"promise\": \"again\", \"projectId\": \"{launch}\"}}, \
              {{\"message\": 4, \"recipient\": \"Ada\", \"promise\": \"past the end\", \"projectId\": \"{launch}\"}}, \
              {{\"message\": 0, \"recipient\": \"Ada\", \"promise\": \"before the start\", \"projectId\": \"{launch}\"}}, \
              {{\"message\": 1, \"recipient\": \"Ada\", \"promise\": \"elsewhere\", \"projectId\": \"{stranger}\"}}, \
              {{\"message\": 3, \"recipient\": \"\", \"promise\": \"for nobody\", \"projectId\": \"{launch}\"}}, \
              {{\"message\": 3, \"recipient\": \"Ada\", \"promise\": \" \", \"projectId\": \"{launch}\"}}, \
              {{\"message\": 3, \"recipient\": \"{long_name}\", \"promise\": \"too long\", \"projectId\": \"{launch}\"}}\
            ]}}",
            launch = launch.id,
            stranger = Uuid::now_v7(),
            long_name = "A".repeat(RECIPIENT_MAX_CHARACTERS + 1),
        );
        let reading = parse_conversation_answer(&answer, &messages, std::slice::from_ref(&launch))
            .expect("a reading");

        // Only the first holds up: the rest repeat its message, name none, name a project that
        // was not offered, name nobody, promise nothing, or name no one a person is called.
        assert_eq!(
            reading.promises,
            vec![FoundPromise {
                message: 1,
                recipient: "Ada".to_owned(),
                summary: "when the login fix is live".to_owned(),
                project_id: launch.id,
            }]
        );
    }

    #[test]
    fn a_conversations_promises_are_bounded() {
        let launch = project("Launch");
        let messages: Vec<TranscriptLine> = (0..20)
            .map(|index| line(index, "Alex", "I'll let you know."))
            .collect();
        let promises: Vec<String> = (1..=20)
            .map(|number| {
                format!(
                    "{{\"message\": {number}, \"recipient\": \"Ada\", \"promise\": \"{long}\", \
                      \"projectId\": \"{launch}\"}}",
                    long = "word ".repeat(200),
                    launch = launch.id,
                )
            })
            .collect();
        let answer = format!(
            "{{\"segments\": [{{\"firstMessage\": 1, \"lastMessage\": 20}}], \"promises\": [{}]}}",
            promises.join(", ")
        );
        let reading = parse_conversation_answer(&answer, &messages, std::slice::from_ref(&launch))
            .expect("a reading");

        assert_eq!(reading.promises.len(), MAX_PROMISES);
        for promise in &reading.promises {
            assert!(promise.summary.chars().count() <= SUMMARY_MAX_CHARACTERS);
            assert!(promise.summary.ends_with('…'));
        }
    }

    #[test]
    fn an_items_prompt_names_its_issues_and_where_each_projects_work_is() {
        let website = project("Website");
        let launch = project("Launch");
        let issues = [ItemIssue {
            provider: "Jira".to_owned(),
            key: "ELY-12".to_owned(),
            title: "Fix the login".to_owned(),
            url: "https://acme.atlassian.net/browse/ELY-12".to_owned(),
            place: Some("ELY".to_owned()),
        }];
        let work = [
            ProjectWork {
                project_id: website.id,
                issues_in: Some("Jira ELY".to_owned()),
                repositories: vec!["https://github.com/acme/site".to_owned()],
            },
            // A project with nowhere named is listed among the projects alone.
            ProjectWork {
                project_id: launch.id,
                issues_in: None,
                repositories: Vec::new(),
            },
        ];
        let prompt = item_prompt(
            ItemContext {
                title: "Fix the login",
                notes: "Safari logs people out.",
                issues: &issues,
            },
            &[website.clone(), launch.clone()],
            &work,
        );

        assert!(
            prompt.starts_with("Action item: Fix the login\nNotes:\nSafari logs people out.\n")
        );
        assert!(prompt.contains(
            "- Jira ELY-12 in ELY: Fix the login (https://acme.atlassian.net/browse/ELY-12)"
        ));
        assert!(prompt.contains(&format!("- {}: Website (Website description)", website.id)));
        assert!(prompt.contains(&format!(
            "- {}: files issues in Jira ELY; repositories https://github.com/acme/site",
            website.id
        )));
        let described = prompt
            .split("Where each project's work is:")
            .nth(1)
            .expect("a section");
        assert!(!described.contains(&launch.id.to_string()));
    }

    #[test]
    fn an_items_answer_keeps_offered_projects_surest_first() {
        let website = project("Website");
        let launch = project("Launch");
        let offered = [website.clone(), launch.clone()];
        let answer = format!(
            "Here you go: {{\"projects\": [\
             {{\"projectId\": \"{website}\", \"confidence\": 0.5}}, \
             {{\"projectId\": \"{launch}\", \"confidence\": 1.4}}, \
             {{\"projectId\": \"{launch}\", \"confidence\": 0.2}}, \
             {{\"projectId\": \"{unknown}\", \"confidence\": 0.9}}]}}",
            website = website.id,
            launch = launch.id,
            unknown = Uuid::now_v7(),
        );
        let projects = parse_item_answer(&answer, &offered).expect("reads");
        // Clamped to 1, the repeat and the unoffered project dropped, the surest first.
        assert_eq!(
            projects,
            vec![
                SegmentProject {
                    project_id: launch.id,
                    confidence: 1.0,
                },
                SegmentProject {
                    project_id: website.id,
                    confidence: 0.5,
                },
            ]
        );

        // An item no project is about reads, with or without the field.
        assert_eq!(
            parse_item_answer("{\"projects\": []}", &offered),
            Ok(Vec::new())
        );
        assert_eq!(parse_item_answer("{}", &offered), Ok(Vec::new()));
        assert_eq!(
            parse_item_answer("no idea", &offered),
            Err(AnswerError::NoJson)
        );
        assert!(matches!(
            parse_item_answer("{\"projects\": \"website\"}", &offered),
            Err(AnswerError::Shape(_))
        ));
    }

    #[test]
    fn clocks_read_like_a_player() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(65_000), "1:05");
        assert_eq!(clock(3_725_000), "1:02:05");
    }
}
