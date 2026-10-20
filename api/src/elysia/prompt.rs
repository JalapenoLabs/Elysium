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
    }
    prompt
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
