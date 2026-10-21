// Copyright © 2026 Jalapeno Labs

//! Cutting a meeting's transcript into topic segments, each attributed to the projects it is
//! about. Pure: the prompt Elysia reads, and the checks her answer has to pass.
//!
//! Segmentation is one text turn on the runner (`crate::runner::TurnMode::Text`): no tools,
//! no settings, only the transcript and the workspace's projects in the prompt and a JSON
//! answer back. Nothing the answer says is taken on trust: [`parse_answer`] keeps only
//! segments that lie inside the meeting, in order and without overlapping, and only
//! attributions to projects that were offered, so a model's mistake can drop a segment but
//! never invent a project or a time.
//!
//! # Size
//!
//! The runner passes the prompt as one argument to `claude`, and Linux allows one argument
//! 128 KiB. [`transcript_text`] therefore merges each speaker's consecutive lines and, past
//! [`TRANSCRIPT_BUDGET_BYTES`], shortens every line evenly, so a long meeting still reaches
//! the model whole, only in less detail.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::source::TranscriptLine;

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

/// The least confidence an attribution needs to be kept. Below it the model is guessing.
const MIN_CONFIDENCE: f32 = 0.3;

/// The model that segments meetings: reading an hour of conversation for its topics takes
/// more than the small model that writes turn summaries.
pub const SEGMENTATION_MODEL: &str = "claude-sonnet-5-5";

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
    pub start_ms: i64,
    pub end_ms: i64,
    pub title: String,
    pub summary: String,
    pub projects: Vec<SegmentProject>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentProject {
    pub project_id: Uuid,
    pub confidence: f32,
}

/// The meeting as the prompt describes it.
#[derive(Debug, Clone, Copy)]
pub struct MeetingContext<'meeting> {
    pub title: &'meeting str,
    pub duration_ms: i64,
    pub participants: &'meeting [String],
}

/// The segmentation turn's prompt.
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

    prompt.push_str("\nTranscript, each line starting at [minutes:seconds]:\n");
    prompt.push_str(&transcript_text(transcript, TRANSCRIPT_BUDGET_BYTES));
    prompt
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

    let whole = render(None);
    if whole.len() <= budget || turns.is_empty() {
        return whole;
    }
    // Every turn's text gets the same share of what the prefixes leave, never less than a
    // short line. Each line also carries a newline and, once cut, a three byte ellipsis.
    let overhead: usize = prefixes.iter().map(|prefix| prefix.len() + 4).sum();
    let share = (budget.saturating_sub(overhead) / turns.len()).max(MIN_LINE_CHARACTERS);
    let mut shortened = render(Some(share));
    if shortened.len() > budget {
        // Too many turns for even a short line each: keep the meeting's start in full detail
        // and say that the rest was cut, rather than exceed the argument limit.
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
struct AnswerProject {
    project_id: String,
    confidence: f32,
}

/// The segments in the model's answer that hold up: inside the meeting, in order, apart,
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

        let mut projects: Vec<SegmentProject> = Vec::new();
        for project in candidate.projects {
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

        let title = one_line(&candidate.title, TITLE_CHARACTERS);
        segments.push(Segment {
            start_ms,
            end_ms,
            title: if title.is_empty() {
                format!("From {}", clock(start_ms))
            } else {
                title
            },
            summary: one_line(&candidate.summary, SUMMARY_CHARACTERS),
            projects,
        });
        previous_end = end_ms;
    }

    if segments.is_empty() {
        return Err(AnswerError::Empty);
    }
    Ok(segments)
}

/// The transcript lines that fall inside `segment`, as its excerpt.
pub fn excerpt(transcript: &[TranscriptLine], segment: &Segment) -> String {
    let mut excerpt = String::new();
    for line in transcript {
        if line.start_ms < segment.start_ms || line.start_ms >= segment.end_ms {
            continue;
        }
        let speaker = if line.speaker.is_empty() {
            "Someone"
        } else {
            line.speaker.as_str()
        };
        let _ = writeln!(
            excerpt,
            "[{}] {speaker}: {}",
            clock(line.start_ms),
            line.text
        );
    }
    excerpt
}

/// The speakers heard in `segment`, in the order they first spoke.
pub fn speakers(transcript: &[TranscriptLine], segment: &Segment) -> Vec<String> {
    let mut speakers: Vec<String> = Vec::new();
    for line in transcript {
        let inside = line.start_ms >= segment.start_ms && line.start_ms < segment.end_ms;
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
        };
        assert_eq!(excerpt(&transcript, &segment), "[0:30] Grace: Launch.\n");
        assert_eq!(speakers(&transcript, &segment), vec!["Grace".to_owned()]);
    }

    #[test]
    fn clocks_read_like_a_player() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(65_000), "1:05");
        assert_eq!(clock(3_725_000), "1:02:05");
    }
}
