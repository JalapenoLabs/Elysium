// Copyright © 2026 Jalapeno Labs

//! What Elysia is told at the start of a turn, and how her final text is read back.
//!
//! Her standing instructions live in `instructions.md` beside this file, versioned with the
//! code, and are appended to Claude Code's system prompt on every turn. The prompt itself
//! names why the turn started and carries what she needs for it. Both are plain functions of
//! their inputs, so they are tested without a runner.

use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

/// Her standing instructions.
pub const INSTRUCTIONS: &str = include_str!("instructions.md");

/// The marker her final line starts with; see `instructions.md`, "Ending a turn".
const SUMMARY_MARKER: &str = "Summary:";

/// The longest summary stored, matching `elysia_turns.summary`'s limit.
const MAX_SUMMARY_CHARACTERS: usize = 1000;

/// A person's message, as the prompt quotes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMessage {
    pub author: String,
    pub body: String,
    pub sent_at: DateTime<Utc>,
}

/// Work she is coming back to, as the prompt lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleWork {
    pub title: String,
    /// `gave-up`, `blocked`, or `working` past its estimate.
    pub status: String,
    pub detail: String,
    pub ready_at: Option<DateTime<Utc>>,
}

/// A comment thread on a meeting that mentioned her, as the prompt quotes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingThread {
    pub meeting_title: String,
    /// Where its range starts and ends on the meeting's clock, in milliseconds.
    pub start_ms: i64,
    pub end_ms: i64,
    /// What was said just before the range, and just after it.
    pub before: String,
    pub after: String,
    /// The range's words in order, each with the id her tools name it by.
    pub words: Vec<ThreadWord>,
    /// How many of the range's words were left out past the ones quoted.
    pub words_left_out: usize,
    /// The meeting's topics the range lies in, each with the projects it is on.
    pub topics: Vec<ThreadTopic>,
    /// Its comments, oldest first.
    pub comments: Vec<PromptMessage>,
}

/// One word of a thread's range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadWord {
    pub id: Uuid,
    pub speaker: String,
    pub text: String,
}

/// One topic of a meeting a thread's range lies in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadTopic {
    pub title: String,
    /// Each project it is on, as `name (id)`.
    pub projects: Vec<String>,
}

/// Why a turn started, with what it carries.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnContext {
    /// People wrote to her; every message since her last reply, oldest first.
    Messages(Vec<PromptMessage>),
    /// A person answered her question.
    Answer {
        question: String,
        answer: String,
        answered_by: String,
    },
    /// Changes in the workspace or a provider, each as the event stream announced it.
    Events(Vec<Value>),
    /// Nothing was queued; this is her work worth revisiting.
    Idle(Vec<IdleWork>),
    /// A person mentioned her in a comment thread on a meeting.
    MeetingComment(MeetingThread),
}

/// The prompt for one turn.
pub fn render(context: &TurnContext, now: DateTime<Utc>) -> String {
    let mut prompt = String::new();
    let _ = writeln!(prompt, "It is {} (UTC).", now.to_rfc3339());

    match context {
        TurnContext::Messages(messages) => {
            prompt.push_str("Trigger: message. Reply to them.\n");
            for message in messages {
                let _ = write!(
                    prompt,
                    "\n{} wrote at {}:\n{}\n",
                    message.author,
                    message.sent_at.to_rfc3339(),
                    message.body
                );
            }
        }
        TurnContext::Answer {
            question,
            answer,
            answered_by,
        } => {
            let _ = write!(
                prompt,
                "Trigger: answer. You asked:\n{question}\n\n{answered_by} answered:\n{answer}\n\n\
                 Continue the work it was about.\n"
            );
        }
        TurnContext::Events(events) => {
            let _ = writeln!(
                prompt,
                "Trigger: event. {} change(s), one JSON object per line, as Elysium's event stream \
                 announced them. Decide what each means for you.\n",
                events.len()
            );
            for change in events {
                let _ = writeln!(prompt, "{change}");
            }
        }
        TurnContext::Idle(work) => {
            prompt.push_str(
                "Trigger: idle. Nothing is queued. Revisit this work: try again what you gave up \
                 on, check whether what blocked you has changed, and update what is past its \
                 estimate.\n",
            );
            for item in work {
                let ready = item
                    .ready_at
                    .map(|ready_at| format!(", expected ready {}", ready_at.to_rfc3339()))
                    .unwrap_or_default();
                let _ = write!(
                    prompt,
                    "\n- {} ({}{ready})\n  {}\n",
                    item.title, item.status, item.detail
                );
            }
        }
        TurnContext::MeetingComment(thread) => render_thread(&mut prompt, thread),
    }
    prompt
}

/// A thread's prompt: the meeting, the range with each word's id, the topics it lies in, and the
/// thread itself, its newest comment last.
fn render_thread(prompt: &mut String, thread: &MeetingThread) {
    let _ = write!(
        prompt,
        "Trigger: meeting comment. A person mentioned you in a comment thread on the meeting \
         \"{}\". Your reply is posted in the thread.\n\n\
         The thread is on the words from {} to {} of the meeting. Each word below is its id, \
         then what it says; your meeting tools name words by these ids.\n",
        thread.meeting_title,
        clock(thread.start_ms),
        clock(thread.end_ms),
    );
    if !thread.before.is_empty() {
        let _ = writeln!(prompt, "\nJust before: …{}", thread.before);
    }
    let mut speaker: Option<&str> = None;
    for word in &thread.words {
        if speaker != Some(word.speaker.as_str()) {
            speaker = Some(word.speaker.as_str());
            let name = if word.speaker.is_empty() {
                "(no speaker named)"
            } else {
                word.speaker.as_str()
            };
            let _ = writeln!(prompt, "\n{name}:");
        }
        let _ = writeln!(prompt, "{} {}", word.id, word.text);
    }
    if thread.words_left_out > 0 {
        let _ = writeln!(
            prompt,
            "(and {} more words of the range)",
            thread.words_left_out
        );
    }
    if !thread.after.is_empty() {
        let _ = writeln!(prompt, "\nJust after: {}…", thread.after);
    }

    if thread.topics.is_empty() {
        prompt.push_str("\nThe range lies in none of the meeting's topics yet.\n");
    } else {
        prompt.push_str("\nThe range lies in these topics of the meeting:\n");
        for topic in &thread.topics {
            let projects = if topic.projects.is_empty() {
                "on no project".to_owned()
            } else {
                format!("on {}", topic.projects.join(", "))
            };
            let _ = writeln!(prompt, "- {}, {projects}", topic.title);
        }
    }

    prompt.push_str("\nThe thread, oldest first:\n");
    for comment in &thread.comments {
        let _ = write!(
            prompt,
            "\n{} wrote at {}:\n{}\n",
            comment.author,
            comment.sent_at.to_rfc3339(),
            comment.body
        );
    }
}

/// `ms` on a meeting's clock as `m:ss`, or `h:mm:ss` past the first hour.
fn clock(ms: i64) -> String {
    let seconds = ms.max(0) / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        return format!("{hours}:{minutes:02}:{seconds:02}");
    }
    format!("{minutes}:{seconds:02}")
}

/// Her final text, split into the reply a person reads and the summary her log shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadBack {
    /// Everything before the summary line, trimmed; `None` when nothing is left.
    pub reply: Option<String>,
    pub summary: Option<String>,
}

/// Splits her final text at its last `Summary:` line.
pub fn read_back(result: &str) -> ReadBack {
    let Some((before, after)) = result.rsplit_once(SUMMARY_MARKER) else {
        return ReadBack {
            reply: non_empty(result),
            summary: None,
        };
    };
    // Only a marker at the start of a line counts, not one quoted mid-sentence.
    if !before.is_empty() && !before.ends_with('\n') {
        return ReadBack {
            reply: non_empty(result),
            summary: None,
        };
    }
    let summary =
        non_empty(after).map(|summary| summary.chars().take(MAX_SUMMARY_CHARACTERS).collect());
    ReadBack {
        reply: non_empty(before),
        summary,
    }
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 15, 0, 0)
            .single()
            .expect("a valid instant")
    }

    #[test]
    fn a_message_prompt_quotes_each_message_with_its_author() {
        let prompt = render(
            &TurnContext::Messages(vec![PromptMessage {
                author: "Alex".to_owned(),
                body: "Draft the quarterly deck.".to_owned(),
                sent_at: now(),
            }]),
            now(),
        );
        assert!(prompt.contains("Trigger: message"));
        assert!(
            prompt.contains("Alex wrote at 2026-10-20T15:00:00+00:00:\nDraft the quarterly deck.")
        );
    }

    #[test]
    fn an_event_prompt_lists_one_change_per_line() {
        let prompt = render(
            &TurnContext::Events(vec![
                json!({ "type": "history.appended", "data": { "kind": "created" } }),
                json!({ "type": "changeset.upserted", "data": { "state": "applied" } }),
            ]),
            now(),
        );
        assert!(prompt.contains("2 change(s)"));
        assert_eq!(
            prompt.lines().filter(|line| line.starts_with('{')).count(),
            2
        );
    }

    #[test]
    fn a_meeting_comment_prompt_names_each_word_by_its_id_and_quotes_the_thread() {
        let sheriff = Uuid::now_v7();
        let ships = Uuid::now_v7();
        let prompt = render(
            &TurnContext::MeetingComment(MeetingThread {
                meeting_title: "Weekly sync".to_owned(),
                start_ms: 65_000,
                end_ms: 3_725_000,
                before: "so the plan is".to_owned(),
                after: "on Friday".to_owned(),
                words: vec![
                    ThreadWord {
                        id: sheriff,
                        speaker: "Ada".to_owned(),
                        text: "Sheriff".to_owned(),
                    },
                    ThreadWord {
                        id: ships,
                        speaker: "Ada".to_owned(),
                        text: "ships".to_owned(),
                    },
                ],
                words_left_out: 3,
                topics: vec![ThreadTopic {
                    title: "Launch date".to_owned(),
                    projects: vec!["Launch (0190)".to_owned()],
                }],
                comments: vec![PromptMessage {
                    author: "Alex".to_owned(),
                    body: "@Elysia this was Sharath".to_owned(),
                    sent_at: now(),
                }],
            }),
            now(),
        );
        assert!(prompt.contains("Trigger: meeting comment"));
        assert!(prompt.contains("\"Weekly sync\""));
        assert!(prompt.contains("from 1:05 to 1:02:05"));
        assert!(prompt.contains(&format!("\nAda:\n{sheriff} Sheriff\n{ships} ships\n")));
        assert!(prompt.contains("(and 3 more words of the range)"));
        assert!(prompt.contains("- Launch date, on Launch (0190)"));
        assert!(
            prompt.contains("Alex wrote at 2026-10-20T15:00:00+00:00:\n@Elysia this was Sharath")
        );
    }

    #[test]
    fn the_summary_line_is_split_from_the_reply() {
        assert_eq!(
            read_back("The deck is drafted.\n\nSummary: Drafted the quarterly deck."),
            ReadBack {
                reply: Some("The deck is drafted.".to_owned()),
                summary: Some("Drafted the quarterly deck.".to_owned()),
            }
        );
    }

    #[test]
    fn a_turn_with_only_a_summary_has_no_reply() {
        assert_eq!(
            read_back("Summary: Filed two newsletters."),
            ReadBack {
                reply: None,
                summary: Some("Filed two newsletters.".to_owned()),
            }
        );
    }

    #[test]
    fn a_marker_mid_sentence_is_part_of_the_reply() {
        let text = "I wrote the Summary: section of the doc.";
        assert_eq!(
            read_back(text),
            ReadBack {
                reply: Some(text.to_owned()),
                summary: None,
            }
        );
    }

    #[test]
    fn the_instructions_ask_for_the_summary_line_this_module_reads() {
        assert!(INSTRUCTIONS.contains(SUMMARY_MARKER));
    }
}
