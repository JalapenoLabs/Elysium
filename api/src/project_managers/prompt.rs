// Copyright © 2026 Jalapeno Labs

//! What a Project Manager is told at the start of a turn.
//!
//! Its standing instructions live in `instructions.md` beside this file, versioned with the code,
//! and are appended to Claude Code's system prompt on every turn. The prompt names its project,
//! says how its changes reach the work and where its issues file ([`Writes`]), and carries
//! everything that woke it: the messages, the records newly on its timeline, the batches that
//! finished, and the promises found on its project or whose work finished. Every turn also carries
//! each promise still owed a follow-up, woken for or not, so a turn that failed never strands one.
//! It is a plain function of its inputs, so it is tested without a runner.
//!
//! The runner passes the prompt as one argument, and Linux allows one argument 128 KiB, so a
//! record is quoted in part ([`EXCERPT_MAX_CHARACTERS`]) and the whole prompt stays under
//! [`PROMPT_MAX_BYTES`]; the Project Manager reads any record in full with `manager_record`.

use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::action_item::ActionItemState;
use crate::models::promise::PromiseState;
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
    /// The repositories it declares for its workers, which its batches name.
    pub repositories: Vec<String>,
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
    /// The promises still waiting on it, which a batch that failed leaves for the Project Manager
    /// to tie to the work that replaces it.
    pub waiting_promises: Vec<Uuid>,
    /// The question its worker waits on, which the Project Manager answers.
    pub question: Option<String>,
}

/// A promise its Project Manager has to act on: one just found, which waits for its work, or one
/// whose work finished, which is owed its follow-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPromise {
    pub id: Uuid,
    /// Who made it: the person whose message it was made in.
    pub person: String,
    pub recipient: String,
    pub summary: String,
    /// Where it was made: the message's record, its title, and what it said.
    pub record_id: Uuid,
    pub record_title: String,
    pub said: String,
    /// `Waiting` or `Due`; others are the person's to decide and are never shown.
    pub state: PromiseState,
    pub work: Option<PromptWork>,
}

/// The work a promise waits on, as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptWork {
    Batch {
        id: Uuid,
        title: String,
        state: BatchState,
        result: Option<String>,
    },
    Item {
        id: Uuid,
        title: String,
        state: ActionItemState,
    },
}

/// How a turn's changes reach the work, and where the issues it files go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writes {
    /// Whether its changes apply as it makes them, rather than waiting for a person to approve.
    pub direct: bool,
    /// Where its issues file, such as `Jira project ELY, which this project names`, or why it
    /// cannot file any.
    pub tracker: Result<String, String>,
}

/// Everything one turn is told.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnContext {
    pub project: ProjectBrief,
    pub writes: Writes,
    pub messages: Vec<PromptMessage>,
    pub records: Vec<PromptRecord>,
    pub batches: Vec<PromptBatch>,
    pub promises: Vec<PromptPromise>,
    /// Whether some of what woke it was given to an earlier turn that never finished.
    pub retried: bool,
    /// How many more wakes wait for the next turn, beyond what this one takes.
    pub still_waiting: i64,
}

/// The prompt for one turn.
pub fn render(context: &TurnContext, now: DateTime<Utc>) -> String {
    let mut prompt = String::new();
    let _ = writeln!(prompt, "It is {} (UTC).", now.to_rfc3339());
    render_project(&mut prompt, &context.project);
    render_writes(&mut prompt, &context.writes);
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
        prompt.push_str("\n## Batches\n");
        for batch in &context.batches {
            render_batch(&mut prompt, batch);
        }
    }

    if !context.promises.is_empty() {
        let _ = write!(prompt, "\n## Promises ({})\n", context.promises.len());
        for promise in &context.promises {
            render_promise(&mut prompt, promise);
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

/// Which project it manages, and what the project is.
fn render_project(prompt: &mut String, project: &ProjectBrief) {
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
    if project.repositories.is_empty() {
        prompt.push_str(
            "It declares no repositories for its workers yet; a batch may still name any it \
             needs.\n",
        );
    } else {
        let _ = writeln!(
            prompt,
            "The repositories it declares for its workers, which a batch names as it needs: {}",
            project.repositories.join(", ")
        );
    }
}

/// Whether its changes apply at once, and where its issues file.
fn render_writes(prompt: &mut String, writes: &Writes) {
    if writes.direct {
        prompt.push_str(
            "The changes you make with manager_make_changes apply as you make them: this \
             project lets you write to its action items and trackers yourself.\n",
        );
    } else {
        prompt.push_str(
            "The changes you make with manager_make_changes wait for a person to approve them: \
             this project has you propose them.\n",
        );
    }
    match &writes.tracker {
        Ok(tracker) => {
            let _ = writeln!(prompt, "Issues you file go to {tracker}.");
        }
        Err(reason) => {
            let _ = writeln!(prompt, "You cannot file issues: {reason}.");
        }
    }
}

/// One batch that woke the turn: where it stands, what its worker reported, the question it
/// waits on, and the promises a failed one leaves.
fn render_batch(prompt: &mut String, batch: &PromptBatch) {
    let (state, result_label) = match batch.state {
        BatchState::Succeeded => ("succeeded", "Its worker reported"),
        BatchState::Failed => ("failed", "Its worker reported"),
        BatchState::Cancelled => ("was cancelled", "Why"),
        BatchState::Queued | BatchState::Paused | BatchState::Assigned | BatchState::Running => {
            ("is still under way", "So far")
        }
    };
    let _ = write!(
        prompt,
        "\n- \"{}\" (batch {}) {state}.\n",
        batch.title, batch.id
    );
    if let Some(result) = &batch.result {
        let _ = writeln!(prompt, "  {result_label}: {}", result.trim());
    }
    if let Some(question) = &batch.question {
        let _ = writeln!(
            prompt,
            "  Its worker waits on your answer to: {}\n  Answer it with manager_answer_batch, \
             or ask the person when only they can decide.",
            question.trim()
        );
    }
    if !batch.waiting_promises.is_empty() {
        let ids: Vec<String> = batch.waiting_promises.iter().map(Uuid::to_string).collect();
        let _ = writeln!(
            prompt,
            "  Promises still wait on it: {}. Tie each to the work that replaces it with \
             manager_tie_promise.",
            ids.join(", ")
        );
    }
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

fn render_promise(prompt: &mut String, promise: &PromptPromise) {
    let _ = write!(
        prompt,
        "\n- Promise {}: {} promised {} \"{}\", in \"{}\" (record {}).\n",
        promise.id,
        promise.person,
        promise.recipient,
        promise.summary,
        promise.record_title,
        promise.record_id,
    );
    let said: String = promise
        .said
        .trim()
        .chars()
        .take(EXCERPT_MAX_CHARACTERS)
        .collect();
    if !said.is_empty() {
        let _ = writeln!(prompt, "  They wrote: {}", said.replace('\n', "\n  "));
    }
    let work = match &promise.work {
        None => "It waits on no work yet.".to_owned(),
        Some(PromptWork::Batch {
            id,
            title,
            state,
            result,
        }) => {
            let reported = result
                .as_deref()
                .map(|result| format!(" Its worker reported: {}", result.trim()))
                .unwrap_or_default();
            format!(
                "It waits on batch \"{title}\" (batch {id}), which is {}.{reported}",
                batch_state_words(*state)
            )
        }
        Some(PromptWork::Item { id, title, state }) => {
            let state = match state {
                ActionItemState::Inbox => "in the inbox",
                ActionItemState::Open => "open",
                ActionItemState::Resolved => "resolved",
                ActionItemState::Dismissed => "dismissed",
            };
            format!("It waits on action item \"{title}\" (item {id}), which is {state}.")
        }
    };
    let _ = writeln!(prompt, "  {work}");
    let next = match promise.state {
        PromiseState::Due => {
            "Its work is done: draft the follow-up in their voice with manager_draft_follow_up."
        }
        PromiseState::Waiting if promise.work.is_none() => {
            "Tie it to the batch or action item that keeps it with manager_tie_promise."
        }
        PromiseState::Waiting
        | PromiseState::Drafted
        | PromiseState::Kept
        | PromiseState::Dismissed => "Nothing to do until its work finishes.",
    };
    let _ = writeln!(prompt, "  {next}");
}

/// How a batch's state reads in a sentence.
const fn batch_state_words(state: BatchState) -> &'static str {
    match state {
        BatchState::Queued => "queued",
        BatchState::Paused => "paused",
        BatchState::Assigned => "waiting for its worker",
        BatchState::Running => "running",
        BatchState::Succeeded => "done",
        BatchState::Failed => "failed",
        BatchState::Cancelled => "cancelled",
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
                repositories: vec!["https://github.com/JalapenoLabs/Launch.git".to_owned()],
            },
            writes: Writes {
                direct: false,
                tracker: Ok("Jira project ELY, where this project's issues already are".to_owned()),
            },
            messages: Vec::new(),
            records: Vec::new(),
            batches: Vec::new(),
            promises: Vec::new(),
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
    fn it_says_when_its_changes_apply_at_once_and_why_it_cannot_file() {
        let mut direct = context();
        direct.writes = Writes {
            direct: true,
            tracker: Err("this project names no tracker".to_owned()),
        };

        let prompt = render(&direct, now());
        assert!(prompt.contains("apply as you make them"));
        assert!(prompt.contains("You cannot file issues: this project names no tracker."));
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
            waiting_promises: Vec::new(),
            question: None,
        });

        let prompt = render(&woken, now());
        assert!(prompt.contains("Project Manager of the project \"Launch\""));
        assert!(prompt.contains("wait for a person to approve them"));
        assert!(prompt.contains(
            "Issues you file go to Jira project ELY, where this project's issues already are."
        ));
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

    fn promise(state: PromiseState, work: Option<PromptWork>) -> PromptPromise {
        PromptPromise {
            id: Uuid::nil(),
            person: "Alex".to_owned(),
            recipient: "Ada".to_owned(),
            summary: "when annual pricing is live".to_owned(),
            record_id: Uuid::nil(),
            record_title: "Ada in a direct message in Mooreslab".to_owned(),
            said: "Sure, I'll let you know here once it's live.".to_owned(),
            state,
            work,
        }
    }

    #[test]
    fn a_promise_says_what_it_needs_next() {
        let mut woken = context();
        woken.promises.push(promise(PromiseState::Waiting, None));
        woken.promises.push(promise(
            PromiseState::Due,
            Some(PromptWork::Batch {
                id: Uuid::nil(),
                title: "Annual pricing".to_owned(),
                state: BatchState::Succeeded,
                result: Some("Deployed to production.".to_owned()),
            }),
        ));
        let prompt = render(&woken, now());
        assert!(prompt.contains("## Promises (2)"));
        assert!(prompt.contains("Alex promised Ada \"when annual pricing is live\""));
        assert!(prompt.contains("They wrote: Sure, I'll let you know here once it's live."));
        assert!(prompt.contains("It waits on no work yet."));
        assert!(prompt.contains("manager_tie_promise"));
        assert!(prompt.contains("batch \"Annual pricing\""));
        assert!(prompt.contains("which is done. Its worker reported: Deployed to production."));
        assert!(prompt.contains("draft the follow-up in their voice with manager_draft_follow_up"));
    }

    #[test]
    fn a_worker_waiting_on_a_question_asks_for_an_answer() {
        let mut woken = context();
        woken.batches.push(PromptBatch {
            id: Uuid::nil(),
            title: "Annual pricing".to_owned(),
            state: BatchState::Running,
            result: None,
            waiting_promises: Vec::new(),
            question: Some("Should annual plans get the launch discount too?".to_owned()),
        });
        let prompt = render(&woken, now());
        assert!(prompt.contains("is still under way"));
        assert!(prompt.contains(
            "Its worker waits on your answer to: Should annual plans get the launch discount too?"
        ));
        assert!(prompt.contains("manager_answer_batch"));
        assert!(prompt.contains(
            "The repositories it declares for its workers, which a batch names as it needs: \
             https://github.com/JalapenoLabs/Launch.git"
        ));
    }

    #[test]
    fn a_failed_batch_names_the_promises_still_waiting_on_it() {
        let mut woken = context();
        let waiting = Uuid::now_v7();
        woken.batches.push(PromptBatch {
            id: Uuid::nil(),
            title: "Annual pricing".to_owned(),
            state: BatchState::Failed,
            result: None,
            waiting_promises: vec![waiting],
            question: None,
        });
        let prompt = render(&woken, now());
        assert!(prompt.contains(&format!("Promises still wait on it: {waiting}")));
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
