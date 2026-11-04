// Copyright © 2026 Jalapeno Labs

//! Merging a worker's pull requests: who merges each, when Elysium does, and what a merge puts on
//! the project's timeline. See `docs/engine.md`, Merging.
//!
//! Each repository a project declares says who merges the pull requests its workers open
//! (`crate::models::worker_setup::MergePolicy`): a person, by default, or Elysium. A worker records
//! each pull request it opens (`crate::tools::worker`); nothing else is ever merged, and the worker
//! never merges one itself.
//!
//! # When it looks
//!
//! The engine looks at a batch's open pull requests ([`Merging::look`]) when its worker's wait
//! ends, before the turn that follows, and once more before the batch finishes. A worker waits on
//! CI or a review while it keeps its container, so the look follows the worker's own loop: nothing
//! new polls GitHub, and a pull request is read once per wait.
//!
//! # What a look does
//!
//! Each pull request is read through the project's GitHub token, its base branch's rules too
//! when Elysium may merge it, and [`decide::decide`] says what follows, failing closed: one that
//! merged already, by anyone, is recorded as merged; one closed is recorded closed; one Elysium
//! may merge and nothing holds is merged naming its head commit, so a push since the read makes
//! GitHub refuse it rather than merge what was not read; and one held keeps every reason it was
//! held (`held_because`), which the worker's next turn and the batch's page read. A merge is
//! recorded, put on the timeline, and wakes the project's Project Manager in one transaction
//! that moves the pull request from `open` alone, so a merge is recorded once however often it
//! is read. An engine that stops between GitHub merging and the record finds it merged on its
//! next look and records it then, naming whoever GitHub says merged it.
//!
//! Promises tied to the batch become due when the batch succeeds, which follows its merges
//! (`crate::models::work_batch::finish`); those tied to an action item, when the item resolves,
//! which the Project Manager the merge woke decides.

pub mod decide;
pub mod timeline;

use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use secrecy::SecretString;
use tracing::{Level, event};
use uuid::Uuid;

use self::decide::Verdict;
use crate::database::Pool;
use crate::github::issues::IssueRef;
use crate::github::pulls::PullRequestStatus;
use crate::github::{Github, Repository};
use crate::models::timeline_record::TimelineRecord;
use crate::models::work_batch::WorkBatch;
use crate::models::work_batch::pull_request::{
    self, BatchPullRequest, Merge, MergeMethod, PullRequestState,
};
use crate::models::worker_setup::{self, MergePolicy, WorkerSetup};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::coding_sessions::repository_identity;
use crate::worker_pool::actions::announce;

/// What a look at a pull request a person merges says of it.
const PERSON_MERGES: &str = "A person merges pull requests in this repository.";

/// Who merges the pull requests a worker opens in the repository at `url`: what the project
/// declares for it, or a person for a repository it does not declare.
pub fn policy_for(setup: &WorkerSetup, url: &str) -> MergePolicy {
    let identity = repository_identity(url);
    setup
        .repositories
        .iter()
        .find(|declared| repository_identity(&declared.url) == identity)
        .map_or_else(MergePolicy::default, |declared| declared.merge_policy)
}

/// What looks at workers' pull requests. Cheap to clone.
#[derive(Clone)]
pub struct Merging {
    github: Github,
    database: Pool,
    events: EventBus,
}

impl std::fmt::Debug for Merging {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pool has nothing useful to print.
        formatter.debug_struct("Merging").finish_non_exhaustive()
    }
}

impl Merging {
    pub const fn new(github: Github, database: Pool, events: EventBus) -> Self {
        Self {
            github,
            database,
            events,
        }
    }

    /// Looks at every open pull request `batch` recorded, through the project's `token`, merging
    /// what Elysium may and recording what merged. Never fails: a pull request that cannot be
    /// read is held saying why, and what goes wrong otherwise is logged and looked at again next
    /// time.
    pub async fn look(&self, batch: &WorkBatch, token: Option<&SecretString>) {
        if let Err(error) = self.look_at_batch(batch, token).await {
            event!(
                name: "worker_pool.merging.look_failure",
                Level::WARN,
                work_batch.id = %batch.id,
                error.message = %format!("{error:#}"),
                "could not look at a batch's pull requests; the next look tries again",
            );
        }
    }

    async fn look_at_batch(
        &self,
        batch: &WorkBatch,
        token: Option<&SecretString>,
    ) -> anyhow::Result<()> {
        let (open, setup) = {
            let mut connection = self.database.get().await?;
            let recorded = pull_request::of_batch(&mut connection, batch.id).await?;
            let open: Vec<BatchPullRequest> = recorded
                .into_iter()
                .filter(|recorded| recorded.state == PullRequestState::Open)
                .collect();
            if open.is_empty() {
                return Ok(());
            }
            // Read now, so a change to who merges reaches the next look.
            let setup = worker_setup::find(&mut connection, batch.project_id).await?;
            (open, setup)
        };
        let mut changed = false;
        for recorded in &open {
            let policy = policy_for(&setup, &recorded.github_url());
            let outcome = match token {
                Some(token) => self.look_at(recorded, policy, token).await,
                None => Outcome::Held(
                    "The project's workers have no GitHub token, so Elysium cannot read it."
                        .to_owned(),
                ),
            };
            changed |= self.land(batch, recorded, outcome).await?;
        }
        if changed {
            announce(&self.events, batch);
        }
        Ok(())
    }

    /// What one look finds a pull request to be, merging it when it may.
    async fn look_at(
        &self,
        recorded: &BatchPullRequest,
        policy: MergePolicy,
        token: &SecretString,
    ) -> Outcome {
        let Some(reference) = reference_of(recorded) else {
            return Outcome::Held("Elysium cannot read its repository's name.".to_owned());
        };
        let pull_request = match self.github.pull_request(token, &reference).await {
            Ok(Some(pull_request)) => pull_request,
            Ok(None) => {
                return Outcome::Held(
                    "The project's GitHub token cannot see it any more.".to_owned(),
                );
            }
            Err(error) => {
                return Outcome::Held(format!(
                    "GitHub could not be read: {}.",
                    error.to_string().trim_end_matches('.')
                ));
            }
        };
        // The rules matter only to a merge Elysium might make.
        let rules = if policy == MergePolicy::Auto && pull_request.status == PullRequestStatus::Open
        {
            self.github
                .branch_rules(token, &reference.repository, &pull_request.base_branch)
                .await
                .inspect_err(|error| {
                    event!(
                        name: "worker_pool.merging.rules_unread",
                        Level::INFO,
                        pull_request = %reference,
                        error.message = %error,
                        "could not read a base branch's rules; the pull request is held",
                    );
                })
                .ok()
        } else {
            None
        };
        match decide::decide(policy, &pull_request, rules.as_ref()) {
            Verdict::Merged { by, at } => Outcome::Merged {
                by: by.unwrap_or_else(|| "someone".to_owned()),
                at: at.unwrap_or_else(Utc::now),
                method: None,
            },
            Verdict::Closed => Outcome::Closed,
            Verdict::PersonMerges => Outcome::Held(PERSON_MERGES.to_owned()),
            Verdict::Hold(holds) => Outcome::Held(decide::describe(&holds)),
            Verdict::Merge(method) => {
                let merged = self
                    .github
                    .merge_pull_request(token, &reference, method, &pull_request.head_oid)
                    .await;
                match merged {
                    Ok(_commit) => {
                        event!(
                            name: "worker_pool.merging.merged",
                            Level::INFO,
                            pull_request = %reference,
                            merge.method = method.as_str(),
                            "Elysium merged a worker's pull request",
                        );
                        Outcome::Merged {
                            by: pull_request.viewer,
                            at: Utc::now(),
                            method: Some(method),
                        }
                    }
                    Err(error) => Outcome::Held(format!(
                        "GitHub refused the merge: {}.",
                        error.to_string().trim_end_matches('.')
                    )),
                }
            }
        }
    }

    /// Records what a look found. `true` when the pull request moved on.
    async fn land(
        &self,
        batch: &WorkBatch,
        recorded: &BatchPullRequest,
        outcome: Outcome,
    ) -> anyhow::Result<bool> {
        let mut connection = self.database.get().await?;
        let now = Utc::now();
        match outcome {
            Outcome::Held(reason) => {
                pull_request::hold(&mut connection, recorded.id, &reason, now).await?;
                Ok(false)
            }
            Outcome::Closed => {
                pull_request::close(&mut connection, recorded.id, now).await?;
                Ok(true)
            }
            Outcome::Merged { by, at, method } => {
                let merge = Merge {
                    at,
                    by: &by,
                    method,
                };
                let Some(record) = record_merge(&mut connection, batch, recorded.id, merge).await?
                else {
                    return Ok(false);
                };
                self.events.publish(&ServerEvent::TimelineRecordCreated {
                    id: record.id,
                    source_kind: record.source_kind,
                    created_by: record.created_by,
                    action_item_id: None,
                    conversation_id: None,
                });
                self.events.signal(&Signal::ProjectManagers);
                Ok(true)
            }
        }
    }
}

/// What one look found a pull request to be.
enum Outcome {
    /// It merged: `by` whoever GitHub names, `method` set when Elysium merged it just now.
    Merged {
        by: String,
        at: chrono::DateTime<Utc>,
        method: Option<MergeMethod>,
    },
    Closed,
    /// It stays open, for this reason.
    Held(String),
}

/// Records pull request `id`'s `merge`, puts it on `batch`'s project's timeline, and wakes the
/// project's Project Manager, all in one transaction, answering the record; `None` when an
/// earlier look recorded the merge already.
async fn record_merge(
    connection: &mut AsyncPgConnection,
    batch: &WorkBatch,
    id: Uuid,
    merge: Merge<'_>,
) -> diesel::QueryResult<Option<TimelineRecord>> {
    connection
        .transaction(async |connection| {
            let Some(merged) = pull_request::mark_merged(connection, id, merge).await? else {
                return Ok(None);
            };
            let record = timeline::record_merge(connection, batch, &merged).await?;
            pull_request::set_timeline_record(connection, id, record.id).await?;
            Ok(Some(record))
        })
        .await
}

/// The pull request a recorded one names on GitHub.
fn reference_of(recorded: &BatchPullRequest) -> Option<IssueRef> {
    let repository = Repository::from_url(&recorded.github_url())?;
    let number = u64::try_from(recorded.number).ok()?;
    Some(IssueRef { repository, number })
}

#[cfg(test)]
mod tests;
