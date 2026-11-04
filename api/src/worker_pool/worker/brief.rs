// Copyright © 2026 Jalapeno Labs

//! What a worker is told each turn: the same `/goal` prompt, and a system prompt of its standing
//! instructions, its batch, and why this turn starts.
//!
//! Every turn's prompt is `/goal` with [`GOAL_CONDITION`], so Claude Code keeps working across
//! its own turns until the worker has reported on the batch with one of its tools, the one thing
//! the worker can always make true itself. What changes from turn to turn (the batch, an answer,
//! a wait that ended, where its pull requests stand) goes in the system prompt Claude Code appends
//! ([`system_prompt`]), which a resumed session reads afresh. [`render`] is a pure function,
//! tested alone.

use std::fmt::Write as _;

use uuid::Uuid;

use super::setup::PlannedRepository;
use crate::models::work_batch::pull_request::{BatchPullRequest, PullRequestState};
use crate::models::work_batch::worker::Wait;
use crate::models::worker_setup::MergePolicy;

/// The worker's standing instructions, appended to Claude Code's own system prompt.
const INSTRUCTIONS: &str = include_str!("instructions.md");

/// What every turn's `/goal` holds out for. It names what the worker does, never a judgment
/// someone else owns, so the goal's evaluator can see from the conversation when it holds.
pub const GOAL_CONDITION: &str = "You have reported on this work batch with worker_finish or \
     worker_wait from your elysium_worker tools, as your instructions say, as the last thing you \
     did this turn.";

/// Every turn's prompt.
pub fn prompt() -> String {
    format!("/goal {GOAL_CONDITION}")
}

/// Why a turn starts, which the worker is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Situation {
    /// The batch's first turn.
    Start,
    /// It waited on `on`, saying `detail`, and the wait ended: it was time to look again, or
    /// its Project Manager gave `answers`.
    AfterWait {
        on: Wait,
        detail: String,
        answers: Vec<String>,
    },
    /// Its last turn was cut short: Elysium restarted, or it went quiet too long.
    AfterInterruption,
    /// Its last turn stopped because every model credential's usage ran out, which has reset.
    AfterUsageLimit,
    /// Its last turn ended without reporting, failing with `error` when it says so.
    Unreported { error: Option<String> },
}

/// The batch as its worker is told it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch<'a> {
    pub id: Uuid,
    pub title: &'a str,
    pub goal: &'a str,
    pub project_id: Uuid,
    pub project_name: &'a str,
    pub repositories: &'a [PlannedRepository],
    pub action_item_ids: &'a [Uuid],
    /// The pull requests it recorded, in order.
    pub pull_requests: &'a [PullRequestStanding],
}

/// One pull request the worker recorded, as the last look left it, with who merges it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestStanding {
    pub pull_request: BatchPullRequest,
    pub policy: MergePolicy,
}

/// The system prompt a turn appends: the standing instructions, the batch, and `situation`.
pub fn system_prompt(batch: &Batch<'_>, situation: &Situation) -> String {
    let mut prompt = INSTRUCTIONS.to_owned();
    prompt.push('\n');
    prompt.push_str(&render(batch, situation));
    prompt
}

/// The batch and the turn's situation, in Markdown.
pub fn render(batch: &Batch<'_>, situation: &Situation) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "## Your batch\n");
    let _ = writeln!(
        text,
        "\"{}\" (batch {}), for the project \"{}\" (project {}).\n",
        batch.title, batch.id, batch.project_name, batch.project_id
    );
    let _ = writeln!(text, "{}\n", batch.goal.trim());
    if batch.repositories.is_empty() {
        text.push_str("It names no repositories; clone what it needs yourself.\n");
    } else {
        text.push_str("Its repositories, cloned under `repos/`:\n\n");
        for repository in batch.repositories {
            let branch = repository
                .base_branch
                .as_deref()
                .map_or_else(String::new, |branch| format!(", from {branch}"));
            let merges = match repository.merge_policy {
                MergePolicy::Person => "a person merges your pull requests here",
                MergePolicy::Auto => "Elysium merges your pull requests here once they are ready",
            };
            let _ = writeln!(
                text,
                "- `repos/{}`: {}{branch}; {merges}",
                repository.directory, repository.url
            );
        }
    }
    render_pull_requests(&mut text, batch.pull_requests);
    if !batch.action_item_ids.is_empty() {
        let ids: Vec<String> = batch.action_item_ids.iter().map(Uuid::to_string).collect();
        let _ = writeln!(
            text,
            "\nThe action items it works on, in Elysium: {}.",
            ids.join(", ")
        );
    }

    text.push_str("\n## This turn\n\n");
    match situation {
        Situation::Start => text.push_str("This is the batch's first turn. Begin it.\n"),
        Situation::AfterWait {
            on,
            detail,
            answers,
        } => {
            let waited = match on {
                Wait::Review => "a review",
                Wait::Ci => "CI",
                Wait::Question => "an answer to your question",
            };
            let _ = writeln!(text, "You waited on {waited}: {}", detail.trim());
            if answers.is_empty() {
                text.push_str(
                    "\nIt is time to look again. Check where it stands, act on what changed, and \
                     report again.\n",
                );
            } else {
                text.push_str("\nYour Project Manager answered:\n");
                for answer in answers {
                    let _ = writeln!(text, "\n> {}", answer.trim().replace('\n', "\n> "));
                }
                text.push_str("\nCarry on with the answer, and report again.\n");
            }
        }
        Situation::AfterInterruption => text.push_str(
            "Your last turn was cut short before you reported: Elysium restarted, or the turn \
             went quiet too long. Carry on where you left off; check what is already done (your \
             branches, pull requests, and their checks) before doing anything again.\n",
        ),
        Situation::AfterUsageLimit => text.push_str(
            "Your last turn stopped because the model's usage ran out, and it has reset. Carry on \
             where you left off.\n",
        ),
        Situation::Unreported { error } => {
            text.push_str(
                "Your last turn ended without worker_finish or worker_wait. Carry on from where it \
                 stopped, and end this turn with one of them.\n",
            );
            if let Some(error) = error {
                let _ = writeln!(text, "\nIt ended with this error: {}", error.trim());
            }
        }
    }
    text
}

/// Where each pull request the worker recorded stands, so it knows what is left to do.
fn render_pull_requests(text: &mut String, pull_requests: &[PullRequestStanding]) {
    text.push_str("\n## Your pull requests\n\n");
    if pull_requests.is_empty() {
        text.push_str(
            "You have recorded none yet. Record each pull request you open with \
             worker_pull_request.\n",
        );
        return;
    }
    for standing in pull_requests {
        let pull_request = &standing.pull_request;
        let stands = match (pull_request.state, standing.policy) {
            (PullRequestState::Merged, _) if pull_request.merged_by_elysium => {
                "merged by Elysium.".to_owned()
            }
            (PullRequestState::Merged, _) => format!(
                "merged by {}.",
                pull_request.merged_by.as_deref().unwrap_or("someone")
            ),
            (PullRequestState::Closed, _) => "closed without merging.".to_owned(),
            (PullRequestState::Open, MergePolicy::Person) => {
                "open; a person merges it. Address their review until they do.".to_owned()
            }
            (PullRequestState::Open, MergePolicy::Auto) => match &pull_request.held_because {
                Some(reason) => format!("open; Elysium has not merged it yet: {reason}"),
                None => "open; Elysium looks at it when your wait ends.".to_owned(),
            },
        };
        let _ = writeln!(
            text,
            "- {} ({}): {stands}",
            pull_request.reference(),
            pull_request.url
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> PlannedRepository {
        PlannedRepository {
            url: "https://github.com/JalapenoLabs/Elysium.git".to_owned(),
            clone_url: "https://github.com/JalapenoLabs/Elysium.git".to_owned(),
            directory: "Elysium".to_owned(),
            base_branch: Some("main".to_owned()),
            setup_script: None,
            merge_policy: MergePolicy::Auto,
        }
    }

    fn batch(repositories: &[PlannedRepository]) -> Batch<'_> {
        Batch {
            id: Uuid::nil(),
            title: "Fix the login bug",
            goal: "Users cannot sign in with a passkey; fix it and open a pull request.",
            project_id: Uuid::nil(),
            project_name: "Elysium",
            repositories,
            action_item_ids: &[],
            pull_requests: &[],
        }
    }

    fn standing(
        number: i32,
        state: PullRequestState,
        policy: MergePolicy,
        held_because: Option<&str>,
    ) -> PullRequestStanding {
        let merged = state == PullRequestState::Merged;
        PullRequestStanding {
            pull_request: BatchPullRequest {
                id: Uuid::nil(),
                work_batch_id: Uuid::nil(),
                repository: "JalapenoLabs/Elysium".to_owned(),
                number,
                url: format!("https://github.com/JalapenoLabs/Elysium/pull/{number}"),
                title: "Fix passkeys".to_owned(),
                state,
                merged_at: None,
                merged_by: merged.then(|| "alex".to_owned()),
                merged_by_elysium: false,
                merge_method: None,
                held_because: held_because.map(str::to_owned),
                looked_at: None,
                timeline_record_id: None,
                created_by: Uuid::nil(),
                created_at: chrono::Utc::now(),
            },
            policy,
        }
    }

    #[test]
    fn every_turn_holds_out_for_a_report_the_worker_makes_itself() {
        assert!(prompt().starts_with("/goal "));
        assert!(GOAL_CONDITION.contains("worker_finish"));
        assert!(GOAL_CONDITION.contains("worker_wait"));
        assert!(
            GOAL_CONDITION.chars().count() < 4_000,
            "Claude Code takes a condition of at most 4,000 characters"
        );
    }

    #[test]
    fn the_first_turn_carries_the_goal_and_where_each_repository_is() {
        let repositories = [repository()];
        let text = system_prompt(&batch(&repositories), &Situation::Start);
        assert!(text.starts_with("# You are a worker"), "instructions first");
        assert!(text.contains("Users cannot sign in with a passkey"));
        assert!(text.contains(
            "`repos/Elysium`: https://github.com/JalapenoLabs/Elysium.git, from main; Elysium \
             merges your pull requests here once they are ready"
        ));
        assert!(text.contains("You have recorded none yet"));
        assert!(text.contains("first turn"));
    }

    #[test]
    fn each_pull_request_says_who_merges_it_and_what_holds_it() {
        let pull_requests = [
            standing(1, PullRequestState::Merged, MergePolicy::Person, None),
            standing(
                2,
                PullRequestState::Open,
                MergePolicy::Auto,
                Some("1 review thread is not resolved."),
            ),
            standing(3, PullRequestState::Open, MergePolicy::Person, None),
            standing(4, PullRequestState::Closed, MergePolicy::Auto, None),
        ];
        let text = render(
            &Batch {
                pull_requests: &pull_requests,
                ..batch(&[])
            },
            &Situation::Start,
        );
        assert!(text.contains("JalapenoLabs/Elysium#1 (https://github.com/JalapenoLabs/Elysium/pull/1): merged by alex."));
        assert!(text.contains(
            "#2 (https://github.com/JalapenoLabs/Elysium/pull/2): open; Elysium has not merged it \
             yet: 1 review thread is not resolved."
        ));
        assert!(text.contains(
            "#3 (https://github.com/JalapenoLabs/Elysium/pull/3): open; a person merges it."
        ));
        assert!(text.contains(
            "#4 (https://github.com/JalapenoLabs/Elysium/pull/4): closed without merging."
        ));
    }

    #[test]
    fn an_answer_is_quoted_word_for_word() {
        let text = render(
            &batch(&[]),
            &Situation::AfterWait {
                on: Wait::Question,
                detail: "Should the fix also cover recovery codes?".to_owned(),
                answers: vec!["Yes.\nBoth flows.".to_owned()],
            },
        );
        assert!(text.contains("You waited on an answer to your question"));
        assert!(text.contains("> Yes.\n> Both flows."));
        assert!(text.contains("clone what it needs yourself"));
    }

    #[test]
    fn a_turn_that_did_not_report_is_told_why_it_runs_again() {
        let text = render(
            &batch(&[]),
            &Situation::Unreported {
                error: Some("tool crashed".to_owned()),
            },
        );
        assert!(text.contains("without worker_finish or worker_wait"));
        assert!(text.contains("tool crashed"));
    }
}
