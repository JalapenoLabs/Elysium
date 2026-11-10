// Copyright © 2026 Jalapeno Labs

//! Findings: how agent-found work reaches a person. See `docs/autonomy.md`, Findings, and
//! `docs/engine.md`, Findings.
//!
//! Agent-found work never queues itself. A worker or a Project Manager that notices work nobody
//! asked for reports it as a finding ([`report`], behind `worker_report_finding` and
//! `manager_report_finding`): what it saw, where, and what acting on it would cost. It files no
//! issue and submits no batch. The finding waits on its project's Findings list until a person
//! decides:
//!
//! - **Accepting** ([`accept`]) is the person's own action: it makes an open action item in the
//!   project, as theirs, owes the item's issue in the project's tracker through the watcher's
//!   outbox (`crate::models::action_item_issue_filing`), filed as them, and submits a batch for
//!   the item. The batch is human-rooted and generation 0 (`Lineage::accepted_finding`), since a
//!   person decided the work is wanted, so like any batch that traces to a person it waits for
//!   approval, or queues at once, as the project's level of autonomy says
//!   (`crate::models::project_autonomy`).
//! - **Dismissing** ([`dismiss`]) keeps the finding, with why when the person said, so the same
//!   finding reported again lands on it and stays dismissed (`crate::models::finding`).
//!
//! Only a person decides: both take a `MemberUser`, which no machine user can be, so no agent's
//! tool reaches them.

use chrono::{DateTime, Utc};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::auth::MemberUser;
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::action_item_issue_filing::{self, FiledBy, NewFiling};
use crate::models::action_item_link::LinkCredential;
use crate::models::finding::{
    self, Cost, DETAIL_MAX_CHARACTERS, DISMISSAL_REASON_MAX_CHARACTERS, DecideError, Finding,
    FindingState, LOCATION_MAX_CHARACTERS, NewFinding, Reported, Reporter, TITLE_MAX_CHARACTERS,
};
use crate::models::work_batch::lineage::Lineage;
use crate::models::work_batch::{NewWorkBatch, Priority, Repository};
use crate::project_managers::batches::{self, Stored};
use crate::project_managers::trackers::{self, Place};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::coding_sessions::validate_repository_url;

/// The longest repository URL, matching `findings.repository_url`.
const REPOSITORY_URL_MAX_CHARACTERS: usize = 2_048;

/// A finding as an agent reports it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub title: String,
    pub detail: String,
    #[serde(default)]
    pub repository_url: Option<String>,
    pub location: String,
    pub cost: Cost,
}

/// The input schema `worker_report_finding` and `manager_report_finding` share.
pub fn report_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "minLength": 1, "maxLength": TITLE_MAX_CHARACTERS },
            "detail": { "type": "string", "minLength": 1, "maxLength": DETAIL_MAX_CHARACTERS },
            "repositoryUrl": {
                "type": "string",
                "minLength": 1,
                "maxLength": REPOSITORY_URL_MAX_CHARACTERS,
            },
            "location": { "type": "string", "minLength": 1, "maxLength": LOCATION_MAX_CHARACTERS },
            "cost": { "type": "string", "enum": ["small", "medium", "large"] },
        },
        "required": ["title", "detail", "location", "cost"],
        "additionalProperties": false,
    })
}

/// `report` tidied, each text trimmed, and held to the rules its columns hold: a title, a detail,
/// and a place, each within its length, and a repository Elysium could clone, since accepting the
/// finding names it in a batch. Answers what to change.
///
/// # Errors
/// Names the first rule `report` breaks.
pub fn check(report: Report) -> Result<NewFinding, String> {
    let title = report.title.trim().to_owned();
    let detail = report.detail.trim().to_owned();
    let location = report.location.trim().to_owned();
    for (name, text, most) in [
        ("title", &title, TITLE_MAX_CHARACTERS),
        ("detail", &detail, DETAIL_MAX_CHARACTERS),
        ("location", &location, LOCATION_MAX_CHARACTERS),
    ] {
        if text.is_empty() || text.chars().count() > most {
            return Err(format!("{name} must hold text, at most {most} characters"));
        }
    }
    let repository_url = report
        .repository_url
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty());
    if let Some(url) = &repository_url {
        if url.chars().count() > REPOSITORY_URL_MAX_CHARACTERS {
            return Err(format!(
                "repositoryUrl holds at most {REPOSITORY_URL_MAX_CHARACTERS} characters"
            ));
        }
        if let Err(refusal) = validate_repository_url(url) {
            let reason = refusal
                .message
                .unwrap_or_else(|| "is not a repository URL".into());
            return Err(format!("repositoryUrl {url}: {reason}"));
        }
    }
    Ok(NewFinding {
        title,
        detail,
        repository_url,
        location,
        cost: report.cost,
    })
}

/// Reports `report` on `project_id`, by `reporter`, and announces it. Answers how it landed: new,
/// or the same finding reported before, with what a person made of it.
///
/// # Errors
/// [`FindingError::Invalid`] naming what to change, when [`check`] refuses it; any database error.
pub async fn report(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    project_id: Uuid,
    reporter: Reporter,
    report: Report,
) -> Result<Reported, FindingError> {
    let new_finding = check(report).map_err(FindingError::Invalid)?;
    let landed = finding::report(connection, project_id, reporter, new_finding, Utc::now()).await?;
    tracing::event!(
        name: "finding.reported",
        tracing::Level::INFO,
        project.id = %project_id,
        finding.id = %landed.finding.id,
        finding.is_new = landed.is_new,
        "an agent reported a finding",
    );
    publish(events, &landed.finding);
    Ok(landed)
}

/// What an agent is told once its report landed: the finding, and what to do now, which for one
/// reported before is what a person made of it.
pub fn answer(reported: &Reported) -> Value {
    let finding = &reported.finding;
    let next = if reported.is_new {
        "Recorded on the project's Findings list, where a person accepts or dismisses it. Do not \
         file an issue for it, submit a batch for it, or start on it: carry on with your own work."
            .to_owned()
    } else {
        match finding.state {
            FindingState::Open => "This finding is already on the project's Findings list, \
                waiting for a person; your report was counted on it. There is nothing more to do."
                .to_owned(),
            FindingState::Accepted => format!(
                "A person already accepted this finding: it is action item {} with batch {}, \
                 which does the work. Leave it to that batch.",
                display_id(finding.action_item_id),
                display_id(finding.accepted_batch_id),
            ),
            FindingState::Dismissed => {
                let why = finding
                    .dismissal_reason
                    .as_deref()
                    .map(|reason| format!(", saying: {reason}"))
                    .unwrap_or_default();
                format!(
                    "A person dismissed this finding{why}. Do not report it again, and do not \
                     work on it."
                )
            }
        }
    };
    json!({
        "finding": {
            "id": finding.id,
            "state": finding.state,
            "reports": finding.reports,
        },
        "next": next,
    })
}

/// An id an accepted finding names, or `gone` once what it named was deleted with its project's
/// other rows.
fn display_id(id: Option<Uuid>) -> String {
    id.map_or_else(|| "gone".to_owned(), |id| id.to_string())
}

/// Why reporting or deciding a finding was refused.
#[derive(Debug, thiserror::Error)]
pub enum FindingError {
    /// What an agent or a person sent breaks a rule; the words say which.
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Decide(#[from] DecideError),
    #[error(transparent)]
    Work(#[from] WorkError),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// What accepting a finding made, for the caller to announce once its transaction committed.
#[derive(Debug)]
pub struct Accepted {
    pub finding: Finding,
    pub item: Recorded<ActionItem>,
    pub batch: Stored,
    /// Where the item's issue is owed, or why it is owed nowhere: the project names no tracker
    /// and links no issue, or files on GitHub with no token (`crate::project_managers::trackers`).
    pub issue: Result<Place, &'static str>,
}

/// Accepts open finding `id` as `person`, at `now`, in one transaction under its lock: makes an
/// open action item in its project as the person, owes the item's issue in the project's tracker,
/// filed as them, when the project has one, submits a batch for the item, human-rooted and
/// generation 0, and records the finding accepted.
///
/// A project with nowhere to file still gets the item and the batch: the work is accepted either
/// way, the item is Elysium's record of it, and its issue can be filed once the project names a
/// tracker. The caller says so.
///
/// # Errors
/// [`DecideError::NotFound`] and [`DecideError::Decided`] through [`FindingError::Decide`]; an
/// item or filing the records refuse through [`FindingError::Work`]; any database error.
pub async fn accept(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    person: &MemberUser,
    now: DateTime<Utc>,
) -> Result<Accepted, FindingError> {
    let person_name = person.0.name.clone();
    let person = person.id();
    connection
        .transaction(async |connection| {
            let finding = finding::lock_open(connection, id).await?;
            let item = action_item::create(
                connection,
                NewActionItem {
                    title: finding.title.clone(),
                    notes: issue_notes(&finding),
                    // A person accepting it accepts the item too.
                    state: ActionItemState::Open,
                    priority: ActionItemPriority::Normal,
                    due_at: None,
                    owner: Owner::User,
                    project_ids: vec![finding.project_id],
                    initiative_ids: Vec::new(),
                },
                Actor::User(person),
                now,
            )
            .await?;

            let issue = match trackers::resolve(connection, finding.project_id).await? {
                Ok(tracker) => {
                    action_item_issue_filing::owe(
                        connection,
                        NewFiling {
                            action_item_id: item.record.id,
                            credential: LinkCredential {
                                provider: tracker.place.provider,
                                id: tracker.credential_id,
                            },
                            destination: tracker.place.destination.clone(),
                            filed_by: FiledBy::Person(person),
                            changeset_operation_id: None,
                            timeline_record_id: None,
                        },
                        now,
                    )
                    .await?;
                    Ok(tracker.place)
                }
                Err(reason) => Err(reason),
            };

            let repositories: Vec<Repository> = finding
                .repository_url
                .iter()
                .map(|url| Repository {
                    url: url.clone(),
                    base_branch: None,
                })
                .collect();
            let batch = batches::store(
                connection,
                NewWorkBatch {
                    id: Uuid::now_v7(),
                    project_id: finding.project_id,
                    title: finding.title.clone(),
                    goal: batch_goal(&finding),
                    repositories: json!(repositories),
                    priority: Priority::Normal,
                    scheduled_for: None,
                    turn_id: None,
                    source_record_ids: json!([]),
                    action_item_ids: json!([item.record.id]),
                    source_message_ids: json!([]),
                    lineage: Lineage::accepted_finding(),
                    created_by: person,
                },
                &person_name,
            )
            .await?;

            let batch_id = batch.batch().id;
            let finding =
                finding::record_accepted(connection, id, person, now, item.record.id, batch_id)
                    .await?;
            Ok(Accepted {
                finding,
                item,
                batch,
                issue,
            })
        })
        .await
}

/// Dismisses open finding `id` as `person`, at `now`, saying `reason` when they gave one, under its
/// lock. The finding stays, so reporting it again lands on it.
///
/// # Errors
/// [`FindingError::Invalid`] for a reason longer than it may be; [`DecideError::NotFound`] and
/// [`DecideError::Decided`] through [`FindingError::Decide`]; any database error.
pub async fn dismiss(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    person: &MemberUser,
    reason: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Finding, FindingError> {
    let reason = reason.map(str::trim).filter(|reason| !reason.is_empty());
    if reason.is_some_and(|reason| reason.chars().count() > DISMISSAL_REASON_MAX_CHARACTERS) {
        return Err(FindingError::Invalid(format!(
            "reason holds at most {DISMISSAL_REASON_MAX_CHARACTERS} characters"
        )));
    }
    let person = person.id();
    connection
        .transaction(async |connection| {
            finding::lock_open(connection, id).await?;
            let finding = finding::record_dismissed(connection, id, person, now, reason).await?;
            Ok(finding)
        })
        .await
}

/// Announces that `finding` was reported, accepted, or dismissed: its ids and state alone, since
/// what it says quotes agents' work, which members load.
pub fn publish(events: &EventBus, finding: &Finding) {
    events.publish(&ServerEvent::FindingChanged {
        id: finding.id,
        project_id: finding.project_id,
        state: finding.state,
    });
}

/// An accepted finding's item notes, which its issue opens with: what was seen, where, and what
/// it would cost, for a teammate reading only the issue.
fn issue_notes(finding: &Finding) -> String {
    format!(
        "{}\n\nWhere: {}\nCost: {}",
        finding.detail,
        finding.place(),
        finding.cost.words()
    )
}

/// An accepted finding's batch goal: what was seen and where, and that a person asked for it by
/// accepting it, for a worker who has seen nothing else.
fn batch_goal(finding: &Finding) -> String {
    format!(
        "{}\n\nWhere: {}\n\nAn agent noticed this, and a person accepted it from the project's \
         Findings list, so the work is wanted. Do what it describes, and nothing more.",
        finding.detail,
        finding.place()
    )
}

#[cfg(test)]
mod tests;
