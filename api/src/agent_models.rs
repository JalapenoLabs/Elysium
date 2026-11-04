// Copyright © 2026 Jalapeno Labs

//! The model and effort every agent runs on: what there is to set, each one's default, and the
//! one place a turn's model is resolved.
//!
//! An agent is anything whose work is a turn on the runner, named as it is in code:
//!
//! - **a workflow step** whose work is an agent turn ([`crate::workflows::Step::RUNS_MODEL`]),
//!   named by its step name, such as `correct_transcript`;
//! - **an agent outside the step framework** ([`Agent`]): Elysia, coding sessions, Studio
//!   sessions, turn summaries, and meeting segmentation, which keeps the name of the step it is
//!   becoming (`segment_meeting`) so its setting carries over when it moves.
//!
//! The [`catalog`] lists both, steps from every declared workflow (`crate::workflows::declared`)
//! including those that run no model, so the settings page shows them honestly rather than
//! offering a setting that changes nothing.
//!
//! **Resolving.** [`for_step`] and [`for_agent`] answer what a turn runs on: an admin's setting
//! (`crate::models::model_setting`) when there is one, otherwise the agent's default. Callers
//! resolve as each turn starts, so a change applies from the next turn, and pass both to the
//! runner (`TurnRequest::model` and `effort`), whose credential failover stack and usage-aware
//! skipping every turn runs through.
//!
//! **Layering.** Overrides per agent kind and per project (#146) layer over this: they widen
//! the resolvers with the scope a turn runs in (its project) and consult rows keyed by that
//! scope and the same agent name before falling back to what is resolved here. Nothing that
//! submits a turn changes for it.

use std::collections::HashSet;

use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use crate::models::model_setting::{self, Effort, ModelChoice};
use crate::workflows;

/// The models the settings page offers, strongest first: the current Claude models the
/// official CLI takes by id. Any other id or alias the CLI accepts, such as `opus[1m]`, may be
/// typed; these are suggestions, not a limit, since Anthropic releases models more often than
/// Elysium is told of them.
pub const SUGGESTED_MODELS: [&str; 4] = [
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
];

/// The most characters a model holds, as `model_settings.model` allows.
const MODEL_MAX_CHARACTERS: usize = 100;

/// The agents whose turns run outside the workflow step framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Agent {
    /// Elysia's own turns: messages, answers, events, and her idle work (`crate::elysia`).
    Elysia,
    /// A coding session's turns (`crate::sessions`).
    CodingSession,
    /// A Studio item's session's turns, which make single assets (`crate::studio`).
    StudioSession,
    /// The one or two sentence summary of each finished session turn
    /// (`crate::sessions::summary`).
    TurnSummary,
    /// Cutting a meeting's transcript into segments attributed to projects, which runs in the
    /// meetings loop until it becomes the meeting workflow's step of the same name.
    SegmentMeeting,
}

impl Agent {
    /// Every agent, in the order the settings page lists them.
    pub const ALL: [Self; 5] = [
        Self::Elysia,
        Self::CodingSession,
        Self::StudioSession,
        Self::TurnSummary,
        Self::SegmentMeeting,
    ];

    /// The agent's name in code, which its setting is filed under.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Elysia => "elysia",
            Self::CodingSession => "coding_session",
            Self::StudioSession => "studio_session",
            Self::TurnSummary => "turn_summary",
            Self::SegmentMeeting => "segment_meeting",
        }
    }

    /// What it runs on until an admin chooses otherwise. Opus 5.5 at high effort, the default
    /// for every agent, except turn summaries: a summary is one or two sentences about a turn
    /// already done, written for every turn, so the smallest model at low effort is plenty
    /// (`docs/coding.md`, Turn summaries).
    pub fn default_choice(self) -> ModelChoice {
        match self {
            Self::TurnSummary => ModelChoice {
                model: "claude-haiku-5-5".to_owned(),
                effort: Effort::Low,
            },
            Self::Elysia | Self::CodingSession | Self::StudioSession | Self::SegmentMeeting => {
                ModelChoice::default()
            }
        }
    }
}

/// Whether an entry of the catalog is a workflow step or an agent of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Step,
    Agent,
}

/// One thing the settings page lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name in code: a step's name, or an [`Agent`]'s.
    pub name: &'static str,
    pub kind: EntryKind,
    /// What it runs on until an admin chooses otherwise; `None` for a step that runs no model,
    /// which has nothing to set.
    pub default: Option<ModelChoice>,
}

/// Every step of every declared workflow, each once, first to last, then every [`Agent`].
///
/// # Panics
/// Panics when a step and an agent share a name, which only a programming error can cause:
/// they would share a setting.
pub fn catalog() -> Vec<Entry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for workflow in workflows::declared() {
        for step in workflow.steps() {
            if !seen.insert(step.name()) {
                // A step two workflows share is listed once.
                continue;
            }
            entries.push(Entry {
                name: step.name(),
                kind: EntryKind::Step,
                default: step.runs_model().then(ModelChoice::default),
            });
        }
    }
    for agent in Agent::ALL {
        assert!(
            seen.insert(agent.name()),
            "agent {} shares its name with a step",
            agent.name()
        );
        entries.push(Entry {
            name: agent.name(),
            kind: EntryKind::Agent,
            default: Some(agent.default_choice()),
        });
    }
    entries
}

/// What step `step`'s turns run on: its setting, or Opus 5.5 at high effort.
///
/// # Errors
/// Propagates any database error.
pub async fn for_step(connection: &mut AsyncPgConnection, step: &str) -> QueryResult<ModelChoice> {
    let stored = model_setting::find(connection, step).await?;
    Ok(stored.map(|stored| stored.choice).unwrap_or_default())
}

/// What `agent`'s turns run on: its setting, or its default.
///
/// # Errors
/// Propagates any database error.
pub async fn for_agent(
    connection: &mut AsyncPgConnection,
    agent: Agent,
) -> QueryResult<ModelChoice> {
    let stored = model_setting::find(connection, agent.name()).await?;
    Ok(stored.map_or_else(|| agent.default_choice(), |stored| stored.choice))
}

/// Why a setting could not be changed.
#[derive(Debug, thiserror::Error)]
pub enum ChangeError {
    /// No step or agent of this build has the name.
    #[error("no step or agent is named {0}")]
    Unknown(String),
    /// The step runs no model, so there is nothing to set.
    #[error("step {0} runs no model")]
    RunsNoModel(String),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Sets what `name` runs on, as `admin` chose. A choice that is the agent's default removes its
/// setting instead, so it follows the default from then on, should a later build change it.
/// Answers whether anything changed.
///
/// # Errors
/// [`ChangeError::Unknown`] or [`ChangeError::RunsNoModel`] when there is nothing by that name
/// to set; any database error.
pub async fn change(
    connection: &mut AsyncPgConnection,
    name: &str,
    choice: &ModelChoice,
    admin: Uuid,
) -> Result<bool, ChangeError> {
    let default = settable_default(name)?;
    if *choice == default {
        return Ok(model_setting::clear(connection, name, admin).await?);
    }
    Ok(model_setting::set(connection, name, choice, admin).await?)
}

/// Puts `name` back on its default, as `admin` asked. Answers whether it had a setting.
///
/// # Errors
/// As [`change`].
pub async fn reset(
    connection: &mut AsyncPgConnection,
    name: &str,
    admin: Uuid,
) -> Result<bool, ChangeError> {
    settable_default(name)?;
    Ok(model_setting::clear(connection, name, admin).await?)
}

/// The default of the entry named `name`, which must run a model.
fn settable_default(name: &str) -> Result<ModelChoice, ChangeError> {
    let entry = catalog()
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| ChangeError::Unknown(name.to_owned()))?;
    entry
        .default
        .ok_or_else(|| ChangeError::RunsNoModel(name.to_owned()))
}

/// Whether `model` is something the CLI's `--model` may be given: a letter or digit, then up to
/// 99 letters, digits, dots, underscores, hyphens, and brackets (`opus[1m]`). It never starts
/// with a hyphen, so it can never read as another flag. The rule `model_settings.model` checks.
pub fn is_model_name(model: &str) -> bool {
    let mut characters = model.chars();
    let starts_plainly = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric());
    let rest_is_plain = characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '[' | ']')
    });
    starts_plainly && rest_is_plain && model.len() <= MODEL_MAX_CHARACTERS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_named_as_a_setting_can_be_filed_and_listed_once() {
        let entries = catalog();
        let mut names = HashSet::new();
        for entry in &entries {
            assert!(
                entry
                    .name
                    .starts_with(|first: char| first.is_ascii_lowercase())
                    && entry.name.len() <= 64
                    && entry
                        .name
                        .chars()
                        .all(|character| character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || character == '_'),
                "{} is snake_case, as model_settings.agent checks",
                entry.name
            );
            assert!(names.insert(entry.name), "{} is listed once", entry.name);
        }
    }

    #[test]
    fn the_catalog_lists_the_meeting_steps_honestly_and_every_agent() {
        let entries = catalog();
        let find = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("{name} is listed"))
        };

        // Fetching and the local Whisper run no model of Claude's.
        for step in ["fetch_meeting", "transcribe_meeting"] {
            assert_eq!(find(step).kind, EntryKind::Step);
            assert_eq!(find(step).default, None, "{step} has nothing to set");
        }
        // Correcting a transcript and the company doc's review are agent turns.
        for step in ["correct_transcript", "review_company_doc"] {
            assert_eq!(find(step).default, Some(ModelChoice::default()), "{step}");
        }
        for agent in Agent::ALL {
            let entry = find(agent.name());
            assert_eq!(entry.kind, EntryKind::Agent);
            assert_eq!(entry.default, Some(agent.default_choice()));
        }
    }

    #[test]
    fn every_agent_runs_on_opus_at_high_effort_but_summaries_on_haiku() {
        for agent in Agent::ALL {
            let expected = if agent == Agent::TurnSummary {
                ModelChoice {
                    model: "claude-haiku-5-5".to_owned(),
                    effort: Effort::Low,
                }
            } else {
                ModelChoice {
                    model: "claude-opus-5-5".to_owned(),
                    effort: Effort::High,
                }
            };
            assert_eq!(agent.default_choice(), expected, "{agent:?}");
        }
    }

    #[test]
    fn only_a_listed_agent_that_runs_a_model_can_be_set() {
        assert!(matches!(
            settable_default("correct_spelling"),
            Err(ChangeError::Unknown(_))
        ));
        assert!(matches!(
            settable_default("fetch_meeting"),
            Err(ChangeError::RunsNoModel(_))
        ));
        assert_eq!(
            settable_default("elysia").expect("Elysia runs a model"),
            ModelChoice::default()
        );
    }

    #[test]
    fn a_model_is_an_id_or_alias_and_never_a_flag() {
        for model in [
            "claude-opus-5-5",
            "claude-haiku-5-5",
            "opus",
            "opus[1m]",
            "claude-sonnet-4.6",
            &"m".repeat(100),
        ] {
            assert!(is_model_name(model), "{model} is a model");
        }
        for model in [
            "",
            "--dangerously-skip-permissions",
            "-x",
            "opus 4",
            "opus;rm",
            "über",
            &"m".repeat(101),
        ] {
            assert!(!is_model_name(model), "{model} is refused");
        }
    }

    #[test]
    fn every_suggested_model_is_one_the_cli_may_be_given() {
        for model in SUGGESTED_MODELS {
            assert!(is_model_name(model), "{model}");
        }
        assert!(SUGGESTED_MODELS.contains(&model_setting::DEFAULT_MODEL));
    }
}
