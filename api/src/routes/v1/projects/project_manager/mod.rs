// Copyright © 2026 Jalapeno Labs

//! `/api/v1/projects/{id}/project-manager`: a project's Project Manager, as its page shows it:
//! what it is doing, its latest turns with what woke each, its batches, its promises, and how its
//! changes reach the work; one turn with its log; a person writing to it; and the project's
//! settings for it: whether its changes apply at once, and the tracker its issues file in. Its
//! turns quote what was ingested, so these answer the company's members alone, as the knowledge
//! base does. See `docs/engine.md`, Project Managers.

pub mod clear_tracker;
pub mod get_project_manager;
pub mod get_turn;
pub mod list_destinations;
pub mod send_message;
pub mod set_tracker;
pub mod update_project_manager;

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::action_item_link::LinkProvider;
use crate::models::project_manager::{Turn, TurnStatus, Wake, WakeKind};
use crate::models::{github_credential, project, token_account};
use crate::project_managers::status::Status;
use crate::project_managers::trackers::{self, TrackerSource};
use crate::project_managers::{Snapshot, WritesSnapshot};
use crate::routes::v1::promises::PromiseResponse;
use crate::routes::v1::work_batches::BatchResponse;

/// The credential a tracker on `provider` files through for `project_id`, as `person` names it,
/// refused when there is none they may name.
///
/// A Jira tracker names a Jira credential, which is the workspace's. A YouTrack tracker names a
/// YouTrack account, which is its owner's alone, so only its owner names it. A GitHub tracker
/// names none: it files with the token the project's sessions get, its own or the workspace
/// default (`crate::project_managers::trackers`).
///
/// # Errors
/// [`ApiError::BadRequest`] for a credential named where none is taken or missing where one is,
/// and for a project whose sessions get no GitHub token; [`ApiError::NotFound`] for a YouTrack
/// account that is not `person`'s; and any database error.
pub async fn tracker_credential(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    provider: LinkProvider,
    credential_id: Option<Uuid>,
    person: Uuid,
) -> Result<Uuid, ApiError> {
    match (provider, credential_id) {
        (LinkProvider::Github, None) => {
            let project = project::find(connection, project_id).await?;
            let workspace_default = github_credential::find_default(connection)
                .await?
                .map(|credential| credential.id);
            project
                .github_credential(workspace_default)
                .ok_or_else(|| ApiError::BadRequest(trackers::NO_GITHUB_TOKEN.to_owned()))
        }
        (LinkProvider::Github, Some(_credential_id)) => Err(ApiError::BadRequest(
            "a GitHub tracker files with the project's own token; leave credentialId out"
                .to_owned(),
        )),
        (LinkProvider::Jira | LinkProvider::Youtrack, None) => Err(ApiError::BadRequest(
            "credentialId names the Jira credential or YouTrack account that files there"
                .to_owned(),
        )),
        (LinkProvider::Jira, Some(credential_id)) => Ok(credential_id),
        (LinkProvider::Youtrack, Some(credential_id)) => {
            token_account::find_owned(connection, credential_id, person).await?;
            Ok(credential_id)
        }
    }
}

/// A project's Project Manager, as its page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManagerResponse {
    project_id: Uuid,
    status: Status,
    /// How many wakes wait for its next turn.
    waiting: i64,
    /// When a usage limit lets it start again, while one holds it.
    usage_paused_until: Option<DateTime<Utc>>,
    /// Its latest turns, newest first.
    turns: Vec<TurnResponse>,
    /// Its latest batches, newest first.
    batches: Vec<BatchResponse>,
    /// Every batch of its waiting for a person's approval, newest first, up to 200.
    proposed: Vec<BatchResponse>,
    /// Its latest promises, newest first.
    promises: Vec<PromiseResponse>,
    writes: WritesResponse,
}

/// How a Project Manager's changes reach the work.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WritesResponse {
    /// Whether its changes apply as it makes them, rather than waiting for a person.
    direct: bool,
    /// Where its issues file; `null` when there is nowhere, and `trackerRefusal` says why.
    tracker: Option<TrackerResponse>,
    tracker_refusal: Option<&'static str>,
}

/// A project's tracker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerResponse {
    source: TrackerSource,
    provider: LinkProvider,
    /// The Jira credential, GitHub token, or YouTrack account that files there.
    credential_id: Uuid,
    credential_name: String,
    /// A Jira project key, a GitHub `owner/name`, or a YouTrack project's short name.
    destination: String,
}

impl From<WritesSnapshot> for WritesResponse {
    fn from(writes: WritesSnapshot) -> Self {
        let (tracker, tracker_refusal) = match writes.tracker {
            Ok(snapshot) => (
                Some(TrackerResponse {
                    source: snapshot.tracker.source,
                    provider: snapshot.tracker.place.provider,
                    credential_id: snapshot.tracker.credential_id,
                    credential_name: snapshot.credential_name,
                    destination: snapshot.tracker.place.destination,
                }),
                None,
            ),
            Err(reason) => (None, Some(reason)),
        };
        Self {
            direct: writes.direct,
            tracker,
            tracker_refusal,
        }
    }
}

impl From<Snapshot> for ProjectManagerResponse {
    fn from(snapshot: Snapshot) -> Self {
        let now = Utc::now();
        Self {
            project_id: snapshot.project_id,
            status: snapshot.status,
            waiting: snapshot.waiting,
            usage_paused_until: snapshot
                .state
                .and_then(|state| state.usage_paused_until)
                .filter(|until| *until > now),
            turns: snapshot
                .turns
                .into_iter()
                .map(|(turn, wakes)| TurnResponse::new(turn, wakes))
                .collect(),
            batches: snapshot
                .batches
                .into_iter()
                .map(BatchResponse::from)
                .collect(),
            proposed: snapshot
                .proposed
                .into_iter()
                .map(BatchResponse::from)
                .collect(),
            promises: snapshot
                .promises
                .into_iter()
                .map(|(promise, record, draft_id)| {
                    PromiseResponse::new(promise, record.as_ref(), draft_id)
                })
                .collect(),
            writes: WritesResponse::from(snapshot.writes),
        }
    }
}

/// One turn, collapsed to its summary, with what woke it and what it answered.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResponse {
    id: Uuid,
    status: TurnStatus,
    summary: Option<String>,
    reply: Option<String>,
    error: Option<String>,
    /// The record it put on the project's timeline.
    timeline_record_id: Option<Uuid>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    /// What woke it, oldest first.
    wakes: Vec<WakeResponse>,
}

impl TurnResponse {
    pub fn new(turn: Turn, wakes: Vec<Wake>) -> Self {
        Self {
            id: turn.id,
            status: turn.status,
            summary: turn.summary,
            reply: turn.reply,
            error: turn.error,
            timeline_record_id: turn.timeline_record_id,
            started_at: turn.started_at,
            finished_at: turn.finished_at,
            wakes: wakes.into_iter().map(WakeResponse::from).collect(),
        }
    }
}

/// One thing that woke a turn: a message with its text, a record, a batch, or a promise, by id.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeResponse {
    id: Uuid,
    kind: WakeKind,
    timeline_record_id: Option<Uuid>,
    work_batch_id: Option<Uuid>,
    promise_id: Option<Uuid>,
    body: Option<String>,
    /// Who caused it: the person or Elysia who wrote, or whoever attributed the record.
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<Wake> for WakeResponse {
    fn from(wake: Wake) -> Self {
        Self {
            id: wake.id,
            kind: wake.kind,
            timeline_record_id: wake.timeline_record_id,
            work_batch_id: wake.work_batch_id,
            promise_id: wake.promise_id,
            body: wake.body,
            created_by: wake.created_by,
            created_at: wake.created_at,
        }
    }
}
