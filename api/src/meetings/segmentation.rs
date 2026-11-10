// Copyright © 2026 Jalapeno Labs

//! Cutting a meeting's transcript or a conversation's messages into topic segments, each
//! attributed to the projects it is about. Pure: the prompts Elysia reads, the checks her
//! answers have to pass, and where a segment's clip is cut.
//!
//! Segmentation is one text turn on the runner (`crate::runner::TurnMode::Text`): no tools,
//! no settings, only the transcript and the workspace's projects in the prompt and a JSON
//! answer back. A meeting is answered in seconds of its recording ([`parse_answer`]), and a
//! conversation by the numbers of its messages ([`parse_conversation_answer`]), since a chat's
//! topics are runs of messages rather than stretches of time. A conversation's answer also names
//! the promises its person made in it ("I'll let you know here when it's live"), each by the
//! message it was made in, for its Project Manager to follow up (`docs/engine.md`, Follow-ups).
//! A meeting in words alone, a transcript uploaded with no times and no recording, has no clock to
//! answer in, so it is answered by the numbers of its lines as a conversation is
//! ([`parse_lines_answer`]), each segment a run of its lines.
//! An action item that arrived in no project is read the same way, without segments: its answer
//! names the projects it belongs to, surest first ([`parse_item_answer`]), the surest to lead it.
//! Nothing either answer says is taken on trust: only segments that lie inside the meeting or the
//! conversation, in order and without overlapping, are kept, only attributions to projects that
//! were offered, and only promises made in a message that was shown, so a model's mistake can
//! drop a segment or a promise but never invent a project, a time, or a message.
//!
//! # Size
//!
//! The runner passes the prompt as one argument to `claude`, and Linux allows one argument
//! 128 KiB. [`transcript_text`] therefore merges each speaker's consecutive lines and, past
//! [`TRANSCRIPT_BUDGET_BYTES`], shortens every line evenly, so a long meeting still reaches
//! the model whole, only in less detail. A conversation's messages are shortened the same way.
//!
//! # Clips
//!
//! A segment's times come from the transcript, whose lines a source times to the second or
//! so. When Whisper heard the recording, its word timings are exact to the audio, so a clip is
//! cut at the words nearest the segment's ends ([`clip_span`]) and starts and stops between
//! words rather than inside one.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::source::TranscriptLine;
use crate::models::promise::{RECIPIENT_MAX_CHARACTERS, SUMMARY_MAX_CHARACTERS};
use crate::whisper::WhisperWord;

/// The most bytes of transcript one prompt carries, leaving room under the 128 KiB argument
/// limit for the instructions and the projects.
pub const TRANSCRIPT_BUDGET_BYTES: usize = 100_000;

/// The most bytes of projects one prompt carries: names, and descriptions cut short.
const PROJECTS_BUDGET_BYTES: usize = 12_000;

/// How much of a project's description the prompt shows.
const DESCRIPTION_CHARACTERS: usize = 200;

/// The shortest a line is cut to when a transcript is over budget.
const MIN_LINE_CHARACTERS: usize = 40;

/// The most segments one meeting is cut into. A segment is a topic, not a sentence.
pub const MAX_SEGMENTS: usize = 40;

/// The shortest segment kept, so a passing remark does not become a clip.
const MIN_SEGMENT_MS: i64 = 15_000;

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

/// The meeting's segmentation turn's instructions: segments in seconds of the recording.
pub const SYSTEM_PROMPT: &str = "\
You divide a meeting's transcript into its topics, the way a person skimming the recording \
would want it chaptered, and say which of the listed projects each topic is about. \
Answer with one JSON object and nothing else: \
{\"segments\": [{\"startSeconds\": number, \"endSeconds\": number, \"title\": string, \
\"summary\": string, \"projects\": [{\"projectId\": string, \"confidence\": number}]}]}. \
Segments are in order, do not overlap, and together cover the meeting; a topic usually runs \
for minutes, not seconds. The title is a short phrase. The summary is two to four plain \
sentences of what was said and decided, with any action items and who owns them. \
Name only projects from the list, by their id, with a confidence from 0 to 1, and leave \
projects empty for small talk or anything no project is about.";

/// A meeting in words alone's segmentation turn's instructions: segments as runs of its numbered
/// lines, since it has no clock.
pub const LINES_SYSTEM_PROMPT: &str = "\
You divide a meeting's transcript into its topics, the way a person skimming it would want it \
chaptered, and say which of the listed projects each topic is about. The transcript carries no \
times, so its lines are numbered from 1. Answer with one JSON object and nothing else: \
{\"segments\": [{\"firstLine\": number, \"lastLine\": number, \"title\": string, \
\"summary\": string, \"projects\": [{\"projectId\": string, \"confidence\": number}]}]}. \
Segments are in order, do not overlap, and together cover the meeting; a topic usually runs \
for many lines, not one. The title is a short phrase. The summary is two to four plain \
sentences of what was said and decided, with any action items and who owns them. \
Name only projects from the list, by their id, with a confidence from 0 to 1, and leave \
projects empty for small talk or anything no project is about.";

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

/// One topic of a meeting, as stored in `meetings.segments`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// On the capture's clock; both `0` for a meeting in words alone, which has none.
    pub start_ms: i64,
    pub end_ms: i64,
    pub title: String,
    pub summary: String,
    pub projects: Vec<SegmentProject>,
    /// The run of transcript lines a meeting in words alone's segment holds, by their place from
    /// 0, first and last included; `None` for a segment of a meeting on a clock, which its times
    /// say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<LineRun>,
}

/// A run of a transcript's lines, by their place from 0, first and last included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRun {
    pub first: usize,
    pub last: usize,
}

impl Segment {
    /// Whether `line`, the transcript's line at `index`, falls inside this segment: by its place
    /// for a meeting in words alone, and by its start otherwise.
    fn holds(&self, index: usize, line: &TranscriptLine) -> bool {
        match self.lines {
            Some(run) => (run.first..=run.last).contains(&index),
            None => line.start_ms >= self.start_ms && line.start_ms < self.end_ms,
        }
    }
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

/// The meeting's segmentation turn's prompt.
pub fn prompt(
    meeting: MeetingContext<'_>,
    transcript: &[TranscriptLine],
    projects: &[ProjectChoice],
) -> String {
    let mut prompt = String::new();
    let _ = writeln!(prompt, "Meeting: {}", meeting.title);
    let _ = writeln!(
        prompt,
        "Length: {} ({} seconds)",
        clock(meeting.duration_ms),
        meeting.duration_ms / 1000
    );
    if !meeting.participants.is_empty() {
        let _ = writeln!(prompt, "Participants: {}", meeting.participants.join(", "));
    }
    push_projects(&mut prompt, projects);
    prompt.push_str("\nTranscript, each line starting at [minutes:seconds]:\n");
    prompt.push_str(&transcript_text(transcript, TRANSCRIPT_BUDGET_BYTES));
    prompt
}

/// The segmentation turn's prompt for a meeting in words alone: its lines numbered, with no times,
/// for [`parse_lines_answer`] to read.
pub fn lines_prompt(
    meeting: MeetingContext<'_>,
    transcript: &[TranscriptLine],
    projects: &[ProjectChoice],
) -> String {
    let mut prompt = String::new();
    let _ = writeln!(prompt, "Meeting: {}", meeting.title);
    let _ = writeln!(prompt, "Lines: {}", transcript.len());
    if !meeting.participants.is_empty() {
        let _ = writeln!(prompt, "Participants: {}", meeting.participants.join(", "));
    }
    push_projects(&mut prompt, projects);
    prompt.push_str("\nTranscript, each line numbered:\n");
    prompt.push_str(&numbered_text(transcript, TRANSCRIPT_BUDGET_BYTES, false));
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
    prompt.push_str(&messages_text(messages, TRANSCRIPT_BUDGET_BYTES));
    prompt
}

/// The projects a segment may be attributed to, each as its id, name, and the start of its
/// description, within [`PROJECTS_BUDGET_BYTES`].
fn push_projects(prompt: &mut String, projects: &[ProjectChoice]) {
    prompt.push_str("\nProjects:\n");
    let mut projects_bytes = 0;
    for project in projects {
        let description = truncated(project.description.trim(), DESCRIPTION_CHARACTERS);
        let line = if description.is_empty() {
            format!("- {}: {}\n", project.id, project.name)
        } else {
            format!("- {}: {} ({description})\n", project.id, project.name)
        };
        projects_bytes += line.len();
        if projects_bytes > PROJECTS_BUDGET_BYTES {
            break;
        }
        prompt.push_str(&line);
    }
    if projects.is_empty() {
        prompt.push_str("(none)\n");
    }
}

/// A conversation's messages as `[n] [m:ss] Speaker: text` lines, numbered from 1 and never
/// merged, since an answer names them by number. Past `budget` bytes every message is shortened
/// evenly; a conversation too long for even that keeps its start and says where it was cut, and
/// its answer may still only name the messages shown.
pub fn messages_text(messages: &[TranscriptLine], budget: usize) -> String {
    numbered_text(messages, budget, true)
}

/// Lines as `[n] [m:ss] Speaker: text`, numbered from 1 and never merged, shortened as
/// [`messages_text`] says; without the `[m:ss]` when `with_clock` is false, for a meeting in words
/// alone.
fn numbered_text(messages: &[TranscriptLine], budget: usize, with_clock: bool) -> String {
    let prefixes: Vec<String> = messages
        .iter()
        .zip(1..)
        .map(|(message, number)| {
            let speaker = if message.speaker.is_empty() {
                "Someone"
            } else {
                message.speaker.as_str()
            };
            if with_clock {
                format!("[{number}] [{}] {speaker}: ", clock(message.start_ms))
            } else {
                format!("[{number}] {speaker}: ")
            }
        })
        .collect();
    let render = |limit: Option<usize>| {
        let mut rendered = String::new();
        for (message, prefix) in messages.iter().zip(&prefixes) {
            rendered.push_str(prefix);
            let text = message
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            match limit {
                Some(limit) => rendered.push_str(&truncated(&text, limit)),
                None => rendered.push_str(&text),
            }
            rendered.push('\n');
        }
        rendered
    };
    fit_to_budget(render, &prefixes, budget)
}

/// The transcript as `[mm:ss] Speaker: text` lines, one per run of a speaker's consecutive
/// lines, shortened evenly to fit `budget` bytes.
pub fn transcript_text(transcript: &[TranscriptLine], budget: usize) -> String {
    let mut turns: Vec<(i64, &str, String)> = Vec::new();
    for line in transcript {
        match turns.last_mut() {
            Some((_, speaker, text)) if *speaker == line.speaker => {
                text.push(' ');
                text.push_str(&line.text);
            }
            _ => turns.push((line.start_ms, line.speaker.as_str(), line.text.clone())),
        }
    }

    // Each turn's `[m:ss] Speaker: ` prefix, which is never shortened.
    let prefixes: Vec<String> = turns
        .iter()
        .map(|(start_ms, speaker, _)| {
            let speaker = if speaker.is_empty() {
                "Someone"
            } else {
                speaker
            };
            format!("[{}] {speaker}: ", clock(*start_ms))
        })
        .collect();
    let render = |limit: Option<usize>| {
        let mut rendered = String::new();
        for ((_, _, text), prefix) in turns.iter().zip(&prefixes) {
            rendered.push_str(prefix);
            match limit {
                Some(limit) => rendered.push_str(&truncated(text, limit)),
                None => rendered.push_str(text),
            }
            rendered.push('\n');
        }
        rendered
    };
    fit_to_budget(render, &prefixes, budget)
}

/// Lines rendered by `render` within `budget` bytes: whole when they fit, otherwise each line's
/// text cut to the same share of what its `prefixes` leave (`render(Some(share))`), and when even
/// that is too long, the start kept and a note saying where the rest was cut.
fn fit_to_budget(
    render: impl Fn(Option<usize>) -> String,
    prefixes: &[String],
    budget: usize,
) -> String {
    let whole = render(None);
    if whole.len() <= budget || prefixes.is_empty() {
        return whole;
    }
    // Every line's text gets the same share of what the prefixes leave, never less than a
    // short line. Each line also carries a newline and, once cut, a three byte ellipsis.
    let overhead: usize = prefixes.iter().map(|prefix| prefix.len() + 4).sum();
    let share = (budget.saturating_sub(overhead) / prefixes.len()).max(MIN_LINE_CHARACTERS);
    let mut shortened = render(Some(share));
    if shortened.len() > budget {
        // Too many lines for even a short one each: keep the start in full detail and say that
        // the rest was cut, rather than exceed the argument limit.
        let cut = shortened
            .char_indices()
            .take_while(|(index, _)| *index < budget)
            .last()
            .map_or(0, |(index, _)| index);
        let line_end = shortened[..cut].rfind('\n').map_or(0, |end| end + 1);
        shortened.truncate(line_end);
        shortened.push_str("[the rest of the transcript did not fit]\n");
    }
    shortened
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

    // A budget of its own the size of the project list's, so the two together stay well under the
    // argument limit.
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
        let line = format!("- {}: {}\n", project.project_id, whereabouts.join("; "));
        if work_lines.len() + line.len() > PROJECTS_BUDGET_BYTES {
            break;
        }
        work_lines.push_str(&line);
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    segments: Vec<AnswerSegment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnswerSegment {
    start_seconds: f64,
    end_seconds: f64,
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

/// A meeting in words alone's answer: its segments as runs of its lines.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinesAnswer {
    segments: Vec<ConversationAnswerSegment>,
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
    /// A meeting in words alone names its lines where a conversation names its messages.
    #[serde(alias = "firstLine")]
    first_message: i64,
    #[serde(alias = "lastLine")]
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

/// The runs among `candidates`, of a sequence of `count` messages or lines numbered from 1, that
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

/// The segments in a meeting in words alone's answer that hold up: runs of its lines, kept as a
/// conversation's runs of messages are ([`parse_conversation_answer`]), attributed only to
/// projects in `offered`. Each has no times, since the meeting has no clock.
///
/// # Errors
/// [`AnswerError`] when the answer holds no JSON object, is not the asked-for shape, or leaves no
/// segment standing.
pub fn parse_lines_answer(
    answer: &str,
    transcript: &[TranscriptLine],
    offered: &[ProjectChoice],
) -> Result<Vec<Segment>, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let parsed: LinesAnswer =
        serde_json::from_str(json).map_err(|error| AnswerError::Shape(error.to_string()))?;
    let runs = kept_runs(parsed.segments, transcript.len(), offered, |first| {
        format!("From line {}", first + 1)
    });
    if runs.is_empty() {
        return Err(AnswerError::Empty);
    }
    Ok(runs
        .into_iter()
        .map(|segment| Segment {
            start_ms: 0,
            end_ms: 0,
            title: segment.title,
            summary: segment.summary,
            projects: segment.projects,
            lines: Some(LineRun {
                first: segment.first,
                last: segment.last,
            }),
        })
        .collect())
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

/// The segments in a meeting's answer that hold up: inside the meeting, in order, apart,
/// long enough, and attributed only to projects in `offered`.
///
/// # Errors
/// [`AnswerError`] when the answer holds no JSON object, is not the asked-for shape, or
/// leaves no segment standing.
pub fn parse_answer(
    answer: &str,
    duration_ms: i64,
    offered: &[ProjectChoice],
) -> Result<Vec<Segment>, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let parsed: Answer =
        serde_json::from_str(json).map_err(|error| AnswerError::Shape(error.to_string()))?;

    let mut segments: Vec<Segment> = Vec::new();
    let mut previous_end = 0;
    for candidate in parsed.segments {
        if segments.len() == MAX_SEGMENTS {
            break;
        }
        let (Some(start_ms), Some(end_ms)) = (
            seconds_to_milliseconds(candidate.start_seconds),
            seconds_to_milliseconds(candidate.end_seconds),
        ) else {
            continue;
        };
        // A segment may start where the last one ended, never before it.
        let start_ms = start_ms.max(previous_end);
        let end_ms = if duration_ms > 0 {
            end_ms.min(duration_ms)
        } else {
            end_ms
        };
        if end_ms - start_ms < MIN_SEGMENT_MS {
            continue;
        }
        segments.push(Segment {
            start_ms,
            end_ms,
            title: title_or(&candidate.title, start_ms),
            summary: one_line(&candidate.summary, SUMMARY_CHARACTERS),
            projects: kept_projects(candidate.projects, offered),
            lines: None,
        });
        previous_end = end_ms;
    }

    if segments.is_empty() {
        return Err(AnswerError::Empty);
    }
    Ok(segments)
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

/// `answered` as a title, or where the segment starts when the answer gave none.
fn title_or(answered: &str, start_ms: i64) -> String {
    let title = one_line(answered, TITLE_CHARACTERS);
    if title.is_empty() {
        return format!("From {}", clock(start_ms));
    }
    title
}

/// Where a segment's clip is cut, from `start_ms` to `end_ms` of the recording: at the start of
/// the word Whisper heard nearest the segment's start and the end of the word nearest its end,
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

/// The transcript lines that fall inside `segment`, as its excerpt: `[m:ss] Speaker: text` lines,
/// without the time for a meeting in words alone.
pub fn excerpt(transcript: &[TranscriptLine], segment: &Segment) -> String {
    let mut excerpt = String::new();
    for (index, line) in transcript.iter().enumerate() {
        if !segment.holds(index, line) {
            continue;
        }
        let speaker = if line.speaker.is_empty() {
            "Someone"
        } else {
            line.speaker.as_str()
        };
        let _ = if segment.lines.is_some() {
            writeln!(excerpt, "{speaker}: {}", line.text)
        } else {
            writeln!(
                excerpt,
                "[{}] {speaker}: {}",
                clock(line.start_ms),
                line.text
            )
        };
    }
    excerpt
}

/// The speakers heard in `segment`, in the order they first spoke.
pub fn speakers(transcript: &[TranscriptLine], segment: &Segment) -> Vec<String> {
    let mut speakers: Vec<String> = Vec::new();
    for (index, line) in transcript.iter().enumerate() {
        let inside = segment.holds(index, line);
        if inside && !line.speaker.is_empty() && !speakers.contains(&line.speaker) {
            speakers.push(line.speaker.clone());
        }
    }
    speakers
}

/// The first `{` to the last `}`: models sometimes wrap JSON in a sentence or a code fence.
fn json_object(answer: &str) -> Option<&str> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    (end > start).then(|| &answer[start..=end])
}

fn seconds_to_milliseconds(seconds: f64) -> Option<i64> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a meeting's length in milliseconds is far inside i64"
    )]
    Some((seconds * 1000.0).round() as i64)
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

    #[test]
    fn the_prompt_names_the_meeting_its_projects_and_its_transcript() {
        let launch = project("Launch site");
        let prompt = prompt(
            MeetingContext {
                title: "Weekly sync",
                duration_ms: 3_725_000,
                participants: &["Ada".to_owned(), "Grace".to_owned()],
            },
            &[
                line(0, "Ada", "Hi."),
                line(65, "Grace", "Launch is Friday."),
            ],
            std::slice::from_ref(&launch),
        );

        assert!(prompt.contains("Meeting: Weekly sync"));
        assert!(prompt.contains("Length: 1:02:05 (3725 seconds)"));
        assert!(prompt.contains("Participants: Ada, Grace"));
        assert!(prompt.contains(&format!(
            "- {}: Launch site (Launch site description)",
            launch.id
        )));
        assert!(prompt.contains("[0:00] Ada: Hi."));
        assert!(prompt.contains("[1:05] Grace: Launch is Friday."));
    }

    #[test]
    fn a_speakers_consecutive_lines_merge_into_one() {
        let text = transcript_text(
            &[
                line(0, "Ada", "First."),
                line(3, "Ada", "Second."),
                line(6, "Grace", "Third."),
            ],
            TRANSCRIPT_BUDGET_BYTES,
        );
        assert_eq!(text, "[0:00] Ada: First. Second.\n[0:06] Grace: Third.\n");
    }

    #[test]
    fn a_transcript_over_budget_is_shortened_to_fit() {
        let transcript: Vec<TranscriptLine> = (0..400)
            .zip(["Ada", "Grace"].iter().cycle())
            .map(|(index, speaker)| line(index * 10, speaker, &"word ".repeat(200)))
            .collect();
        let budget = 50_000;
        let text = transcript_text(&transcript, budget);

        assert!(text.len() <= budget + 100, "{} bytes", text.len());
        // Every turn is still there, only shorter.
        assert_eq!(text.lines().count(), 400);
    }

    #[test]
    fn a_transcript_too_long_for_short_lines_says_where_it_was_cut() {
        let transcript: Vec<TranscriptLine> = (0..5_000)
            .zip(["Ada", "Grace"].iter().cycle())
            .map(|(index, speaker)| line(index, speaker, &"word ".repeat(30)))
            .collect();
        let text = transcript_text(&transcript, 20_000);

        assert!(text.len() <= 20_100);
        assert!(text.ends_with("[the rest of the transcript did not fit]\n"));
        assert!(text.starts_with("[0:00] Ada:"));
    }

    #[test]
    fn an_answer_keeps_only_what_holds_up() {
        let launch = project("Launch");
        let answer = format!(
            "Here you go:\n```json\n{{\"segments\": [\
              {{\"startSeconds\": 0, \"endSeconds\": 120, \"title\": \"  Intros \", \"summary\": \"Hello.\", \
                \"projects\": [{{\"projectId\": \"{launch}\", \"confidence\": 1.4}}, \
                               {{\"projectId\": \"{stranger}\", \"confidence\": 0.9}}, \
                               {{\"projectId\": \"{launch}\", \"confidence\": 0.5}}]}}, \
              {{\"startSeconds\": 100, \"endSeconds\": 400, \"title\": \"Launch\", \"summary\": \"Friday.\", \
                \"projects\": [{{\"projectId\": \"{launch}\", \"confidence\": 0.1}}]}}, \
              {{\"startSeconds\": 400, \"endSeconds\": 405, \"title\": \"Aside\", \"summary\": \"\"}}, \
              {{\"startSeconds\": 405, \"endSeconds\": 9000, \"title\": \"\", \"summary\": \"Wrap up.\"}}\
            ]}}\n```",
            launch = launch.id,
            stranger = Uuid::now_v7(),
        );
        let segments =
            parse_answer(&answer, 600_000, std::slice::from_ref(&launch)).expect("segments");

        assert_eq!(segments.len(), 3);
        // Clamped to 1, the stranger and the repeat dropped.
        assert_eq!(
            segments[0].projects,
            vec![SegmentProject {
                project_id: launch.id,
                confidence: 1.0
            }]
        );
        assert_eq!(segments[0].title, "Intros");
        // Moved to start where the first ended; its guess was too unsure to keep.
        assert_eq!(
            (segments[1].start_ms, segments[1].end_ms),
            (120_000, 400_000)
        );
        assert!(segments[1].projects.is_empty());
        // The five second aside is dropped, and the last segment ends with the meeting.
        assert_eq!(
            (segments[2].start_ms, segments[2].end_ms),
            (405_000, 600_000)
        );
        assert_eq!(segments[2].title, "From 6:45");
    }

    #[test]
    fn an_answer_without_json_or_segments_is_an_error() {
        assert_eq!(
            parse_answer("No idea.", 60_000, &[]),
            Err(AnswerError::NoJson)
        );
        assert!(matches!(
            parse_answer("{\"chapters\": []}", 60_000, &[]),
            Err(AnswerError::Shape(_))
        ));
        assert_eq!(
            parse_answer("{\"segments\": []}", 60_000, &[]),
            Err(AnswerError::Empty)
        );
    }

    #[test]
    fn an_excerpt_holds_the_lines_inside_its_segment() {
        let transcript = [
            line(0, "Ada", "Hi."),
            line(30, "Grace", "Launch."),
            line(61, "Ada", "Budget."),
        ];
        let segment = Segment {
            start_ms: 20_000,
            end_ms: 61_000,
            title: "Launch".to_owned(),
            summary: String::new(),
            projects: Vec::new(),
            lines: None,
        };
        assert_eq!(excerpt(&transcript, &segment), "[0:30] Grace: Launch.\n");
        assert_eq!(speakers(&transcript, &segment), vec!["Grace".to_owned()]);
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
    fn a_meeting_in_words_alone_is_asked_and_answered_by_its_lines() {
        let launch = project("Launch");
        let transcript = [
            line(0, "Ada", "Hello everyone."),
            line(0, "Ada", "Launch is Friday."),
            line(0, "", "Agreed."),
            line(0, "Grace", "How was the weekend?"),
        ];
        let prompt = lines_prompt(
            MeetingContext {
                title: "Weekly sync",
                duration_ms: 0,
                participants: &["Ada".to_owned()],
            },
            &transcript,
            std::slice::from_ref(&launch),
        );
        assert!(prompt.contains("Lines: 4\n"), "{prompt}");
        assert!(
            prompt.contains("[2] Ada: Launch is Friday.\n[3] Someone: Agreed.\n"),
            "lines are numbered, never merged, and carry no times: {prompt}"
        );
        assert!(!prompt.contains("0:00"), "{prompt}");

        let answer = format!(
            "{{\"segments\": [\
              {{\"firstLine\": 1, \"lastLine\": 3, \"title\": \"Launch\", \
                \"summary\": \"Ships Friday.\", \
                \"projects\": [{{\"projectId\": \"{launch}\", \"confidence\": 0.9}}]}}, \
              {{\"firstLine\": 3, \"lastLine\": 9, \"title\": \"\"}}\
            ]}}",
            launch = launch.id,
        );
        let segments = parse_lines_answer(&answer, &transcript, std::slice::from_ref(&launch))
            .expect("segments");

        assert_eq!(
            segments,
            vec![
                Segment {
                    start_ms: 0,
                    end_ms: 0,
                    title: "Launch".to_owned(),
                    summary: "Ships Friday.".to_owned(),
                    projects: vec![SegmentProject {
                        project_id: launch.id,
                        confidence: 0.9,
                    }],
                    lines: Some(LineRun { first: 0, last: 2 }),
                },
                // It overlapped the first, so it starts after it and ends at the last line.
                Segment {
                    start_ms: 0,
                    end_ms: 0,
                    title: "From line 4".to_owned(),
                    summary: String::new(),
                    projects: Vec::new(),
                    lines: Some(LineRun { first: 3, last: 3 }),
                },
            ]
        );
        assert_eq!(
            excerpt(&transcript, &segments[0]),
            "Ada: Hello everyone.\nAda: Launch is Friday.\nSomeone: Agreed.\n"
        );
        assert_eq!(speakers(&transcript, &segments[1]), ["Grace"]);
        assert_eq!(
            parse_lines_answer("{\"segments\": []}", &transcript, &[]),
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
