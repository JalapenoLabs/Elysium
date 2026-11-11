// Copyright © 2026 Jalapeno Labs

//! What the agent handling a conversation reads and answers. Pure: the prompts, and the checks
//! its answers have to pass. See `super::handle`.
//!
//! The agent is one Claude session per conversation, resumed for every turn as text in, text
//! out (`crate::runner::TurnMode::ResumableText`): no tools, no settings, so it can neither send
//! nor change anything itself. Each turn hands it the messages it has not seen, and it answers in
//! JSON, either to wait for more or with what the conversation asks of the person:
//!
//! ```json
//! { "wait": true }
//! { "items": [{ "title": "Send Ada the Q3 deck", "notes": "...", "messages": [2, 3] }] }
//! ```
//!
//! Nothing the answer says is taken on trust: an item is kept only with a title, and names only
//! messages its turn was given, so a mistake can drop an item but never point one at a message
//! the agent never read.

use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::meetings::segment::SegmentedTopic;

/// Instructions the agent's every turn runs under, in place of Claude Code's own.
pub const SYSTEM_PROMPT: &str = "\
You read a person's conversations for them as messages arrive (chat and mail), and decide what \
each asks of them. You never write to anyone: nothing you say is sent, and the person answers \
everything themselves. Each turn gives you the messages you have not seen yet, in order, and you \
answer with one JSON object and nothing else.\n\
\n\
When the newest message reads as unfinished, so that more is clearly coming (\"one sec\", \
\"also...\", a sentence cut off, a list being typed one line at a time), and you may wait, \
answer {\"wait\": true}: you are told what arrived in the next 30 seconds, and answer again. \
Otherwise answer {\"items\": [{\"title\": string, \"notes\": string, \"messages\": [number]}]}: \
one item for each question someone asked the person, and each thing someone asked them to do, \
that the person has not already answered or done in the conversation. The title says what the \
person owes, as a short imperative or question (\"Send Ada the Q3 deck\", \"Can the launch move \
to Friday?\"), at most 200 characters. The notes say in one to three plain sentences who asked, \
what exactly, and any deadline. `messages` are the numbers of the messages it comes from. A \
question you raised before and that is still open is not raised again. Greetings, thanks, \
acknowledgements, and information that asks nothing of the person raise nothing: answer \
{\"items\": []}.";

/// How long the agent's one wait lasts.
pub const WAIT: chrono::TimeDelta = chrono::TimeDelta::seconds(30);

/// The most items one answer raises: a conversation is a few requests at most, and more is a
/// misreading the person should not have to clear.
pub const MAX_ITEMS: usize = 5;

/// The longest title and notes kept, inside `action_items`' checks.
pub const TITLE_CHARACTERS: usize = 200;
pub const NOTES_CHARACTERS: usize = 2_000;

/// The most bytes of messages one prompt carries. The conversation's agent is one session
/// resumed every turn, keeping all it was told, so one burst of long messages (a pasted log, a
/// forwarded thread) is shortened rather than left to crowd every turn after it.
const MESSAGES_BUDGET_BYTES: usize = 90_000;

/// The shortest a message is cut to when a turn's messages are over budget.
const MIN_MESSAGE_CHARACTERS: usize = 200;

/// One message as the agent is given it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMessage<'text> {
    pub occurred_at: DateTime<Utc>,
    pub actor: &'text str,
    pub text: &'text str,
}

/// What one turn's prompt is built from.
#[derive(Debug, Clone)]
pub struct Prompt<'text> {
    /// What the conversation is called where it is had.
    pub title: &'text str,
    /// Whose conversation it is, by name.
    pub person: &'text str,
    /// Whether this starts the agent's session, which then needs to be told where it is.
    pub new_session: bool,
    /// What segmenting found the conversation to be about, on a run's first turn; empty after.
    pub topics: &'text [SegmentedTopic],
    /// The projects segmenting named, by name, beside each topic.
    pub project_names: &'text [(uuid::Uuid, String)],
    /// The messages it has not seen, oldest first, numbered from 1.
    pub messages: &'text [PromptMessage<'text>],
    /// Whether the wait it asked for just ended.
    pub after_wait: bool,
    /// Whether it may still ask to wait.
    pub may_wait: bool,
}

/// One turn's prompt.
pub fn prompt(context: &Prompt<'_>) -> String {
    let mut prompt = String::new();
    if context.new_session {
        let _ = writeln!(
            prompt,
            "This is {}'s conversation \"{}\". You will hear more of it over later turns.",
            context.person, context.title
        );
        prompt.push('\n');
    }
    if !context.topics.is_empty() {
        prompt.push_str("What the conversation is about so far:\n");
        for topic in context.topics {
            let projects: Vec<&str> = topic
                .projects
                .iter()
                .filter_map(|attributed| {
                    context
                        .project_names
                        .iter()
                        .find(|(id, _name)| *id == attributed.project_id)
                        .map(|(_id, name)| name.as_str())
                })
                .collect();
            let _ = write!(prompt, "- {}: {}", topic.title, topic.summary);
            if !projects.is_empty() {
                let _ = write!(prompt, " (projects: {})", projects.join(", "));
            }
            prompt.push('\n');
        }
        prompt.push('\n');
    }
    if context.after_wait {
        prompt.push_str("The 30 seconds you waited are over. ");
    }
    if context.messages.is_empty() {
        prompt.push_str("No new message arrived.\n");
    } else {
        let _ = writeln!(
            prompt,
            "New messages in \"{}\", numbered for this turn:",
            context.title
        );
        prompt.push_str(&messages_text(context.messages));
    }
    prompt.push('\n');
    if context.may_wait {
        prompt.push_str("You may answer {\"wait\": true} once if more is clearly coming.");
    } else {
        prompt.push_str("Decide now: there is no more waiting.");
    }
    prompt
}

/// `messages` as `[n] <time> Sender: text` lines, each shortened evenly when they are over
/// budget together, so every message reaches the agent, only in less detail.
fn messages_text(messages: &[PromptMessage<'_>]) -> String {
    let total: usize = messages.iter().map(|message| message.text.len()).sum();
    let per_message = if total > MESSAGES_BUDGET_BYTES {
        (MESSAGES_BUDGET_BYTES / messages.len().max(1)).max(MIN_MESSAGE_CHARACTERS)
    } else {
        usize::MAX
    };
    let mut text = String::new();
    for (index, message) in messages.iter().enumerate() {
        let body: String = message
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(per_message)
            .collect();
        let _ = writeln!(
            text,
            "[{}] {} {}: {body}",
            index + 1,
            message.occurred_at.format("%Y-%m-%d %H:%M UTC"),
            message.actor
        );
    }
    text
}

/// What the agent answered, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// It asked to wait for more.
    Wait,
    /// What the conversation asks of the person, possibly nothing.
    Items(Vec<Item>),
}

/// One thing the person owes, as the agent named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub notes: String,
    /// The messages it comes from, as indices into the turn's messages, in order, each once.
    pub messages: Vec<usize>,
}

/// Why an answer could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerError {
    #[error("the answer holds no JSON object")]
    NoJson,
    #[error("the answer is not the expected shape: {0}")]
    Shape(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAnswer {
    #[serde(default)]
    wait: bool,
    #[serde(default)]
    items: Vec<RawItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    #[serde(default)]
    title: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    messages: Vec<u64>,
}

/// The agent's `answer` to a turn that gave it `message_count` messages.
///
/// # Errors
/// [`AnswerError`] for an answer with no JSON object, or one not shaped as asked.
pub fn parse_answer(answer: &str, message_count: usize) -> Result<Answer, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let raw: RawAnswer =
        serde_json::from_str(json).map_err(|error| AnswerError::Shape(error.to_string()))?;
    if raw.wait {
        return Ok(Answer::Wait);
    }
    let mut items = Vec::new();
    for candidate in raw.items {
        let title: String = candidate
            .title
            .trim()
            .chars()
            .take(TITLE_CHARACTERS)
            .collect();
        if title.is_empty() {
            continue;
        }
        let mut messages: Vec<usize> = candidate
            .messages
            .into_iter()
            .filter_map(|number| usize::try_from(number).ok())
            .filter(|number| (1..=message_count).contains(number))
            .map(|number| number - 1)
            .collect();
        messages.sort_unstable();
        messages.dedup();
        items.push(Item {
            title,
            notes: candidate
                .notes
                .trim()
                .chars()
                .take(NOTES_CHARACTERS)
                .collect(),
            messages,
        });
        if items.len() == MAX_ITEMS {
            break;
        }
    }
    Ok(Answer::Items(items))
}

fn json_object(answer: &str) -> Option<&str> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    (end > start).then(|| &answer[start..=end])
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::meetings::segmentation::SegmentProject;

    fn at(minute: u32) -> DateTime<Utc> {
        format!("2026-11-04T12:{minute:02}:00Z")
            .parse()
            .expect("a time")
    }

    #[test]
    fn a_wait_is_a_wait_whatever_else_it_says() {
        assert_eq!(parse_answer(r#"{"wait": true}"#, 2), Ok(Answer::Wait));
        assert_eq!(
            parse_answer(r#"Sure. {"wait": true, "items": [{"title": "x"}]}"#, 2),
            Ok(Answer::Wait)
        );
    }

    #[test]
    fn items_keep_only_messages_their_turn_offered() {
        let answer = r#"{"items": [
            {"title": "  Send Ada the Q3 deck ", "notes": "Ada asked twice.", "messages": [3, 1, 3, 9, 0]},
            {"title": "", "notes": "no title, dropped", "messages": [1]}
        ]}"#;
        assert_eq!(
            parse_answer(answer, 3),
            Ok(Answer::Items(vec![Item {
                title: "Send Ada the Q3 deck".to_owned(),
                notes: "Ada asked twice.".to_owned(),
                messages: vec![0, 2],
            }]))
        );
    }

    #[test]
    fn nothing_for_the_person_is_no_items_and_a_runaway_answer_is_capped() {
        assert_eq!(
            parse_answer(r#"{"items": []}"#, 1),
            Ok(Answer::Items(Vec::new()))
        );
        let many: Vec<String> = (0..12)
            .map(|index| format!(r#"{{"title": "Item {index}"}}"#))
            .collect();
        let answer = format!(r#"{{"items": [{}]}}"#, many.join(","));
        let Ok(Answer::Items(items)) = parse_answer(&answer, 1) else {
            panic!("items");
        };
        assert_eq!(items.len(), MAX_ITEMS);
    }

    #[test]
    fn an_answer_without_json_or_of_the_wrong_shape_is_refused() {
        assert_eq!(parse_answer("I'd wait.", 1), Err(AnswerError::NoJson));
        assert!(matches!(
            parse_answer(r#"{"items": "none"}"#, 1),
            Err(AnswerError::Shape(_))
        ));
    }

    #[test]
    fn a_new_sessions_first_turn_says_where_it_is_and_what_it_is_about() {
        let website = Uuid::now_v7();
        let topics = [SegmentedTopic {
            title: "Q3 deck".to_owned(),
            summary: "Ada needs the deck for Friday.".to_owned(),
            start_ms: 0,
            end_ms: 0,
            projects: vec![SegmentProject {
                project_id: website,
                confidence: 0.9,
            }],
            record_ids: Vec::new(),
        }];
        let names = [(website, "Website".to_owned())];
        let messages = [
            PromptMessage {
                occurred_at: at(1),
                actor: "Ada",
                text: "Can you send me\nthe Q3 deck?",
            },
            PromptMessage {
                occurred_at: at(2),
                actor: "Ada",
                text: "also",
            },
        ];
        let text = prompt(&Prompt {
            title: "Ada in a direct message in Mooreslab",
            person: "Alex",
            new_session: true,
            topics: &topics,
            project_names: &names,
            messages: &messages,
            after_wait: false,
            may_wait: true,
        });
        assert!(
            text.starts_with(
                "This is Alex's conversation \"Ada in a direct message in Mooreslab\"."
            )
        );
        assert!(text.contains("- Q3 deck: Ada needs the deck for Friday. (projects: Website)"));
        assert!(text.contains("[1] 2026-11-04 12:01 UTC Ada: Can you send me the Q3 deck?"));
        assert!(text.contains("[2] 2026-11-04 12:02 UTC Ada: also"));
        assert!(text.ends_with("once if more is clearly coming."));
    }

    #[test]
    fn a_turn_after_its_wait_says_so_and_offers_no_second() {
        let text = prompt(&Prompt {
            title: "#launch in Mooreslab",
            person: "Alex",
            new_session: false,
            topics: &[],
            project_names: &[],
            messages: &[],
            after_wait: true,
            may_wait: false,
        });
        assert_eq!(
            text,
            "The 30 seconds you waited are over. No new message arrived.\n\nDecide now: there is no \
             more waiting."
        );
    }

    #[test]
    fn messages_over_budget_are_each_shortened_and_all_kept() {
        let long = "word ".repeat(30_000);
        let messages: Vec<PromptMessage<'_>> = (0..3)
            .map(|_index| PromptMessage {
                occurred_at: at(0),
                actor: "Ada",
                text: &long,
            })
            .collect();
        let text = messages_text(&messages);
        assert!(text.len() <= MESSAGES_BUDGET_BYTES + 3 * 64);
        assert!(text.contains("[3] "));
    }
}
