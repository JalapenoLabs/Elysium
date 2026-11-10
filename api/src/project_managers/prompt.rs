// Copyright © 2026 Jalapeno Labs

//! What a Project Manager is told at the start of a turn.
//!
//! Its standing instructions live in `instructions.md` beside this file, versioned with the code,
//! and are appended to Claude Code's system prompt on every turn. The prompt names its project,
//! carries the project's brief whole (its goals, decisions, and what is out of scope, which every
//! batch serves), names the docs linked to it, says which of its batches and changes wait for a
//! person, as its project's level of autonomy decides, and where its issues file ([`Writes`]), and
//! carries everything that woke it: the messages, someone else's edits to the brief, the action
//! items a person assigned it, the records newly on its timeline, the batches that finished, and
//! the promises found on its project or whose work finished. Every turn also carries each promise
//! still owed a follow-up, woken for or not, so a turn that failed never strands one. It is a plain
//! function of its inputs, so it is tested without a runner.
//!
//! Its session is resumed for every turn and kept indefinitely, so what one turn brings stays in
//! it: a record is quoted in part ([`EXCERPT_MAX_CHARACTERS`]) and the whole prompt stays under
//! [`PROMPT_MAX_BYTES`]; the Project Manager reads any record in full with `manager_record`.

use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::action_item::ActionItemState;
use crate::models::project_autonomy::Autonomy;
use crate::models::promise::PromiseState;
use crate::models::work_batch::BatchState;
use crate::models::work_batch::lineage::Origin;

/// Its standing instructions.
pub const INSTRUCTIONS: &str = include_str!("instructions.md");

/// How much of a record's body the prompt quotes, in characters.
pub const EXCERPT_MAX_CHARACTERS: usize = 800;

/// How much of a project's description the prompt quotes, in characters.
const DESCRIPTION_MAX_CHARACTERS: usize = 1_000;

/// How much of the brief's body the prompt carries, in characters: a brief this long is several,
/// and the rest is a `manager_page` call away.
const BRIEF_MAX_CHARACTERS: usize = 20_000;

/// How many of a project's docs the prompt names, newest published first. Each is only a title, so
/// these fit however long they run; the rest are a `manager_pages` call away.
const PAGES_MAX: usize = 50;

/// The longest prompt, in bytes, about 25,000 tokens: one busy wake (a day of records, a
/// batch's long report) must not crowd out the session it is added to.
pub const PROMPT_MAX_BYTES: usize = 100_000;

/// What the end of a prompt cut to fit says.
const CUT_NOTE: &str = "\n\n(The rest did not fit in this prompt; read it with manager_timeline and manager_batches.)\n";

/// The project a turn works for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptProject {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    /// The repositories it declares for its workers, which its batches name.
    pub repositories: Vec<String>,
    /// Its brief as it stands.
    pub brief: PromptBrief,
    /// The other knowledge base pages linked to it, its docs, newest published first.
    pub pages: Vec<PromptPage>,
}

/// A project's brief, which the turn reads whole and keeps current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBrief {
    pub page_id: Uuid,
    /// The version an edit of it names as its `baseVersion`.
    pub version: i32,
    /// Markdown.
    pub body: String,
    /// Its goals as their headings spell them (`crate::models::project_brief::goals`).
    pub goals: Vec<String>,
}

/// Someone else's publish of the brief since the turn before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBriefEdit {
    /// Who published it, by name.
    pub author: String,
    pub version: i32,
    pub published_at: DateTime<Utc>,
}

/// A knowledge base page linked to its project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPage {
    pub id: Uuid,
    pub title: String,
    pub published_at: DateTime<Utc>,
}

/// A message a person or Elysia sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMessage {
    /// Its wake's id, which a batch it calls for names (`messageIds`).
    pub id: Uuid,
    /// Who wrote it, by name.
    pub author: String,
    pub from_elysia: bool,
    pub body: String,
    pub sent_at: DateTime<Utc>,
}

/// An action item a person assigned it to plan, which its project leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptAssignment {
    /// The item's id, which a batch planned for it names (`actionItemIds`).
    pub item_id: Uuid,
    pub title: String,
    pub notes: String,
    pub state: ActionItemState,
    /// Who assigned it, by name.
    pub assigned_by: String,
    pub assigned_at: DateTime<Utc>,
    /// The issues and pull requests it is linked to, each by its key and URL, the primary first.
    pub links: Vec<String>,
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

/// How a turn's batches and changes reach the work, and where the issues it files go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writes {
    /// Its project's level, which says which of its batches and changes wait for a person.
    pub autonomy: Autonomy,
    /// Where its issues file, such as `Jira project ELY, which this project names`, or why it
    /// cannot file any.
    pub tracker: Result<String, String>,
}

/// Everything one turn is told.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnContext {
    pub project: PromptProject,
    pub writes: Writes,
    pub messages: Vec<PromptMessage>,
    /// Someone else's publishes of the brief, oldest first.
    pub brief_edits: Vec<PromptBriefEdit>,
    pub assignments: Vec<PromptAssignment>,
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
    render_brief(&mut prompt, &context.project.brief);
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
                "\nFrom {who}, at {} (message {}):\n{}\n",
                message.sent_at.to_rfc3339(),
                message.id,
                message.body.trim()
            );
        }
    }

    if !context.brief_edits.is_empty() {
        render_brief_edits(
            &mut prompt,
            &context.brief_edits,
            context.project.brief.version,
        );
    }

    if !context.assignments.is_empty() {
        let _ = write!(
            prompt,
            "\n## Assigned to you ({})\n\nA person chose these action items for you to plan. \
             Plan each into the batch or batches it needs with manager_submit_batch, naming it in \
             actionItemIds, or say in your reply why it cannot be planned yet.\n",
            context.assignments.len()
        );
        for assignment in &context.assignments {
            render_assignment(&mut prompt, assignment);
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
            render_batch(&mut prompt, batch, context.project.brief.version);
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
fn render_project(prompt: &mut String, project: &PromptProject) {
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
    render_pages(prompt, &project.pages);
}

/// The project's docs by title, whatever woke it: a page linked since its last turn wakes nothing,
/// so this is how it learns of one.
fn render_pages(prompt: &mut String, pages: &[PromptPage]) {
    if pages.is_empty() {
        return;
    }
    prompt.push_str("Its docs, the knowledge base pages linked to it, which manager_page reads:\n");
    for page in pages.iter().take(PAGES_MAX) {
        let _ = writeln!(
            prompt,
            "- {} (page {}, published {})",
            page.title,
            page.id,
            page.published_at.to_rfc3339()
        );
    }
    if pages.len() > PAGES_MAX {
        let more = pages.len() - PAGES_MAX;
        let _ = writeln!(prompt, "- and {more} more, which manager_pages lists");
    }
}

/// The project's brief whole, with its goals as a batch names them, on every turn: it is what every
/// batch serves, and what a person corrects to change the project's direction.
fn render_brief(prompt: &mut String, brief: &PromptBrief) {
    let _ = write!(
        prompt,
        "\n## The project's brief (page {}, version {})\n\n",
        brief.page_id, brief.version
    );
    if brief.goals.is_empty() {
        prompt.push_str(
            "It names no goals yet, so no batch can serve one: ask the person what the project is \
             for before you submit any.\n",
        );
    } else {
        let goals: Vec<String> = brief
            .goals
            .iter()
            .map(|goal| format!("\"{goal}\""))
            .collect();
        let _ = writeln!(
            prompt,
            "Its goals, one of which every batch names as the goal it serves: {}.",
            goals.join(", ")
        );
    }
    let _ = writeln!(
        prompt,
        "Keep it current with page_save against version {}: record what is decided and what is \
         out of scope as you learn it. When a save answers page_changed, someone edited it \
         meanwhile: read it again and merge your change into what it says now, keeping theirs.",
        brief.version
    );
    // Set apart, since the brief's own headings would otherwise read as this prompt's sections.
    let body = brief.body.trim();
    let shown: String = body.chars().take(BRIEF_MAX_CHARACTERS).collect();
    let _ = writeln!(prompt, "\n<brief>\n{shown}\n</brief>");
    if shown.len() < body.len() {
        let _ = writeln!(
            prompt,
            "\n(The rest of the brief did not fit; read it with manager_page.)"
        );
    }
}

/// Who published the brief since the last turn. A person's edit is direction: it wins over the
/// Project Manager's plan.
fn render_brief_edits(prompt: &mut String, edits: &[PromptBriefEdit], current: i32) {
    prompt.push_str("\n## The brief changed\n\n");
    for edit in edits {
        let _ = writeln!(
            prompt,
            "- {} published version {} at {}.",
            edit.author,
            edit.version,
            edit.published_at.to_rfc3339()
        );
    }
    let _ = writeln!(
        prompt,
        "Its version {current} is above. What they changed is their direction: follow it, \
         cancel waiting batches it makes wrong, and never write back over it."
    );
}

/// Which of its batches and changes wait for a person, as its project's level says, and where its
/// issues file.
fn render_writes(prompt: &mut String, writes: &Writes) {
    let autonomy = writes.autonomy;
    let _ = writeln!(
        prompt,
        "This project's level of autonomy is {}, which only a person changes.",
        autonomy.words()
    );
    if autonomy.queues(Origin::HumanRooted) {
        prompt.push_str(
            "A human-rooted batch you submit goes straight to the queue for a worker, with no \
             person approving it first. A found batch still waits for a person's approval.\n",
        );
    } else {
        prompt.push_str("Every batch you submit waits for a person to approve it.\n");
    }
    if autonomy.writes_directly() {
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
/// waits on, and the promises a failed one leaves. A rejection's reason is to go into the brief,
/// version `brief_version`, so it outlives the session.
fn render_batch(prompt: &mut String, batch: &PromptBatch, brief_version: i32) {
    let (state, result_label) = match batch.state {
        BatchState::Succeeded => ("succeeded", "Its worker reported"),
        BatchState::Failed => ("failed", "Its worker reported"),
        BatchState::Cancelled => ("was cancelled", "Why"),
        // A person declined it: what they said is the direction to follow, and it is never
        // submitted again as it was.
        BatchState::Rejected => (
            "was rejected by a person, who said why. Follow what they said: plan again, and submit \
             it changed or not at all, never as it was",
            "Why",
        ),
        BatchState::Proposed
        | BatchState::Queued
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
    if batch.state == BatchState::Rejected {
        let _ = writeln!(
            prompt,
            "  Record this under ## Decisions in the brief, with page_save against version \
             {brief_version}: what was rejected, the person's reason in their words, and when."
        );
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

fn render_assignment(prompt: &mut String, assignment: &PromptAssignment) {
    let _ = write!(
        prompt,
        "\n- \"{}\" (item {}), {}, assigned by {} at {}.\n",
        assignment.title,
        assignment.item_id,
        item_state_words(assignment.state),
        assignment.assigned_by,
        assignment.assigned_at.to_rfc3339(),
    );
    if !assignment.links.is_empty() {
        let _ = writeln!(prompt, "  Linked to: {}", assignment.links.join(", "));
    }
    let notes = assignment.notes.trim();
    if !notes.is_empty() {
        let excerpt: String = notes.chars().take(EXCERPT_MAX_CHARACTERS).collect();
        let more = if excerpt.len() < notes.len() {
            " (more with work_item)"
        } else {
            ""
        };
        let _ = writeln!(prompt, "  {}{more}", excerpt.replace('\n', "\n  "));
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
        Some(PromptWork::Item { id, title, state }) => format!(
            "It waits on action item \"{title}\" (item {id}), which is {}.",
            item_state_words(*state)
        ),
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

/// How an item's state reads in a sentence.
const fn item_state_words(state: ActionItemState) -> &'static str {
    match state {
        ActionItemState::Inbox => "in the inbox",
        ActionItemState::Open => "open",
        ActionItemState::Resolved => "resolved",
        ActionItemState::Dismissed => "dismissed",
    }
}

/// How a batch's state reads in a sentence.
const fn batch_state_words(state: BatchState) -> &'static str {
    match state {
        BatchState::Proposed => "waiting for a person's approval",
        BatchState::Queued => "queued",
        BatchState::Paused => "paused",
        BatchState::Assigned => "waiting for its worker",
        BatchState::Running => "running",
        BatchState::Succeeded => "done",
        BatchState::Failed => "failed",
        BatchState::Cancelled => "cancelled",
        BatchState::Rejected => "rejected by a person",
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
            project: PromptProject {
                id: Uuid::nil(),
                name: "Launch".to_owned(),
                description: "The spring launch.".to_owned(),
                repositories: vec!["https://github.com/JalapenoLabs/Launch.git".to_owned()],
                brief: PromptBrief {
                    page_id: Uuid::nil(),
                    version: 3,
                    body: "## Goals\n\n### Annual pricing\n\nAda asked for it.".to_owned(),
                    goals: vec!["Annual pricing".to_owned()],
                },
                pages: Vec::new(),
            },
            writes: Writes {
                autonomy: Autonomy::ApproveEveryBatch,
                tracker: Ok("Jira project ELY, where this project's issues already are".to_owned()),
            },
            messages: Vec::new(),
            brief_edits: Vec::new(),
            assignments: Vec::new(),
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

    fn linked_page(title: &str) -> PromptPage {
        PromptPage {
            id: Uuid::nil(),
            title: title.to_owned(),
            published_at: now(),
        }
    }

    #[test]
    fn it_names_the_projects_docs_on_every_turn_and_says_nothing_of_none() {
        assert!(!render(&context(), now()).contains("Its docs"));

        let mut linked = context();
        linked.project.pages.push(linked_page("Launch checklist"));

        let prompt = render(&linked, now());
        assert!(prompt.contains("Its docs, the knowledge base pages linked to it"));
        assert!(prompt.contains("- Launch checklist (page 00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn it_names_the_newest_docs_and_points_to_the_rest() {
        let mut linked = context();
        linked.project.pages = (0..PAGES_MAX + 3)
            .map(|index| linked_page(&format!("Doc {index}")))
            .collect();

        let prompt = render(&linked, now());
        assert!(prompt.contains(&format!("- Doc {} ", PAGES_MAX - 1)));
        assert!(!prompt.contains(&format!("- Doc {PAGES_MAX} ")));
        assert!(prompt.contains("- and 3 more, which manager_pages lists"));
    }

    #[test]
    fn every_turn_reads_the_brief_whole_with_the_goals_a_batch_names() {
        let prompt = render(&context(), now());
        assert!(prompt.contains(
            "## The project's brief (page 00000000-0000-0000-0000-000000000000, version 3)"
        ));
        assert!(
            prompt.contains(
                "one of which every batch names as the goal it serves: \"Annual pricing\"."
            )
        );
        assert!(
            prompt
                .contains("<brief>\n## Goals\n\n### Annual pricing\n\nAda asked for it.\n</brief>")
        );
        assert!(prompt.contains("page_save against version 3"));
        assert!(prompt.contains("read it again and merge"));
        assert!(
            prompt.find("## The project's brief") < prompt.find("Since your last turn"),
            "the brief comes before what woke the turn, so a long prompt is never cut through it"
        );

        let mut empty = context();
        empty.project.brief.goals.clear();
        assert!(render(&empty, now()).contains("It names no goals yet"));

        let mut long = context();
        long.project.brief.body = "word ".repeat(BRIEF_MAX_CHARACTERS);
        let prompt = render(&long, now());
        assert!(prompt.contains("The rest of the brief did not fit; read it with manager_page."));
    }

    #[test]
    fn someone_elses_edit_of_the_brief_is_direction_to_follow() {
        let mut woken = context();
        woken.brief_edits.push(PromptBriefEdit {
            author: "Alex".to_owned(),
            version: 3,
            published_at: now(),
        });
        let prompt = render(&woken, now());
        assert!(prompt.contains("## The brief changed"));
        assert!(prompt.contains("- Alex published version 3 at 2026-11-04T15:00:00+00:00."));
        assert!(prompt.contains("follow it"));
        assert!(!render(&context(), now()).contains("## The brief changed"));
    }

    #[test]
    fn a_rejections_reason_reaches_the_prompt_and_goes_into_the_brief() {
        let mut woken = context();
        woken.batches.push(PromptBatch {
            id: Uuid::nil(),
            title: "Dark mode".to_owned(),
            state: BatchState::Rejected,
            result: Some("Not this quarter; pricing comes first.".to_owned()),
            waiting_promises: Vec::new(),
            question: None,
        });
        let prompt = render(&woken, now());
        assert!(prompt.contains("was rejected by a person"));
        assert!(prompt.contains("Why: Not this quarter; pricing comes first."));
        assert!(prompt.contains(
            "Record this under ## Decisions in the brief, with page_save against version 3"
        ));
    }

    #[test]
    fn it_says_when_its_changes_apply_at_once_and_why_it_cannot_file() {
        let mut direct = context();
        direct.writes = Writes {
            autonomy: Autonomy::FullAutonomy,
            tracker: Err("this project names no tracker".to_owned()),
        };

        let prompt = render(&direct, now());
        assert!(prompt.contains("level of autonomy is Full autonomy"));
        assert!(prompt.contains("A human-rooted batch you submit goes straight to the queue"));
        assert!(prompt.contains("A found batch still waits for a person's approval."));
        assert!(prompt.contains("apply as you make them"));
        assert!(prompt.contains("You cannot file issues: this project names no tracker."));
    }

    #[test]
    fn it_says_every_batch_waits_at_the_default() {
        let prompt = render(&context(), now());
        assert!(prompt.contains("level of autonomy is Approve every batch"));
        assert!(prompt.contains("Every batch you submit waits for a person to approve it."));
        assert!(!prompt.contains("goes straight to the queue"));
    }

    #[test]
    fn it_says_its_human_rooted_batches_run_while_its_changes_wait() {
        let mut found_only = context();
        found_only.writes.autonomy = Autonomy::ApproveFoundWork;

        let prompt = render(&found_only, now());
        assert!(prompt.contains("level of autonomy is Approve found work only"));
        assert!(prompt.contains("A human-rooted batch you submit goes straight to the queue"));
        assert!(prompt.contains("wait for a person to approve them"));
    }

    #[test]
    fn it_names_its_project_and_everything_that_woke_it() {
        let mut woken = context();
        woken.messages.push(PromptMessage {
            id: Uuid::nil(),
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
        assert!(prompt.contains(&format!("(message {})", Uuid::nil())));
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
    fn an_assigned_item_is_named_with_who_assigned_it_and_how_to_plan_it() {
        let mut assigned = context();
        assert!(!render(&assigned, now()).contains("Assigned to you"));
        assigned.assignments.push(PromptAssignment {
            item_id: Uuid::nil(),
            title: "Annual pricing".to_owned(),
            notes: "Ada wants yearly plans on the pricing page.".to_owned(),
            state: ActionItemState::Open,
            assigned_by: "Alex".to_owned(),
            assigned_at: now(),
            links: vec!["ELY-12 (https://example.atlassian.net/browse/ELY-12)".to_owned()],
        });

        let prompt = render(&assigned, now());
        assert!(prompt.contains("## Assigned to you (1)"));
        assert!(prompt.contains("naming it in actionItemIds"));
        assert!(prompt.contains(&format!(
            "\"Annual pricing\" (item {}), open, assigned by Alex",
            Uuid::nil()
        )));
        assert!(prompt.contains("Linked to: ELY-12 (https://example.atlassian.net/browse/ELY-12)"));
        assert!(prompt.contains("Ada wants yearly plans on the pricing page."));
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
            id: Uuid::nil(),
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
    fn a_prompt_over_its_limit_is_cut_and_says_so() {
        let mut woken = context();
        for _ in 0..10 {
            woken.messages.push(PromptMessage {
                id: Uuid::nil(),
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
