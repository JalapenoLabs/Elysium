// Copyright © 2026 Jalapeno Labs

//! What a Project Manager is told at the start of a turn.
//!
//! Its standing instructions live in `instructions.md` beside this file, versioned with the code,
//! and are appended to Claude Code's system prompt on every turn. The prompt names its project and
//! carries everything that woke it: the messages, the records newly on its timeline, and the
//! batches that finished. It is a plain function of its inputs, so it is tested without a runner.
//!
//! The runner passes the prompt as one argument, and Linux allows one argument 128 KiB, so a
//! record is quoted in part ([`EXCERPT_MAX_CHARACTERS`]) and the whole prompt stays under
//! [`PROMPT_MAX_BYTES`]; the Project Manager reads any record in full with `manager_record`.

use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::work_batch::BatchState;

/// Its standing instructions.
pub const INSTRUCTIONS: &str = include_str!("instructions.md");

/// How much of a record's body the prompt quotes, in characters.
pub const EXCERPT_MAX_CHARACTERS: usize = 800;

/// How much of a project's description the prompt quotes, in characters.
const DESCRIPTION_MAX_CHARACTERS: usize = 1_000;

/// The longest prompt, in bytes: under the 128 KiB Linux allows one argument, with room for the
/// runner's own.
pub const PROMPT_MAX_BYTES: usize = 100_000;

/// What the end of a prompt cut to fit says.
const CUT_NOTE: &str = "\n\n(The rest did not fit in this prompt; read it with manager_timeline and manager_batches.)\n";

/// The project a turn works for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBrief {
    pub id: Uuid,
    pub name: String,
    pub description: String,
}

/// A message a person or Elysia sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMessage {
    /// Who wrote it, by name.
    pub author: String,
    pub from_elysia: bool,
    pub body: String,
    pub sent_at: DateTime<Utc>,
}

/// A record newly on its project's timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptRecord {
    pub id: Uuid,
    /// Its kind, such as `meeting` for a meeting's segment or `slack` for a message.
    pub source_kind: String,
    pub occurred_at: DateTime<Utc>,
    pub actor: String,
    pub title: String,
    /// A meeting segment's summary of what was said and decided.
    pub summary: Option<String>,
    pub body: String,
    /// How sure the attribution to the project is, 0 to 1.
    pub confidence: f32,
    /// Whether a person put it on the project, rather than Elysia's reading.
    pub attributed_by_person: bool,
}

/// A batch of its that finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBatch {
    pub id: Uuid,
    pub title: String,
    pub state: BatchState,
    pub result: Option<String>,
}

/// Everything one turn is told.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnContext {
    pub project: ProjectBrief,
    pub messages: Vec<PromptMessage>,
    pub records: Vec<PromptRecord>,
    pub batches: Vec<PromptBatch>,
    /// Whether some of what woke it was given to an earlier turn that never finished.
    pub retried: bool,
    /// How many more wakes wait for the next turn, beyond what this one takes.
    pub still_waiting: i64,
}

/// The prompt for one turn.
pub fn render(context: &TurnContext, now: DateTime<Utc>) -> String {
    let mut prompt = String::new();
    let project = &context.project;
    let _ = writeln!(prompt, "It is {} (UTC).", now.to_rfc3339());
    let _ = writeln!(
        prompt,
        "You are the Project Manager of the project \"{}\" (project {}).",
        project.name, project.id
    );
    let description: String = project
        .description
        .trim()
        .chars()
        .take(DESCRIPTION_MAX_CHARACTERS)
        .collect();
    if !description.is_empty() {
        let _ = writeln!(prompt, "Its description: {description}");
    }
    prompt.push_str("\nSince your last turn:\n");

    if !context.messages.is_empty() {
        prompt.push_str("\n## Messages\n");
        for message in &context.messages {
            let who = if message.from_elysia {
                "Elysia, Elysium's assistant".to_owned()
            } else {
                format!("{}, a person", message.author)
            };
            let _ = write!(
                prompt,
                "\nFrom {who}, at {}:\n{}\n",
                message.sent_at.to_rfc3339(),
                message.body.trim()
            );
        }
    }

    if !context.records.is_empty() {
        let _ = write!(
            prompt,
            "\n## New on the project's timeline ({})\n",
            context.records.len()
        );
        for record in &context.records {
            render_record(&mut prompt, record);
        }
    }

    if !context.batches.is_empty() {
        prompt.push_str("\n## Finished batches\n");
        for batch in &context.batches {
            let (state, result_label) = match batch.state {
                BatchState::Succeeded => ("succeeded", "Its worker reported"),
                BatchState::Failed => ("failed", "Its worker reported"),
                BatchState::Cancelled => ("was cancelled", "Why"),
                BatchState::Queued
                | BatchState::Paused
                | BatchState::Assigned
                | BatchState::Running => ("is still under way", "So far"),
            };
            let _ = write!(
                prompt,
                "\n- \"{}\" (batch {}) {state}.\n",
                batch.title, batch.id
            );
            if let Some(result) = &batch.result {
                let _ = writeln!(prompt, "  {result_label}: {}", result.trim());
            }
        }
    }

    if context.retried {
        prompt.push_str(
            "\nSome of this was given to a turn of yours that did not finish. Check \
             manager_batches and the project's action items before submitting or proposing \
             anything again.\n",
        );
    }
    if context.still_waiting > 0 {
        let _ = writeln!(
            prompt,
            "\n{} more change(s) wait for your next turn, which starts as soon as this one ends.",
            context.still_waiting
        );
    }
    prompt.push_str(
        "\nDecide what each means for the project's work, act on it, and end with your \
         Summary: line.\n",
    );
    fit(prompt)
}

fn render_record(prompt: &mut String, record: &PromptRecord) {
    let attributed = if record.attributed_by_person {
        "a person put it on the project".to_owned()
    } else {
        format!(
            "Elysia attributed it to the project, {:.0}% sure",
            record.confidence * 100.0
        )
    };
    let _ = write!(
        prompt,
        "\n- [{}] \"{}\" (record {}), {}, from {}; {attributed}.\n",
        record.source_kind,
        record.title,
        record.id,
        record.occurred_at.to_rfc3339(),
        if record.actor.trim().is_empty() {
            "an unnamed source"
        } else {
            record.actor.trim()
        },
    );
    if let Some(summary) = &record.summary {
        let _ = writeln!(prompt, "  Summary: {}", summary.trim());
    }
    let body = record.body.trim();
    if !body.is_empty() {
        let excerpt: String = body.chars().take(EXCERPT_MAX_CHARACTERS).collect();
        let more = if excerpt.len() < body.len() {
            " (more with manager_record)"
        } else {
            ""
        };
        let _ = writeln!(prompt, "  {}{more}", excerpt.replace('\n', "\n  "));
    }
}

/// `prompt`, cut at a character boundary to fit [`PROMPT_MAX_BYTES`] when it does not, saying so.
fn fit(mut prompt: String) -> String {
    if prompt.len() <= PROMPT_MAX_BYTES {
        return prompt;
    }
    let mut cut = PROMPT_MAX_BYTES - CUT_NOTE.len();
    while !prompt.is_char_boundary(cut) {
        cut -= 1;
    }
    prompt.truncate(cut);
    prompt.push_str(CUT_NOTE);
    prompt
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 4, 15, 0, 0)
            .single()
            .expect("a valid instant")
    }

    fn context() -> TurnContext {
        TurnContext {
            project: ProjectBrief {
                id: Uuid::nil(),
                name: "Launch".to_owned(),
                description: "The spring launch.".to_owned(),
            },
            messages: Vec::new(),
            records: Vec::new(),
            batches: Vec::new(),
            retried: false,
            still_waiting: 0,
        }
    }

    fn segment() -> PromptRecord {
        PromptRecord {
            id: Uuid::nil(),
            source_kind: "meeting".to_owned(),
            occurred_at: now(),
            actor: "Ada, Grace".to_owned(),
            title: "Pricing page".to_owned(),
            summary: Some("Ada asked for annual pricing on the page.".to_owned()),
            body: "[0:01] Ada: Can we show annual pricing?".to_owned(),
            confidence: 0.82,
            attributed_by_person: false,
        }
    }

    #[test]
    fn it_names_its_project_and_everything_that_woke_it() {
        let mut woken = context();
        woken.messages.push(PromptMessage {
            author: "Alex".to_owned(),
            from_elysia: false,
            body: "Ship the pricing change this week.".to_owned(),
            sent_at: now(),
        });
        woken.records.push(segment());
        woken.batches.push(PromptBatch {
            id: Uuid::nil(),
            title: "Annual pricing".to_owned(),
            state: BatchState::Failed,
            result: Some("The tests need a Stripe key.".to_owned()),
        });

        let prompt = render(&woken, now());
        assert!(prompt.contains("Project Manager of the project \"Launch\""));
        assert!(prompt.contains("Its description: The spring launch."));
        assert!(prompt.contains("From Alex, a person"));
        assert!(prompt.contains("Ship the pricing change this week."));
        assert!(prompt.contains("New on the project's timeline (1)"));
        assert!(prompt.contains("[meeting] \"Pricing page\""));
        assert!(prompt.contains("82% sure"));
        assert!(prompt.contains("Summary: Ada asked for annual pricing on the page."));
        assert!(prompt.contains("\"Annual pricing\" (batch"));
        assert!(prompt.contains("failed."));
        assert!(prompt.contains("Its worker reported: The tests need a Stripe key."));
        assert!(!prompt.contains("did not finish"));
    }

    #[test]
    fn elysia_and_people_are_told_apart() {
        let mut woken = context();
        woken.messages.push(PromptMessage {
            author: "Elysia".to_owned(),
            from_elysia: true,
            body: "Leadership moved the launch to Friday.".to_owned(),
            sent_at: now(),
        });
        assert!(render(&woken, now()).contains("From Elysia, Elysium's assistant"));
    }

    #[test]
    fn a_retried_turn_is_told_to_check_before_doing_anything_twice() {
        let mut woken = context();
        woken.records.push(segment());
        woken.retried = true;
        woken.still_waiting = 3;
        let prompt = render(&woken, now());
        assert!(prompt.contains("did not finish"));
        assert!(prompt.contains("3 more change(s) wait for your next turn"));
    }

    #[test]
    fn a_long_record_is_quoted_in_part() {
        let mut woken = context();
        let mut long = segment();
        long.body = "word ".repeat(1_000);
        woken.records.push(long);
        let prompt = render(&woken, now());
        assert!(prompt.contains("(more with manager_record)"));
        assert!(prompt.len() < 5_000);
    }

    #[test]
    fn a_prompt_too_long_for_one_argument_is_cut_and_says_so() {
        let mut woken = context();
        for _ in 0..10 {
            woken.messages.push(PromptMessage {
                author: "Alex".to_owned(),
                from_elysia: false,
                body: "é".repeat(19_000),
                sent_at: now(),
            });
        }
        let prompt = render(&woken, now());
        assert!(prompt.len() <= PROMPT_MAX_BYTES);
        assert!(prompt.ends_with(CUT_NOTE));
    }
}
