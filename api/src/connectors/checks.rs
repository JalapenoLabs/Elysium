// Copyright © 2026 Jalapeno Labs

//! Connector checks: what proves a connector can do its job, run live while it is set up and
//! again on demand.
//!
//! Each kind declares its checks in order ([`CheckSpec`]): an id, whether the connector is any
//! use without it, and the checks it builds on. [`run_in_order`] runs them one at a time; a
//! check whose dependency did not pass is skipped and names that dependency, so a token Jira
//! refuses fails `sign_in` once rather than every check after it.
//!
//! A run lives in memory ([`CheckRuns`]) while the web app follows it. Its progress is
//! published as `connectorCheckRun.updated`, ids and states only, since every client hears every
//! event and a check's message can name what an account reaches; the run itself, messages and
//! all, answers the person who started it. A stored connector's run also replaces each check's
//! last result (`crate::models::connector_check`), so the hub shows them after a reload.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::ConnectorKind;
use crate::realtime::{EventBus, ServerEvent};

/// How long a finished run stays readable. Long enough for the page that started it to read
/// its result; a stored connector's results outlive it in the database.
const RUN_RETENTION: Duration = Duration::from_mins(30);

/// One check a kind runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckSpec {
    /// Its id, such as `sign_in`. The web app names it from its kind's translations.
    pub id: &'static str,
    /// Whether the connector is any use when it fails.
    pub required: bool,
    /// The checks that must pass before it can run.
    pub depends_on: &'static [&'static str],
}

/// Where a check stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Queued,
    Running,
    Passed,
    Failed,
    Skipped,
}

impl CheckState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// What a check found. The web app words it: `count` fills its kind's sentence for the check,
/// `reason` is why in Elysium's own terms (`no_messages`), `message` is what the provider said,
/// and `blocked_by` names the dependency that did not pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub state: CheckState,
    pub count: Option<u64>,
    pub reason: Option<&'static str>,
    pub message: Option<String>,
    pub blocked_by: Option<&'static str>,
}

impl Outcome {
    pub const fn passed() -> Self {
        Self {
            state: CheckState::Passed,
            count: None,
            reason: None,
            message: None,
            blocked_by: None,
        }
    }

    /// Passed, having found `count` of what it looks for.
    pub fn counted(count: usize) -> Self {
        Self {
            count: Some(u64::try_from(count).unwrap_or(u64::MAX)),
            ..Self::passed()
        }
    }

    /// Failed on what the provider answered, which `message` repeats.
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            state: CheckState::Failed,
            message: Some(message.into()),
            ..Self::passed()
        }
    }

    /// Failed for a reason in Elysium's own terms, such as a browser that is not running.
    pub fn failed_because(reason: &'static str) -> Self {
        Self {
            state: CheckState::Failed,
            reason: Some(reason),
            ..Self::passed()
        }
    }

    /// Not run, for a reason of its own rather than a dependency, such as an inbox with no
    /// message to sample.
    pub fn skipped(reason: &'static str) -> Self {
        Self {
            state: CheckState::Skipped,
            reason: Some(reason),
            ..Self::passed()
        }
    }

    fn blocked(by: &'static str) -> Self {
        Self {
            state: CheckState::Skipped,
            blocked_by: Some(by),
            ..Self::passed()
        }
    }

    fn running() -> Self {
        Self {
            state: CheckState::Running,
            ..Self::passed()
        }
    }

    fn queued() -> Self {
        Self {
            state: CheckState::Queued,
            ..Self::passed()
        }
    }
}

/// A kind's checks against one account and content. Checks run in order on one value, so a
/// later check can use what an earlier one fetched.
pub trait Checks: Send {
    /// Runs the check `id`, one of its kind's [`CheckSpec`]s.
    fn check(&mut self, id: &'static str) -> impl Future<Output = Outcome> + Send;
}

/// Runs `specs` in order against `checks`, calling `report` as each starts and ends, and
/// answers every outcome. A check whose dependency did not pass is skipped without running.
pub async fn run_in_order<Kind: Checks>(
    specs: &[CheckSpec],
    checks: &mut Kind,
    mut report: impl FnMut(&'static str, &Outcome) + Send,
) -> Vec<(&'static str, Outcome)> {
    let mut outcomes: Vec<(&'static str, Outcome)> = Vec::with_capacity(specs.len());
    for spec in specs {
        let blocker = spec.depends_on.iter().copied().find(|dependency| {
            !outcomes
                .iter()
                .any(|(id, outcome)| id == dependency && outcome.state == CheckState::Passed)
        });
        let outcome = if let Some(dependency) = blocker {
            Outcome::blocked(dependency)
        } else {
            report(spec.id, &Outcome::running());
            checks.check(spec.id).await
        };
        report(spec.id, &outcome);
        outcomes.push((spec.id, outcome));
    }
    outcomes
}

/// One check in a run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckProgress {
    pub check_id: &'static str,
    pub required: bool,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// A run of one connector's checks, or of a connector not stored yet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRun {
    pub id: Uuid,
    pub kind: ConnectorKind,
    /// The stored connector it checks, or `None` for one being set up.
    pub connector_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub checks: Vec<CheckProgress>,
    #[serde(skip)]
    owner: Uuid,
}

/// Every run in memory. Cheap to clone; clones share them.
#[derive(Clone)]
pub struct CheckRuns {
    runs: Arc<Mutex<HashMap<Uuid, CheckRun>>>,
    events: EventBus,
}

impl std::fmt::Debug for CheckRuns {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CheckRuns").finish_non_exhaustive()
    }
}

impl CheckRuns {
    pub fn new(events: EventBus) -> Self {
        Self {
            runs: Arc::new(Mutex::new(HashMap::new())),
            events,
        }
    }

    /// Starts a run of `specs` for `owner`, every check queued, and answers it. Runs finished
    /// longer ago than [`RUN_RETENTION`] are forgotten here.
    pub fn start(
        &self,
        kind: ConnectorKind,
        connector_id: Option<Uuid>,
        owner: Uuid,
        specs: &[CheckSpec],
    ) -> CheckRun {
        let run = CheckRun {
            id: Uuid::now_v7(),
            kind,
            connector_id,
            started_at: Utc::now(),
            finished_at: None,
            checks: specs
                .iter()
                .map(|spec| CheckProgress {
                    check_id: spec.id,
                    required: spec.required,
                    outcome: Outcome::queued(),
                })
                .collect(),
            owner,
        };
        let mut runs = self.lock();
        let forgotten_before =
            Utc::now() - chrono::Duration::from_std(RUN_RETENTION).unwrap_or(chrono::Duration::MAX);
        runs.retain(|_id, kept| kept.finished_at.is_none_or(|at| at > forgotten_before));
        runs.insert(run.id, run.clone());
        run
    }

    /// Records where `check_id` stands in the run `run_id`, and announces it.
    pub fn update(&self, run_id: Uuid, check_id: &'static str, outcome: &Outcome) {
        let connector_id = {
            let mut runs = self.lock();
            let Some(run) = runs.get_mut(&run_id) else {
                return;
            };
            if let Some(progress) = run
                .checks
                .iter_mut()
                .find(|progress| progress.check_id == check_id)
            {
                progress.outcome = outcome.clone();
            }
            run.connector_id
        };
        self.events.publish(&ServerEvent::ConnectorCheckRunUpdated {
            run_id,
            connector_id,
            check_id: Some(check_id.to_owned()),
            state: outcome.state.as_str().to_owned(),
        });
    }

    /// Marks the run `run_id` finished, and announces it.
    pub fn finish(&self, run_id: Uuid) {
        let connector_id = {
            let mut runs = self.lock();
            let Some(run) = runs.get_mut(&run_id) else {
                return;
            };
            run.finished_at = Some(Utc::now());
            run.connector_id
        };
        self.events.publish(&ServerEvent::ConnectorCheckRunUpdated {
            run_id,
            connector_id,
            check_id: None,
            state: "finished".to_owned(),
        });
    }

    /// The run `run_id`, when `viewer` started it.
    pub fn get(&self, run_id: Uuid, viewer: Uuid) -> Option<CheckRun> {
        self.lock()
            .get(&run_id)
            .filter(|run| run.owner == viewer)
            .cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, CheckRun>> {
        self.runs
            .lock()
            .expect("no thread panics while holding the check runs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers each check from a fixed table and remembers the order they ran in.
    struct Scripted {
        answers: HashMap<&'static str, Outcome>,
        ran: Vec<&'static str>,
    }

    impl Checks for Scripted {
        fn check(&mut self, id: &'static str) -> impl Future<Output = Outcome> + Send {
            self.ran.push(id);
            std::future::ready(
                self.answers
                    .get(id)
                    .cloned()
                    .unwrap_or_else(Outcome::passed),
            )
        }
    }

    const SPECS: [CheckSpec; 4] = [
        CheckSpec {
            id: "sign_in",
            required: true,
            depends_on: &[],
        },
        CheckSpec {
            id: "list_projects",
            required: true,
            depends_on: &["sign_in"],
        },
        CheckSpec {
            id: "sample_issues",
            required: false,
            depends_on: &["list_projects"],
        },
        CheckSpec {
            id: "list_boards",
            required: false,
            depends_on: &["sign_in"],
        },
    ];

    #[tokio::test]
    async fn checks_run_in_order_when_each_passes() {
        let mut checks = Scripted {
            answers: HashMap::new(),
            ran: Vec::new(),
        };
        let outcomes = run_in_order(&SPECS, &mut checks, |_id, _outcome| {}).await;

        assert_eq!(
            checks.ran,
            ["sign_in", "list_projects", "sample_issues", "list_boards"]
        );
        assert!(
            outcomes
                .iter()
                .all(|(_id, outcome)| outcome.state == CheckState::Passed)
        );
    }

    #[tokio::test]
    async fn a_failed_check_skips_everything_built_on_it_and_nothing_else() {
        let mut checks = Scripted {
            answers: HashMap::from([("list_projects", Outcome::failed("refused"))]),
            ran: Vec::new(),
        };
        let outcomes = run_in_order(&SPECS, &mut checks, |_id, _outcome| {}).await;

        assert_eq!(checks.ran, ["sign_in", "list_projects", "list_boards"]);
        let sample = &outcomes[2].1;
        assert_eq!(sample.state, CheckState::Skipped);
        assert_eq!(sample.blocked_by, Some("list_projects"));
        assert_eq!(outcomes[3].1.state, CheckState::Passed);
    }

    #[tokio::test]
    async fn a_skipped_check_blocks_its_dependents_too() {
        let mut checks = Scripted {
            answers: HashMap::from([("sign_in", Outcome::skipped("no_browser"))]),
            ran: Vec::new(),
        };
        let outcomes = run_in_order(&SPECS, &mut checks, |_id, _outcome| {}).await;

        assert_eq!(checks.ran, ["sign_in"]);
        assert!(
            outcomes[1..]
                .iter()
                .all(|(_id, outcome)| outcome.blocked_by.is_some())
        );
    }

    #[tokio::test]
    async fn every_check_reports_running_then_its_outcome() {
        let mut checks = Scripted {
            answers: HashMap::from([("sign_in", Outcome::failed("refused"))]),
            ran: Vec::new(),
        };
        let mut reported: Vec<(&'static str, CheckState)> = Vec::new();
        run_in_order(&SPECS[..2], &mut checks, |id, outcome| {
            reported.push((id, outcome.state));
        })
        .await;

        assert_eq!(
            reported,
            [
                ("sign_in", CheckState::Running),
                ("sign_in", CheckState::Failed),
                ("list_projects", CheckState::Skipped),
            ]
        );
    }

    #[test]
    fn a_run_answers_only_the_person_who_started_it() {
        let runs = CheckRuns::new(EventBus::in_process());
        let owner = Uuid::now_v7();
        let run = runs.start(ConnectorKind::Jira, None, owner, &SPECS);

        assert!(runs.get(run.id, owner).is_some());
        assert!(runs.get(run.id, Uuid::now_v7()).is_none());
    }

    #[test]
    fn updates_land_on_their_check() {
        let runs = CheckRuns::new(EventBus::in_process());
        let owner = Uuid::now_v7();
        let run = runs.start(ConnectorKind::Jira, None, owner, &SPECS);
        runs.update(run.id, "list_projects", &Outcome::counted(6));
        runs.finish(run.id);

        let stored = runs.get(run.id, owner).expect("the run");
        assert!(stored.finished_at.is_some());
        assert_eq!(stored.checks[1].outcome.count, Some(6));
        assert_eq!(stored.checks[0].outcome.state, CheckState::Queued);
    }
}
