// Copyright © 2026 Jalapeno Labs

//! Findings: work a worker or a Project Manager noticed that nobody asked for, waiting on its
//! project's Findings list for a person to accept or dismiss. See `docs/engine.md`, Findings.
//!
//! # The same finding
//!
//! A finding is what was seen and where: its title, the repository it was seen in, and the place
//! in it ([`fingerprint`]). Each part is normalized first, so letter case, spacing, punctuation in
//! a title, a repository's URL spelled with or without `.git`, and a line number in a path never
//! make two findings of one. A project holds one row per fingerprint (`findings_once`), so a
//! second report of the same finding, from any worker or Project Manager, after any restart, and
//! two at once, lands on the first ([`report`]): it counts the report and answers how the finding
//! stands. Rows are never deleted, so a dismissed finding stays dismissed however often it is
//! seen again. Two wordings of one problem are two findings; a person dismisses the second.
//!
//! # Deciding
//!
//! Only a person decides, and only once: [`lock_open`] refuses a finding no longer open, and
//! accepting and dismissing each run under its lock (`crate::findings`).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::database::schema::findings;
use crate::models::user::{CODING_AGENT_USER_ID, PROJECT_MANAGER_USER_ID};

/// The longest title, matching `findings.title`, and a batch's, which accepting it makes.
pub const TITLE_MAX_CHARACTERS: usize = 200;

/// The longest detail, matching `findings.detail`: half a batch's goal, which accepting builds
/// from it, so where it was seen always fits beside it.
pub const DETAIL_MAX_CHARACTERS: usize = 10_000;

/// The longest place, matching `findings.location`.
pub const LOCATION_MAX_CHARACTERS: usize = 500;

/// The longest reason for dismissing, matching `findings.dismissal_reason`.
pub const DISMISSAL_REASON_MAX_CHARACTERS: usize = 2_000;

/// The most open findings a project's list holds. More than this waiting is a list nobody reads;
/// its count still says how many.
pub const MAX_OPEN_LISTED: i64 = 200;

/// Where a finding stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::FindingState"]
#[serde(rename_all = "snake_case")]
pub enum FindingState {
    /// On its project's Findings list, for a person.
    Open,
    /// A person accepted it: it is an action item and a proposed batch.
    Accepted,
    /// A person dismissed it; it is never raised again.
    Dismissed,
}

/// What a finding would cost to act on, as its reporter estimates it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::FindingCost"]
#[serde(rename_all = "snake_case")]
pub enum Cost {
    /// An hour of a worker's time or less.
    Small,
    /// About one batch's work.
    Medium,
    /// More than one batch.
    Large,
}

impl Cost {
    /// The cost in a sentence for whoever reads a finding's issue or batch.
    pub const fn words(self) -> &'static str {
        match self {
            Self::Small => "small, an hour of a worker's time or less",
            Self::Medium => "medium, about one batch's work",
            Self::Large => "large, more than one batch",
        }
    }
}

/// A stored finding.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = findings, check_for_backend(diesel::pg::Pg))]
pub struct Finding {
    pub id: Uuid,
    pub project_id: Uuid,
    pub fingerprint: String,
    pub title: String,
    pub detail: String,
    pub repository_url: Option<String>,
    pub location: String,
    pub cost: Cost,
    pub state: FindingState,
    /// The batch whose worker reported it first, while that batch is kept.
    pub work_batch_id: Option<Uuid>,
    /// The Project Manager's turn that reported it first, while that turn is kept.
    pub project_manager_turn_id: Option<Uuid>,
    /// How many times it was reported, the first included.
    pub reports: i32,
    pub last_reported_at: DateTime<Utc>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub dismissal_reason: Option<String>,
    /// The action item accepting it made.
    pub action_item_id: Option<Uuid>,
    /// The batch accepting it proposed.
    pub accepted_batch_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl Finding {
    /// Where it was seen, in a line: `src/billing.rs in https://github.com/org/repo`, or the
    /// place alone when no repository was named.
    pub fn place(&self) -> String {
        match &self.repository_url {
            Some(repository) => format!("{} in {repository}", self.location),
            None => self.location.clone(),
        }
    }
}

/// Who reported a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reporter {
    /// The worker running this batch.
    Worker { batch_id: Uuid },
    /// The Project Manager, in this turn.
    ProjectManager { turn_id: Uuid },
}

impl Reporter {
    /// The machine user a finding it reported is created by.
    pub const fn user_id(self) -> Uuid {
        match self {
            Self::Worker { .. } => CODING_AGENT_USER_ID,
            Self::ProjectManager { .. } => PROJECT_MANAGER_USER_ID,
        }
    }
}

/// A finding to report, already tidied and held to the rules (`crate::findings::check`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFinding {
    pub title: String,
    pub detail: String,
    pub repository_url: Option<String>,
    pub location: String,
    pub cost: Cost,
}

#[derive(Insertable)]
#[diesel(table_name = findings)]
struct FindingRow {
    id: Uuid,
    project_id: Uuid,
    fingerprint: String,
    title: String,
    detail: String,
    repository_url: Option<String>,
    location: String,
    cost: Cost,
    work_batch_id: Option<Uuid>,
    project_manager_turn_id: Option<Uuid>,
    last_reported_at: DateTime<Utc>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

/// What finding `new_finding` is, as 64 hex digits: the SHA-256 of its repository, place, and
/// title, each normalized, so two reports of one finding have one fingerprint however they spell
/// it. Hashed so the key fits a btree index whatever its parts hold.
pub fn fingerprint(new_finding: &NewFinding) -> String {
    let key = format!(
        "{}\n{}\n{}",
        new_finding
            .repository_url
            .as_deref()
            .map(normalized_repository)
            .unwrap_or_default(),
        normalized_location(&new_finding.location),
        normalized_title(&new_finding.title),
    );
    hex::encode(Sha256::digest(key))
}

/// A repository's URL in one spelling: lowercase, with no scheme, no `.git`, and no trailing
/// slash, an SSH remote (`git@github.com:org/repo`) read as its host and path.
fn normalized_repository(url: &str) -> String {
    let lowercase = url.trim().to_lowercase();
    let without_scheme = ["https://", "http://", "ssh://"]
        .iter()
        .find_map(|scheme| lowercase.strip_prefix(scheme))
        .unwrap_or(&lowercase);
    let host_and_path = without_scheme.strip_prefix("git@").map_or_else(
        || without_scheme.to_owned(),
        |remote| remote.replacen(':', "/", 1),
    );
    let trimmed = host_and_path.trim_end_matches('/');
    trimmed
        .strip_suffix(".git")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_owned()
}

/// A place in one spelling: lowercase, its whitespace collapsed, with no leading `./`, and, for a
/// path, with no line, column, or range at its end (`:12`, `:12:5`, `#L12`, `#L12-L20`), since
/// lines move as the code around them changes while the finding stays where it was. A place with
/// spaces in it is a place's name rather than a path or a URL, so its ending is kept whole: the
/// `:30` of `Standup 2026-11-07 10:30` is part of what it names.
fn normalized_location(location: &str) -> String {
    let collapsed = location
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut place = collapsed.strip_prefix("./").unwrap_or(&collapsed);
    if place.contains(' ') {
        return place.to_owned();
    }
    if let Some((path, anchor)) = place.rsplit_once("#l")
        && is_line_range(anchor)
    {
        place = path;
    }
    // A line, then a column.
    for _part in 0..2 {
        if let Some((path, number)) = place.rsplit_once(':')
            && !number.is_empty()
            && number.chars().all(|character| character.is_ascii_digit())
        {
            place = path;
        }
    }
    place.to_owned()
}

/// Whether `anchor`, after a `#L`, is a line or a range of lines: `12` or `12-l20`.
fn is_line_range(anchor: &str) -> bool {
    let is_number =
        |text: &str| !text.is_empty() && text.chars().all(|character| character.is_ascii_digit());
    match anchor.split_once("-l") {
        Some((first, last)) => is_number(first) && is_number(last),
        None => is_number(anchor),
    }
}

/// A title's words, lowercase, joined by single spaces, with its punctuation left out.
fn normalized_title(title: &str) -> String {
    title
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A report, as it landed.
#[derive(Debug, Clone)]
pub struct Reported {
    pub finding: Finding,
    /// Whether this report made the finding. When it did not, the finding was reported before,
    /// and its state says what a person made of it.
    pub is_new: bool,
}

/// Reports `new_finding` on `project_id`, by `reporter`, at `now`: stores it open, or, when the
/// project already holds the same finding ([`fingerprint`]), counts one more report of that one
/// and leaves everything else about it as it stands, whatever a person decided.
///
/// # Errors
/// Propagates any database error, including a field outside its column's checks.
pub async fn report(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    reporter: Reporter,
    new_finding: NewFinding,
    now: DateTime<Utc>,
) -> QueryResult<Reported> {
    let (work_batch_id, project_manager_turn_id) = match reporter {
        Reporter::Worker { batch_id } => (Some(batch_id), None),
        Reporter::ProjectManager { turn_id } => (None, Some(turn_id)),
    };
    let row = FindingRow {
        id: Uuid::now_v7(),
        project_id,
        fingerprint: fingerprint(&new_finding),
        title: new_finding.title,
        detail: new_finding.detail,
        repository_url: new_finding.repository_url,
        location: new_finding.location,
        cost: new_finding.cost,
        work_batch_id,
        project_manager_turn_id,
        last_reported_at: now,
        created_by: reporter.user_id(),
        created_at: now,
    };
    // One statement, so two reports at once both land on the one row: the second waits for the
    // first to commit, then counts itself on it.
    let finding = diesel::insert_into(findings::table)
        .values(row)
        .on_conflict((findings::project_id, findings::fingerprint))
        .do_update()
        .set((
            findings::reports.eq(findings::reports + 1),
            findings::last_reported_at.eq(now),
        ))
        .returning(Finding::as_returning())
        .get_result(connection)
        .await?;
    let is_new = finding.reports == 1;
    Ok(Reported { finding, is_new })
}

/// `project_id`'s open findings, newest first, at most [`MAX_OPEN_LISTED`].
///
/// # Errors
/// Propagates any database error.
pub async fn list_open(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Vec<Finding>> {
    findings::table
        .filter(findings::project_id.eq(project_id))
        .filter(findings::state.eq(FindingState::Open))
        .select(Finding::as_select())
        .order(findings::id.desc())
        .limit(MAX_OPEN_LISTED)
        .load(connection)
        .await
}

/// How many open findings `project_id` holds, however many [`list_open`] lists.
///
/// # Errors
/// Propagates any database error.
pub async fn count_open(connection: &mut AsyncPgConnection, project_id: Uuid) -> QueryResult<i64> {
    findings::table
        .filter(findings::project_id.eq(project_id))
        .filter(findings::state.eq(FindingState::Open))
        .count()
        .get_result(connection)
        .await
}

/// How many open findings each project holds, for every project holding any.
///
/// # Errors
/// Propagates any database error.
pub async fn open_counts(connection: &mut AsyncPgConnection) -> QueryResult<Vec<(Uuid, i64)>> {
    findings::table
        .filter(findings::state.eq(FindingState::Open))
        .group_by(findings::project_id)
        .select((findings::project_id, diesel::dsl::count_star()))
        .load(connection)
        .await
}

/// Why a person's decision on a finding was refused.
#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    #[error("no such finding")]
    NotFound,
    /// Someone already decided it.
    #[error("the finding was already {0:?}")]
    Decided(FindingState),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Finding `id`, locked until the transaction ends, while it is open.
///
/// # Errors
/// [`DecideError::NotFound`] for an unknown finding; [`DecideError::Decided`] once a person
/// decided it; any other database error.
pub async fn lock_open(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> Result<Finding, DecideError> {
    let finding = findings::table
        .find(id)
        .for_update()
        .select(Finding::as_select())
        .first(connection)
        .await
        .optional()?
        .ok_or(DecideError::NotFound)?;
    if finding.state != FindingState::Open {
        return Err(DecideError::Decided(finding.state));
    }
    Ok(finding)
}

/// Records that person `by` accepted finding `id` at `now`, making action item `action_item_id`
/// and batch `batch_id`. Call it under [`lock_open`], in the transaction that made both.
///
/// # Errors
/// Propagates any database error.
pub async fn record_accepted(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    by: Uuid,
    now: DateTime<Utc>,
    action_item_id: Uuid,
    batch_id: Uuid,
) -> QueryResult<Finding> {
    diesel::update(findings::table.find(id))
        .set((
            findings::state.eq(FindingState::Accepted),
            findings::decided_by.eq(by),
            findings::decided_at.eq(now),
            findings::action_item_id.eq(action_item_id),
            findings::accepted_batch_id.eq(batch_id),
        ))
        .returning(Finding::as_returning())
        .get_result(connection)
        .await
}

/// Records that person `by` dismissed finding `id` at `now`, saying `reason` if they said why.
/// Call it under [`lock_open`].
///
/// # Errors
/// Propagates any database error, including a reason longer than its column holds.
pub async fn record_dismissed(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    by: Uuid,
    now: DateTime<Utc>,
    reason: Option<&str>,
) -> QueryResult<Finding> {
    diesel::update(findings::table.find(id))
        .set((
            findings::state.eq(FindingState::Dismissed),
            findings::decided_by.eq(by),
            findings::decided_at.eq(now),
            findings::dismissal_reason.eq(reason),
        ))
        .returning(Finding::as_returning())
        .get_result(connection)
        .await
}

#[cfg(test)]
mod tests;
