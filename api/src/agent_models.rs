// Copyright © 2026 Jalapeno Labs

//! The model and effort every agent runs on: the kinds of agent, what there is to set, each
//! one's default, and the one place a turn's model is resolved.
//!
//! An agent is anything whose work is a turn on the runner, named as it is in code:
//!
//! - **a workflow step** whose work is an agent turn, which says which kind of agent it is
//!   ([`crate::workflows::Step::AGENT_KIND`]), named by its step name, such as
//!   `correct_transcript`;
//! - **an agent outside the step framework** ([`Agent`]): Elysia, coding sessions, Studio
//!   sessions, and turn summaries.
//!
//! Every agent belongs to one [`AgentKind`]: the workers, the Project Managers, correction,
//! segmenting, triage, Elysia, and the summaries. A kind is what an admin tunes once for all of
//! its agents, and what a project overrides for its own.
//!
//! **Resolving.** [`for_step`] and [`for_agent`] answer what a turn runs on, given the project
//! it runs for, if any. The first of these that is set wins ([`resolve`]):
//!
//! 1. the project's override of the agent's kind;
//! 2. the agent's own setting (`crate::models::model_setting`);
//! 3. the kind's setting for the workspace (`crate::models::kind_model_setting`);
//! 4. the kind's default ([`AgentKind::default_choice`]).
//!
//! The narrower choice wins at every level. A project is narrower than the workspace, so its
//! override beats everything an admin set for the workspace. Within the workspace one agent is
//! narrower than its kind, so an agent's own setting stays the exception it was set to be: an
//! admin who later sets the kind does not silently undo it, and the page never has to explain a
//! setting that no longer does anything.
//!
//! Callers resolve as each turn starts, so a change applies from the next turn, and pass the
//! result to the runner (`TurnRequest::model` and `effort`), whose credential failover stack and
//! usage-aware skipping every turn runs through.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::kind_model_setting::{self, Scope, StoredKindSetting};
use crate::models::model_setting::{self, Effort, ModelChoice, StoredSetting};
use crate::models::session_event::{self, RecordedUsage};
use crate::workflows;

/// The models the settings pages offer, strongest first: the current Claude models the
/// official CLI takes by id. Any other id or alias the CLI accepts, such as `opus[1m]`, may be
/// typed; these are suggestions, not a limit, since Anthropic releases models more often than
/// Elysium is told of them.
pub const SUGGESTED_MODELS: [&str; 4] = [
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
];

/// What a suggested model costs on Anthropic's API, in US cents per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    pub model: &'static str,
    pub input_cents_per_million_tokens: u32,
    pub output_cents_per_million_tokens: u32,
}

/// Anthropic's published first-party prices for [`SUGGESTED_MODELS`], as of October 2026, so
/// the settings pages can say what one model costs beside another. A subscription is billed by
/// its plan rather than per token, but its usage windows fill in proportion to the same tokens,
/// so the ratio holds either way. Effort changes how many tokens a turn spends, not their price.
/// A model typed by hand has no price here, and the pages say it is not known.
pub const MODEL_PRICES: [ModelPrice; 4] = [
    ModelPrice {
        model: "claude-fable-5-1",
        input_cents_per_million_tokens: 1_000,
        output_cents_per_million_tokens: 5_000,
    },
    ModelPrice {
        model: "claude-opus-5-5",
        input_cents_per_million_tokens: 400,
        output_cents_per_million_tokens: 2_000,
    },
    ModelPrice {
        model: "claude-sonnet-5-5",
        input_cents_per_million_tokens: 200,
        output_cents_per_million_tokens: 1_000,
    },
    ModelPrice {
        model: "claude-haiku-5-5",
        input_cents_per_million_tokens: 10,
        output_cents_per_million_tokens: 50,
    },
];

/// How far back the settings pages total what each kind's turns spent: long enough to cover a
/// month of work, short enough that a change of model shows in the numbers soon after.
pub const USAGE_WINDOW: Duration = Duration::days(30);

/// The most characters a model holds, as `model_settings.model` allows.
const MODEL_MAX_CHARACTERS: usize = 100;

/// The kinds of agent Elysium runs. Every step and agent whose work is a turn belongs to one,
/// and each kind's setting applies to every one of its agents that has none of its own.
///
/// Stored as `agent_kind` in Postgres. A new kind is added here, to the migration's enum, and
/// to the web app's words; the work that runs as it names it with
/// [`crate::workflows::Step::AGENT_KIND`] or [`Agent::kind`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::AgentKind"]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Agents that do a project's work: today coding sessions and Studio sessions, in the
    /// shared workspace; the engine's pooled workers (#82) join them.
    Worker,
    /// Each project's Project Manager, which plans its work and files its issues (#84, #85).
    ProjectManager,
    /// Correcting a meeting's transcripts against the company doc and other meetings.
    Correction,
    /// Cutting a meeting's transcript or a conversation into topics attributed to projects.
    Segmenting,
    /// Deciding what a message or conversation asks for and who it is for.
    Triage,
    /// Elysia's own turns, and the work she does in her name, such as reviewing the company doc.
    Elysia,
    /// The one or two sentence summaries of finished turns.
    Summaries,
}

impl AgentKind {
    /// Every kind, in the order the settings pages list them.
    pub const ALL: [Self; 7] = [
        Self::Worker,
        Self::ProjectManager,
        Self::Correction,
        Self::Segmenting,
        Self::Triage,
        Self::Elysia,
        Self::Summaries,
    ];

    /// The kind's name in code, as the database and clients name it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::ProjectManager => "project_manager",
            Self::Correction => "correction",
            Self::Segmenting => "segmenting",
            Self::Triage => "triage",
            Self::Elysia => "elysia",
            Self::Summaries => "summaries",
        }
    }

    /// The kind named `name`, if this build has one.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// What its agents run on until someone chooses otherwise. Opus 5.5 at high effort, since an
    /// agent's work (correcting a transcript, planning a project, a coding session) is read by
    /// people and acted on, except summaries: one or two sentences about a turn already done,
    /// written for every turn, for which the smallest model at low effort is plenty
    /// (`docs/coding.md`, Turn summaries).
    pub fn default_choice(self) -> ModelChoice {
        match self {
            Self::Summaries => ModelChoice {
                model: "claude-haiku-5-5".to_owned(),
                effort: Effort::Low,
            },
            Self::Worker
            | Self::ProjectManager
            | Self::Correction
            | Self::Segmenting
            | Self::Triage
            | Self::Elysia => ModelChoice::default(),
        }
    }

    /// Whether its turns run for one project, so a project may override it for its own agents.
    /// Correction, segmenting, and triage read meetings and conversations that may concern any
    /// number of projects, and Elysia works for the whole workspace. The check
    /// `kind_model_settings_project_kind_check` holds the same list.
    pub const fn runs_in_projects(self) -> bool {
        matches!(self, Self::Worker | Self::ProjectManager | Self::Summaries)
    }

    /// Whether its turns record what they cost, so the pages can total it. Only session turns
    /// keep their final event today (`session_events`); every other turn's cost is reported by
    /// the runner and not kept, until the Activity log records every turn (`docs/activity.md`).
    pub const fn records_usage(self) -> bool {
        matches!(self, Self::Worker)
    }
}

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
}

impl Agent {
    /// Every agent, in the order the settings page lists them.
    pub const ALL: [Self; 4] = [
        Self::Elysia,
        Self::CodingSession,
        Self::StudioSession,
        Self::TurnSummary,
    ];

    /// The agent's name in code, which its setting is filed under.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Elysia => "elysia",
            Self::CodingSession => "coding_session",
            Self::StudioSession => "studio_session",
            Self::TurnSummary => "turn_summary",
        }
    }

    /// The kind of agent it is.
    pub const fn kind(self) -> AgentKind {
        match self {
            Self::Elysia => AgentKind::Elysia,
            Self::CodingSession | Self::StudioSession => AgentKind::Worker,
            Self::TurnSummary => AgentKind::Summaries,
        }
    }
}

/// One thing the settings page lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Its name in code: a step's name, or an [`Agent`]'s.
    pub name: &'static str,
    /// Whether it is a workflow step rather than an agent of its own.
    pub is_step: bool,
    /// The kind of agent it is; `None` for a step that runs no model, which has nothing to set.
    pub kind: Option<AgentKind>,
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
                is_step: true,
                kind: step.agent_kind(),
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
            is_step: false,
            kind: Some(agent.kind()),
        });
    }
    entries
}

/// Which level of [`resolve`] a turn's model came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// The project's override of the agent's kind.
    Project,
    /// The agent's own setting.
    Agent,
    /// The kind's setting for the workspace.
    Kind,
    /// The kind's default.
    Default,
}

/// What someone chose at each level for one agent's turn; `None` where nothing was chosen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub project: Option<ModelChoice>,
    pub agent: Option<ModelChoice>,
    pub kind: Option<ModelChoice>,
}

/// What a turn runs on, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub choice: ModelChoice,
    pub source: Source,
}

/// The rule every turn's model follows: the first level that is set, narrowest first (see the
/// module's docs for why), or `kind`'s default.
pub fn resolve(kind: AgentKind, settings: Settings) -> Resolved {
    let levels = [
        (settings.project, Source::Project),
        (settings.agent, Source::Agent),
        (settings.kind, Source::Kind),
    ];
    levels
        .into_iter()
        .find_map(|(choice, source)| choice.map(|choice| Resolved { choice, source }))
        .unwrap_or_else(|| Resolved {
            choice: kind.default_choice(),
            source: Source::Default,
        })
}

/// What step `step`'s turns run on, as a step of kind `kind` working for `project`.
///
/// # Errors
/// Propagates any database error.
pub async fn for_step(
    connection: &mut AsyncPgConnection,
    step: &str,
    kind: AgentKind,
    project: Option<Uuid>,
) -> QueryResult<Resolved> {
    let settings = stored_settings(connection, step, kind, project).await?;
    Ok(resolve(kind, settings))
}

/// What `agent`'s turns run on, working for `project`: a session's project, or `None` for
/// work that is the whole workspace's.
///
/// # Errors
/// Propagates any database error.
pub async fn for_agent(
    connection: &mut AsyncPgConnection,
    agent: Agent,
    project: Option<Uuid>,
) -> QueryResult<Resolved> {
    let settings = stored_settings(connection, agent.name(), agent.kind(), project).await?;
    Ok(resolve(agent.kind(), settings))
}

/// What is stored at each level for the agent named `name`, of kind `kind`, working for
/// `project`. A project's row for a kind that runs in no project cannot exist (the table's
/// check refuses it), so it is not looked for.
async fn stored_settings(
    connection: &mut AsyncPgConnection,
    name: &str,
    kind: AgentKind,
    project: Option<Uuid>,
) -> QueryResult<Settings> {
    let project = match project.filter(|_project| kind.runs_in_projects()) {
        Some(project) => kind_model_setting::find(connection, Scope::Project(project), kind)
            .await?
            .map(|stored| stored.choice),
        None => None,
    };
    let agent = model_setting::find(connection, name)
        .await?
        .map(|stored| stored.choice);
    let kind = kind_model_setting::find(connection, Scope::Workspace, kind)
        .await?
        .map(|stored| stored.choice);
    Ok(Settings {
        project,
        agent,
        kind,
    })
}

/// One step or agent as the workspace's settings page shows it.
#[derive(Debug, Clone)]
pub struct EntryView {
    pub entry: Entry,
    /// What an admin chose for it alone.
    pub setting: Option<StoredSetting>,
    /// What its turns run on outside any project, and why; `None` for a step that runs no model.
    pub resolved: Option<Resolved>,
}

/// One kind as the workspace's settings page shows it.
#[derive(Debug, Clone)]
pub struct KindView {
    pub kind: AgentKind,
    /// What an admin chose for the kind.
    pub setting: Option<StoredKindSetting>,
    /// What its turns spent over [`USAGE_WINDOW`], or `None` where its turns record no cost.
    pub usage: Option<RecordedUsage>,
}

/// Every step, agent, and kind with what was chosen and what it resolves to in the workspace,
/// and what each kind's turns spent since `since`, for the admins' settings page.
///
/// # Errors
/// Propagates any database error.
pub async fn workspace_view(
    connection: &mut AsyncPgConnection,
    since: DateTime<Utc>,
) -> QueryResult<(Vec<EntryView>, Vec<KindView>)> {
    let mut agent_settings: HashMap<String, StoredSetting> = model_setting::list(connection)
        .await?
        .into_iter()
        .map(|stored| (stored.agent.clone(), stored))
        .collect();
    let mut kind_settings: HashMap<AgentKind, StoredKindSetting> =
        kind_model_setting::list(connection, Scope::Workspace)
            .await?
            .into_iter()
            .map(|stored| (stored.kind, stored))
            .collect();

    let mut entries = Vec::new();
    for entry in catalog() {
        let Some(kind) = entry.kind else {
            // A step that runs no model has nothing to set, whatever a row says.
            entries.push(EntryView {
                entry,
                setting: None,
                resolved: None,
            });
            continue;
        };
        let setting = agent_settings.remove(entry.name);
        let settings = Settings {
            project: None,
            agent: setting.as_ref().map(|stored| stored.choice.clone()),
            kind: kind_settings.get(&kind).map(|stored| stored.choice.clone()),
        };
        entries.push(EntryView {
            entry,
            setting,
            resolved: Some(resolve(kind, settings)),
        });
    }

    let mut kinds = Vec::new();
    for kind in AgentKind::ALL {
        kinds.push(KindView {
            kind,
            setting: kind_settings.remove(&kind),
            usage: usage(connection, kind, since, None).await?,
        });
    }
    Ok((entries, kinds))
}

/// The step or agent named `name` as the workspace's settings page shows it, or `None` when
/// the catalog has no entry by that name.
///
/// # Errors
/// Propagates any database error.
pub async fn entry_view(
    connection: &mut AsyncPgConnection,
    name: &str,
) -> QueryResult<Option<EntryView>> {
    let Some(entry) = catalog().into_iter().find(|entry| entry.name == name) else {
        return Ok(None);
    };
    let Some(kind) = entry.kind else {
        return Ok(Some(EntryView {
            entry,
            setting: None,
            resolved: None,
        }));
    };
    let setting = model_setting::find(connection, name).await?;
    let settings = Settings {
        project: None,
        agent: setting.as_ref().map(|stored| stored.choice.clone()),
        kind: kind_model_setting::find(connection, Scope::Workspace, kind)
            .await?
            .map(|stored| stored.choice),
    };
    Ok(Some(EntryView {
        entry,
        setting,
        resolved: Some(resolve(kind, settings)),
    }))
}

/// Kind `kind` as the workspace's settings page shows it, with what its turns spent since
/// `since`.
///
/// # Errors
/// Propagates any database error.
pub async fn kind_view(
    connection: &mut AsyncPgConnection,
    kind: AgentKind,
    since: DateTime<Utc>,
) -> QueryResult<KindView> {
    Ok(KindView {
        kind,
        setting: kind_model_setting::find(connection, Scope::Workspace, kind).await?,
        usage: usage(connection, kind, since, None).await?,
    })
}

/// One kind as a project's model settings show it.
#[derive(Debug, Clone)]
pub struct ProjectKindView {
    pub kind: AgentKind,
    /// The project's own choice for the kind.
    pub setting: Option<StoredKindSetting>,
    /// What the kind follows without it: the workspace's setting of the kind, or its default.
    pub workspace: Resolved,
    /// What the kind's turns for this project spent since `since`, where its turns record it.
    pub usage: Option<RecordedUsage>,
}

/// Every kind that runs in projects, as [`project_kind_view`] shows each for `project`.
///
/// # Errors
/// Propagates any database error.
pub async fn project_view(
    connection: &mut AsyncPgConnection,
    project: Uuid,
    since: DateTime<Utc>,
) -> QueryResult<Vec<ProjectKindView>> {
    let mut kinds = Vec::new();
    for kind in AgentKind::ALL {
        if !kind.runs_in_projects() {
            continue;
        }
        kinds.push(project_kind_view(connection, project, kind, since).await?);
    }
    Ok(kinds)
}

/// Kind `kind` as project `project` has it set, with what it follows without the project's
/// choice and what its turns for the project spent since `since`.
///
/// An agent of the kind with a setting of its own runs on that instead of what the kind
/// follows here; the project's own choice overrides both.
///
/// # Errors
/// Propagates any database error.
pub async fn project_kind_view(
    connection: &mut AsyncPgConnection,
    project: Uuid,
    kind: AgentKind,
    since: DateTime<Utc>,
) -> QueryResult<ProjectKindView> {
    Ok(ProjectKindView {
        kind,
        setting: kind_model_setting::find(connection, Scope::Project(project), kind).await?,
        workspace: workspace_kind_choice(connection, kind).await?,
        usage: usage(connection, kind, since, Some(project)).await?,
    })
}

/// What kind `kind`'s turns spent since `since`, every project's or `project`'s, or `None`
/// where its turns record no cost ([`AgentKind::records_usage`]).
async fn usage(
    connection: &mut AsyncPgConnection,
    kind: AgentKind,
    since: DateTime<Utc>,
    project: Option<Uuid>,
) -> QueryResult<Option<RecordedUsage>> {
    if !kind.records_usage() {
        return Ok(None);
    }
    // Workers are coding and Studio sessions today, whose turns' final events are kept.
    Ok(Some(
        session_event::usage_since(connection, since, project).await?,
    ))
}

/// What kind `kind` runs on in the workspace without any agent's own setting: the kind's
/// setting, or its default. A project's override is measured against it.
async fn workspace_kind_choice(
    connection: &mut AsyncPgConnection,
    kind: AgentKind,
) -> QueryResult<Resolved> {
    let settings = Settings {
        kind: kind_model_setting::find(connection, Scope::Workspace, kind)
            .await?
            .map(|stored| stored.choice),
        ..Settings::default()
    };
    Ok(resolve(kind, settings))
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
    /// A project may override only the kinds whose turns run for one project.
    #[error("{} runs in no project", .0.name())]
    NotInProjects(AgentKind),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Sets what `name` runs on, as `admin` chose. A choice equal to what it follows without one
/// (its kind's setting, or the kind's default) removes its setting instead, so it keeps
/// following its kind, should that change. Answers whether anything changed.
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
    let kind = settable_kind(name)?;
    let followed = workspace_kind_choice(connection, kind).await?.choice;
    if *choice == followed {
        return Ok(model_setting::clear(connection, name, admin).await?);
    }
    Ok(model_setting::set(connection, name, choice, admin).await?)
}

/// Puts `name` back on what its kind resolves to, as `admin` asked. Answers whether it had a
/// setting.
///
/// # Errors
/// As [`change`].
pub async fn reset(
    connection: &mut AsyncPgConnection,
    name: &str,
    admin: Uuid,
) -> Result<bool, ChangeError> {
    settable_kind(name)?;
    Ok(model_setting::clear(connection, name, admin).await?)
}

/// Sets what kind `kind` runs on in `scope`, as `actor` chose. A choice equal to what the kind
/// follows there without one (the workspace's setting of the kind or its default, for a project;
/// its default, for the workspace) removes the row instead, so the kind keeps following it,
/// should that change. Answers whether anything changed.
///
/// # Errors
/// [`ChangeError::NotInProjects`] for a project's override of a kind that runs in no project;
/// any database error.
pub async fn change_kind(
    connection: &mut AsyncPgConnection,
    scope: Scope,
    kind: AgentKind,
    choice: &ModelChoice,
    actor: Uuid,
) -> Result<bool, ChangeError> {
    let followed = match scope {
        Scope::Workspace => kind.default_choice(),
        Scope::Project(_project) => {
            settable_in(scope, kind)?;
            workspace_kind_choice(connection, kind).await?.choice
        }
    };
    if *choice == followed {
        return Ok(kind_model_setting::clear(connection, scope, kind, actor).await?);
    }
    Ok(kind_model_setting::set(connection, scope, kind, choice, actor).await?)
}

/// Removes kind `kind`'s setting in `scope`, as `actor` asked. Answers whether it had one.
///
/// # Errors
/// As [`change_kind`].
pub async fn reset_kind(
    connection: &mut AsyncPgConnection,
    scope: Scope,
    kind: AgentKind,
    actor: Uuid,
) -> Result<bool, ChangeError> {
    settable_in(scope, kind)?;
    Ok(kind_model_setting::clear(connection, scope, kind, actor).await?)
}

/// The kind of the entry named `name`, which must run a model.
fn settable_kind(name: &str) -> Result<AgentKind, ChangeError> {
    let entry = catalog()
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| ChangeError::Unknown(name.to_owned()))?;
    entry
        .kind
        .ok_or_else(|| ChangeError::RunsNoModel(name.to_owned()))
}

/// Checks that `kind` may be set in `scope`: anywhere for the workspace, only a kind whose
/// turns run for one project in a project.
fn settable_in(scope: Scope, kind: AgentKind) -> Result<(), ChangeError> {
    if matches!(scope, Scope::Project(_project)) && !kind.runs_in_projects() {
        return Err(ChangeError::NotInProjects(kind));
    }
    Ok(())
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

    fn choice(model: &str, effort: Effort) -> ModelChoice {
        ModelChoice {
            model: model.to_owned(),
            effort,
        }
    }

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
    fn the_catalog_lists_the_meeting_steps_honestly_and_every_agent_with_its_kind() {
        let entries = catalog();
        let find = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("{name} is listed"))
        };

        // Fetching and the local Whisper run no model of Claude's.
        for step in ["fetch_meeting", "transcribe_meeting"] {
            assert!(find(step).is_step);
            assert_eq!(find(step).kind, None, "{step} has nothing to set");
        }
        // Correction and segmenting are kinds of their own; the company doc's review is Elysia's
        // work, done in her name.
        assert_eq!(find("correct_transcript").kind, Some(AgentKind::Correction));
        assert_eq!(find("segment_meeting").kind, Some(AgentKind::Segmenting));
        assert_eq!(find("review_company_doc").kind, Some(AgentKind::Elysia));
        for agent in Agent::ALL {
            let entry = find(agent.name());
            assert!(!entry.is_step);
            assert_eq!(entry.kind, Some(agent.kind()));
        }
    }

    #[test]
    fn todays_agents_belong_to_the_kinds_the_engine_names() {
        assert_eq!(Agent::CodingSession.kind(), AgentKind::Worker);
        assert_eq!(Agent::StudioSession.kind(), AgentKind::Worker);
        assert_eq!(Agent::TurnSummary.kind(), AgentKind::Summaries);
        assert_eq!(Agent::Elysia.kind(), AgentKind::Elysia);
    }

    #[test]
    fn every_kind_runs_on_opus_at_high_effort_but_summaries_on_haiku() {
        for kind in AgentKind::ALL {
            let expected = if kind == AgentKind::Summaries {
                choice("claude-haiku-5-5", Effort::Low)
            } else {
                choice("claude-opus-5-5", Effort::High)
            };
            assert_eq!(kind.default_choice(), expected, "{kind:?}");
        }
    }

    #[test]
    fn every_kind_reads_back_from_its_name_and_serializes_as_it() {
        for kind in AgentKind::ALL {
            assert_eq!(AgentKind::from_name(kind.name()), Some(kind));
            assert_eq!(
                serde_json::to_value(kind).expect("serializes"),
                kind.name(),
                "clients see the name the database stores"
            );
        }
        assert_eq!(AgentKind::from_name("intern"), None);
    }

    #[test]
    fn only_the_kinds_that_work_for_one_project_may_be_overridden_by_one() {
        let in_projects: Vec<AgentKind> = AgentKind::ALL
            .into_iter()
            .filter(|kind| kind.runs_in_projects())
            .collect();
        assert_eq!(
            in_projects,
            [
                AgentKind::Worker,
                AgentKind::ProjectManager,
                AgentKind::Summaries
            ]
        );
        settable_in(Scope::Workspace, AgentKind::Elysia).expect("the workspace sets any kind");
        assert!(matches!(
            settable_in(Scope::Project(Uuid::now_v7()), AgentKind::Elysia),
            Err(ChangeError::NotInProjects(AgentKind::Elysia))
        ));
        settable_in(Scope::Project(Uuid::now_v7()), AgentKind::Worker)
            .expect("a project sets its workers");
    }

    #[test]
    fn the_narrowest_choice_wins_project_then_agent_then_kind_then_default() {
        let project = choice("claude-haiku-5-5", Effort::Max);
        let agent = choice("claude-sonnet-5-5", Effort::Medium);
        let kind = choice("claude-fable-5-1", Effort::Xhigh);
        let every_level = Settings {
            project: Some(project.clone()),
            agent: Some(agent.clone()),
            kind: Some(kind.clone()),
        };

        assert_eq!(
            resolve(AgentKind::Worker, every_level.clone()),
            Resolved {
                choice: project,
                source: Source::Project
            },
            "a project's override beats everything the workspace set"
        );
        assert_eq!(
            resolve(
                AgentKind::Worker,
                Settings {
                    project: None,
                    ..every_level.clone()
                }
            ),
            Resolved {
                choice: agent,
                source: Source::Agent
            },
            "an agent's own setting beats its kind's"
        );
        assert_eq!(
            resolve(
                AgentKind::Worker,
                Settings {
                    kind: Some(kind.clone()),
                    ..Settings::default()
                }
            ),
            Resolved {
                choice: kind,
                source: Source::Kind
            },
            "the kind applies to every agent without a setting of its own"
        );
        assert_eq!(
            resolve(AgentKind::Summaries, Settings::default()),
            Resolved {
                choice: AgentKind::Summaries.default_choice(),
                source: Source::Default
            },
            "nothing set is the kind's default"
        );
    }

    #[test]
    fn only_a_listed_agent_that_runs_a_model_can_be_set() {
        assert!(matches!(
            settable_kind("correct_spelling"),
            Err(ChangeError::Unknown(_))
        ));
        assert!(matches!(
            settable_kind("fetch_meeting"),
            Err(ChangeError::RunsNoModel(_))
        ));
        assert_eq!(
            settable_kind("elysia").expect("Elysia runs a model"),
            AgentKind::Elysia
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
    fn every_suggested_model_is_one_the_cli_may_be_given_and_has_a_price() {
        for model in SUGGESTED_MODELS {
            assert!(is_model_name(model), "{model}");
            assert!(
                MODEL_PRICES.iter().any(|price| price.model == model),
                "{model} has a price"
            );
        }
        assert!(SUGGESTED_MODELS.contains(&model_setting::DEFAULT_MODEL));
        for kind in AgentKind::ALL {
            assert!(
                SUGGESTED_MODELS.contains(&kind.default_choice().model.as_str()),
                "{kind:?}'s default is offered"
            );
        }
    }
}
