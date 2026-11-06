// Copyright © 2026 Jalapeno Labs

//! One worker's life, from the slot it was handed to its batch's end.
//!
//! 1. **Start.** A batch still `assigned` is started (`work_batch::start`), so no person can
//!    pause or cancel it once a container exists for it. One already `running` was taken over
//!    from an engine that stopped, and its container is found again.
//! 2. **Container.** Its network, then its container and volume, through a runner of its own
//!    (`Runner::prepare`), which also stops any turn an earlier engine left running in it.
//! 3. **Configuration.** `~/.claude` from the configuration repository, fetched into the
//!    worker's volume with the repository's own token and adapted to the container, recorded on
//!    the batch with what was left out (`crate::runner::configuration`). A worker never runs
//!    without it: one that cannot fetch it gives its slot back before its first turn and the
//!    batch waits in the queue, its failure recorded so the pool holds every batch back
//!    (`crate::agent_configuration`). A container that already holds the checkout the batch
//!    recorded keeps it, so every turn of a batch runs on one commit.
//! 4. **Setup**, once per container (`super::setup`).
//! 5. **Turns.** Each turn resumes the worker's Claude Code session, with `/goal` and what this
//!    turn is about (`super::brief`), its tools under a grant for the turn alone, a Blender and
//!    a desktop of its own, and the project's GitHub token. Its events are scrubbed of the
//!    worker's secrets before anything of them is logged. Then the report the worker filed
//!    decides what follows: finished ends the batch and removes the container; waiting keeps the
//!    container and waits for its time, or an answer, then starts the next turn.
//! 6. **Its pull requests.** As each wait ends, and before the batch finishes, the engine looks at
//!    the pull requests the worker recorded (`crate::worker_pool::merging`): it merges those
//!    Elysium may, records what merged, and the next turn's brief says where each stands.
//!
//! A turn that ends without a report, fails, or goes quiet is tried again, telling the worker
//! why, up to [`MAX_STRIKES`] turns in a row before the batch fails. A turn every credential's
//! usage stopped waits for the reset and carries on, keeping the slot. A worker that is stopped
//! (the engine stops, or the slot was lost) leaves its container as it is.

use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use tokio::time::Instant;
use tracing::{Level, event};

use super::Worker;
use super::brief::{self, Batch, PullRequestStanding, Situation};
use super::setup::{self, Plan, Progress};
use crate::agent_configuration;
use crate::agent_models::{self, Agent};
use crate::models::agent_configuration as agent_configuration_status;
use crate::models::user::CODING_AGENT_USER_ID;
use crate::models::work_batch::pull_request;
use crate::models::work_batch::worker::{self as batch_worker, Report};
use crate::models::work_batch::{self, BatchState, EventKind, RESULT_MAX_CHARACTERS, Slot};
use crate::models::{environment_variable, github_credential, project, worker_setup};
use crate::project_managers::runner::TurnLog;
use crate::realtime::Signal;
use crate::routes::v1::coding_sessions::github_token;
use crate::runner::configuration::{Checkout, Fetched, Source};
use crate::runner::workspace::WorkspaceContainer;
use crate::runner::{
    ConversationId, EnvironmentEntry, HarnessSessionId, McpServer, Runner, RunnerEvent, TurnMode,
    TurnOutcome, TurnRequest, TurnStatus, WorkspacePath,
};
use crate::sessions::desktop;
use crate::sessions::queue::{BLENDER_COMMAND, BLENDER_SERVER, turn_environment};
use crate::sessions::scrub::Scrubber;
use crate::tools::{self, CallScope, ToolServer, WorkerScope};
use crate::worker_pool::actions::announce;
use crate::worker_pool::merging::{self, Intent};

/// Turns in a row that may end without a report, fail, or go quiet before the batch fails: a
/// worker that cannot report three times running will not on the fourth.
const MAX_STRIKES: u32 = 3;

/// How often a container is tried for before the batch fails: a Docker proxy restarting is
/// worth waiting out, a daemon out of address pools is not.
const PREPARE_ATTEMPTS: u32 = 3;

/// How long between those tries.
const PREPARE_BACKOFF: Duration = Duration::from_secs(30);

/// How many times one batch's workers may fail to fetch the configuration repository before it
/// fails. Each failure holds the pool until the engine fetches the repository again; a batch
/// whose workers fail where the engine succeeds (a container too slow for a large repository, a
/// network that cannot reach it) would otherwise cycle through the queue for good, raising a new
/// inbox item each time.
const MAX_CONFIGURATION_REQUEUES: usize = 3;

/// How often a running turn's log is stored, so the batch's page follows the worker.
const LOG_FLUSH_INTERVAL: Duration = Duration::from_secs(30);

/// How long a worker whose turn the usage limit stopped waits when no reset is known.
const USAGE_RETRY: Duration = Duration::from_mins(15);

/// How often a waiting worker reads its wait again without a signal, which an answer sends.
const WAIT_LOOK_INTERVAL: Duration = Duration::from_mins(1);

/// Where a worker's desktop serves its view, inside its own container.
const DESKTOP_SOCKET: &str = ".elysium/desktops/worker.sock";

/// How a worker's run ended.
enum Ended {
    /// Its batch ended, and its container was removed.
    Finished,
    /// It was stopped: the engine stops, or the slot was lost. Its container is left.
    Stopped,
    /// The slot's claim was gone when it next wrote.
    Lost,
    /// It gave its slot back before its first turn, and its batch waits in the queue again.
    Requeued,
}

/// Whether a worker has the configuration repository to run on.
enum Configured {
    Ready,
    /// It has none, for this reason: its batch cannot run here now.
    Unavailable(String),
}

/// Why a worker cannot run when no configuration repository is set.
const UNSET_REASON: &str = "No configuration repository is set, and no worker runs without one.";

/// Runs `worker` until its batch ends or it is stopped. A failure is logged; the worker's slot
/// is still held, so the scheduler's next look starts it again.
pub(super) async fn run(worker: Worker) {
    let batch_id = worker.batch_id;
    match worker.work().await {
        Ok(Ended::Finished) => {}
        Ok(Ended::Stopped) => event!(
            name: "worker_pool.worker.stopped",
            Level::INFO,
            work_batch.id = %batch_id,
            "a worker stopped; its container is kept for the engine that takes its slot",
        ),
        Ok(Ended::Lost) => event!(
            name: "worker_pool.worker.slot_lost",
            Level::INFO,
            work_batch.id = %batch_id,
            "a worker's slot was taken over or given back; it stopped",
        ),
        Ok(Ended::Requeued) => event!(
            name: "worker_pool.worker.requeued",
            Level::INFO,
            work_batch.id = %batch_id,
            "a worker had no configuration to run on; its batch waits in the queue again",
        ),
        Err(error) => event!(
            name: "worker_pool.worker.failure",
            Level::ERROR,
            work_batch.id = %batch_id,
            error.message = %format!("{error:#}"),
            "a worker stopped on an error; it starts again on the scheduler's next look",
        ),
    }
}

/// Everything a worker's turns are built from, decided once it started.
struct Job {
    slot: Slot,
    /// Its container, which its runner runs turns in and setup runs scripts in.
    workspace: WorkspaceContainer,
    runner: Runner,
    project_name: String,
    plan: Plan,
    environment: Vec<EnvironmentEntry>,
    /// The values its output is scrubbed of: its GitHub token and every secret variable.
    secrets: Vec<SecretString>,
    /// The project's GitHub token, which its pull requests are read and merged through.
    github_token: Option<SecretString>,
    log: TurnLog,
}

impl Worker {
    async fn work(&self) -> anyhow::Result<Ended> {
        let Some((mut context, plan)) = self.begin().await? else {
            return Ok(Ended::Lost);
        };
        context.plan = match plan {
            Ok(plan) => plan,
            // Nothing was created yet, and nothing will be: the batch cannot be set up.
            Err(refusal) => return self.finish(&mut context, false, &refusal).await,
        };

        if let Err(error) = self.prepare(&context).await {
            return self
                .finish(
                    &mut context,
                    false,
                    &format!("The worker's container could not start: {error:#}"),
                )
                .await;
        }
        self.ready.store(true, std::sync::atomic::Ordering::Release);

        if let Configured::Unavailable(reason) = self.configure(&mut context).await? {
            return self.give_back(&mut context, &reason).await;
        }

        if !setup::is_finished(&context.workspace).await? {
            let mut progress = Progress::new();
            let scrubber = self.scrubber(&context);
            let readied = setup::run(
                &context.workspace,
                &context.plan,
                &exec_environment(&context.environment),
                &scrubber,
                &mut progress,
            )
            .await;
            for line in &progress {
                context
                    .log
                    .record(&RunnerEvent::AgentMessage { text: line.clone() });
            }
            if let Err(failure) = readied {
                return self.finish(&mut context, false, &failure.0).await;
            }
            self.store_log(&context).await;
        }

        // A report filed just before the last engine stopped is acted on, not asked for again.
        let situation = match self.act_on_report(&mut context).await? {
            Some(Next::Ended(ended)) => return Ok(ended),
            Some(Next::Turn(situation)) => situation,
            None if context.slot.batch.waiting_on.is_some() => match self.wait(&mut context).await?
            {
                Next::Ended(ended) => return Ok(ended),
                Next::Turn(situation) => situation,
            },
            None if context.slot.batch.worker_session_id.is_some() => Situation::AfterInterruption,
            None => Situation::Start,
        };
        self.run_turns(&mut context, situation).await
    }

    /// Runs turns, each in the situation the last left, until the batch ends or the worker is
    /// stopped.
    async fn run_turns(
        &self,
        context: &mut Job,
        mut situation: Situation,
    ) -> anyhow::Result<Ended> {
        let mut strikes = 0;
        loop {
            let outcome = self.turn(context, &situation).await?;
            let next = match outcome.status {
                TurnStatus::Cancelled => return Ok(Ended::Stopped),
                TurnStatus::Interrupted if self.stop.is_cancelled() => return Ok(Ended::Stopped),
                TurnStatus::UsageLimited => {
                    let until = outcome.usage_resets_at;
                    if self.wait_out_usage(until).await {
                        return Ok(Ended::Stopped);
                    }
                    strikes = 0;
                    Next::Turn(Situation::AfterUsageLimit)
                }
                // A turn that went quiet may still have reported before it did.
                TurnStatus::Interrupted | TurnStatus::Completed | TurnStatus::Failed => {
                    match self.act_on_report(context).await? {
                        Some(next) => {
                            strikes = 0;
                            next
                        }
                        None if outcome.status == TurnStatus::Interrupted => {
                            Next::Turn(Situation::AfterInterruption)
                        }
                        None => Next::Turn(Situation::Unreported {
                            error: outcome.error.clone(),
                        }),
                    }
                }
            };
            situation = match next {
                Next::Ended(ended) => return Ok(ended),
                Next::Turn(next) => next,
            };
            if matches!(
                situation,
                Situation::AfterInterruption | Situation::Unreported { .. }
            ) {
                strikes += 1;
                if strikes >= MAX_STRIKES {
                    let why = outcome
                        .error
                        .as_deref()
                        .map_or_else(String::new, |error| format!(" The last said: {error}"));
                    let result = format!(
                        "The worker ended {MAX_STRIKES} turns in a row without reporting on the \
                         batch.{why}"
                    );
                    return self.finish(context, false, &result).await;
                }
            }
        }
    }

    /// Starts the batch if no worker has, and reads what its turns are built from, with the plan
    /// for its setup or why it has none. `None` when the slot was lost.
    async fn begin(&self) -> anyhow::Result<Option<(Job, Result<Plan, String>)>> {
        let mut connection = self.deps.database.get().await?;
        let batch = work_batch::find(&mut connection, self.batch_id).await?;
        let mut slot = Slot {
            batch,
            token: self.token,
        };
        match slot.batch.state {
            BatchState::Assigned => {
                let Some(started) = work_batch::start(&mut connection, &slot).await? else {
                    return Ok(None);
                };
                slot.batch = started;
                announce(&self.deps.events, &slot.batch);
                event!(
                    name: "worker_pool.worker.started",
                    Level::INFO,
                    work_batch.id = %slot.batch.id,
                    project.id = %slot.batch.project_id,
                    "a worker started on its batch",
                );
            }
            BatchState::Running => {}
            _not_in_a_slot => return Ok(None),
        }

        let project = project::find(&mut connection, slot.batch.project_id).await?;
        let declared = worker_setup::find(&mut connection, project.id).await?;
        let plan = setup::plan(&slot.batch.repositories(), &declared);
        let variables = environment_variable::turn_environment(&mut connection, &self.deps.cipher)
            .await
            .context("cannot read the workspace's environment variables")?;
        let workspace_default = github_credential::find_default(&mut connection)
            .await?
            .map(|credential| credential.id);
        let github = github_token::open_for_session(
            &mut connection,
            &self.deps.cipher,
            project.github_credential(workspace_default),
        )
        .await
        .map_err(|error| anyhow::anyhow!("cannot open the project's GitHub token: {error}"))?;
        drop(connection);

        let (environment, secrets) = turn_environment(variables, github.as_ref());
        let workspace = self
            .deps
            .containers
            .workspace(slot.batch.id, self.nano_cpus);
        let runner = Runner::new(
            workspace.clone(),
            self.deps.database.clone(),
            std::sync::Arc::clone(&self.deps.cipher),
            self.deps.activity.clone(),
            self.deps.turn_signal.clone(),
            &self.stop,
        );
        let context = Job {
            slot,
            workspace,
            runner,
            project_name: project.name,
            plan: Plan {
                tools_script: None,
                repositories: Vec::new(),
            },
            environment,
            secrets,
            github_token: github,
            log: TurnLog::default(),
        };
        Ok(Some((context, plan)))
    }

    /// Brings the worker's network and container up, trying [`PREPARE_ATTEMPTS`] times.
    async fn prepare(&self, context: &Job) -> anyhow::Result<()> {
        let mut attempt = 1;
        loop {
            let prepared = async {
                let replicas = self.deps.containers.api_replicas().await?;
                self.deps
                    .containers
                    .ensure_network(self.batch_id, &replicas)
                    .await?;
                context.runner.prepare().await
            };
            match prepared.await {
                Ok(()) => return Ok(()),
                Err(error) if attempt >= PREPARE_ATTEMPTS => return Err(error),
                Err(error) => event!(
                    name: "worker_pool.worker.prepare_retry",
                    Level::WARN,
                    work_batch.id = %self.batch_id,
                    error.message = %format!("{error:#}"),
                    "a worker's container could not start; trying again shortly",
                ),
            }
            attempt += 1;
            tokio::select! {
                () = self.stop.cancelled() => anyhow::bail!("the worker was stopped"),
                () = tokio::time::sleep(PREPARE_BACKOFF) => {}
            }
        }
    }

    /// Puts the worker on the configuration repository. A container that holds the checkout the
    /// batch recorded keeps it; otherwise the repository is fetched with its own token, tried
    /// [`PREPARE_ATTEMPTS`] times, and the checkout recorded on the batch with what was left
    /// out of it. A fetch that fails every time is recorded as the configuration's failure, so
    /// the pool holds every batch back until the engine fetches it again.
    async fn configure(&self, context: &mut Job) -> anyhow::Result<Configured> {
        if let Some(recorded) = context.slot.batch.configuration() {
            let held = context.runner.configuration_commit().await?;
            if held.as_deref() == Some(recorded.commit.as_str()) {
                return Ok(Configured::Ready);
            }
        }
        let mut attempt = 1;
        loop {
            match context.runner.fetch_configuration().await? {
                Fetched::Unset => return Ok(Configured::Unavailable(UNSET_REASON.to_owned())),
                Fetched::Attempted {
                    outcome: Ok(checkout),
                    ..
                } => {
                    self.record_configuration(context, checkout).await?;
                    return Ok(Configured::Ready);
                }
                Fetched::Attempted {
                    source,
                    outcome: Err(error),
                } if attempt >= PREPARE_ATTEMPTS => {
                    self.report_configuration_failure(&source, &error).await;
                    return Ok(Configured::Unavailable(format!(
                        "The configuration repository could not be fetched: {error}"
                    )));
                }
                Fetched::Attempted {
                    outcome: Err(error),
                    ..
                } => event!(
                    name: "worker_pool.worker.configuration_retry",
                    Level::WARN,
                    work_batch.id = %self.batch_id,
                    error.message = %error,
                    "a worker could not fetch the configuration repository; trying again shortly",
                ),
            }
            attempt += 1;
            tokio::select! {
                () = self.stop.cancelled() => anyhow::bail!("the worker was stopped"),
                () = tokio::time::sleep(PREPARE_BACKOFF) => {}
            }
        }
    }

    /// Records `checkout` on the batch, and in its log with everything it left out.
    async fn record_configuration(
        &self,
        context: &mut Job,
        checkout: Checkout,
    ) -> anyhow::Result<()> {
        let mut connection = self.deps.database.get().await?;
        batch_worker::set_configuration(&mut connection, &context.slot, &checkout).await?;
        drop(connection);
        let short: String = checkout.commit.chars().take(12).collect();
        context.log.record(&RunnerEvent::AgentMessage {
            text: format!(
                "Running on the configuration repository {} at {short}: {}",
                checkout.repository, checkout.subject
            ),
        });
        for skipped in &checkout.skipped {
            let detail = skipped
                .detail
                .as_deref()
                .map_or_else(String::new, |detail| format!(" ({detail})"));
            context.log.record(&RunnerEvent::AgentMessage {
                text: format!(
                    "Left out of the configuration: {:?} {}, {:?}{detail}",
                    skipped.kind, skipped.name, skipped.reason
                ),
            });
        }
        self.store_log(context).await;
        context.slot.batch.configuration =
            Some(serde_json::to_value(&checkout).expect("a checkout serializes"));
        announce(&self.deps.events, &context.slot.batch);
        Ok(())
    }

    /// Records that the configuration repository could not be fetched here, which holds the pool
    /// back for every batch, and announces it. A failure to record is logged: the batch still
    /// goes back to the queue, and the engine's next fetch says the same.
    async fn report_configuration_failure(&self, source: &Source, error: &str) {
        let recorded = async {
            let mut connection = self.deps.database.get().await?;
            agent_configuration_status::record_failed(&mut connection, source, error, Utc::now())
                .await?;
            agent_configuration::announce(&mut connection, &self.deps.events).await
        };
        if let Err(recording) = recorded.await {
            event!(
                name: "worker_pool.worker.configuration_report_failure",
                Level::WARN,
                work_batch.id = %self.batch_id,
                error.message = %format!("{recording:#}"),
                "could not record that a worker could not fetch the configuration repository",
            );
        }
    }

    /// Gives the slot back for `reason`, the batch waiting in the queue again, and removes the
    /// container. A batch a turn already worked on cannot wait in the queue, since its work lives
    /// in this container, and one whose workers failed [`MAX_CONFIGURATION_REQUEUES`] times will
    /// not on the next try, so each fails instead, saying why.
    async fn give_back(&self, context: &mut Job, reason: &str) -> anyhow::Result<Ended> {
        if context.slot.batch.worker_session_id.is_some() {
            let result = format!(
                "{reason} The batch had already begun, so it could not wait in the queue for it."
            );
            return self.finish(context, false, &result).await;
        }
        let requeued_before = {
            let mut connection = self.deps.database.get().await?;
            work_batch::history(&mut connection, self.batch_id)
                .await?
                .iter()
                .filter(|entry| entry.kind == EventKind::Requeued.as_str())
                .count()
        };
        if requeued_before + 1 >= MAX_CONFIGURATION_REQUEUES {
            let result = format!(
                "{reason} Its workers could not fetch the configuration repository \
                 {MAX_CONFIGURATION_REQUEUES} times, so the batch ends here rather than waiting \
                 in the queue again."
            );
            return self.finish(context, false, &result).await;
        }
        context.log.record(&RunnerEvent::AgentMessage {
            text: format!("{reason} The batch waits in the queue again."),
        });
        self.store_log(context).await;
        let requeued = {
            let mut connection = self.deps.database.get().await?;
            work_batch::requeue(&mut connection, &context.slot, reason, CODING_AGENT_USER_ID)
                .await?
        };
        let Some(requeued) = requeued else {
            return Ok(Ended::Lost);
        };
        announce(&self.deps.events, &requeued);
        self.deps.events.signal(&Signal::WorkerPool);

        self.ready
            .store(false, std::sync::atomic::Ordering::Release);
        context.runner.shutdown().await;
        if let Err(error) = self.deps.containers.remove(self.batch_id).await {
            // The scheduler's looks remove it later, since no worker runs its batch any more.
            event!(
                name: "worker_pool.worker.remove_failure",
                Level::WARN,
                work_batch.id = %self.batch_id,
                error.message = %format!("{error:#}"),
                "could not remove a requeued worker's container; it is removed on a later look",
            );
        }
        Ok(Ended::Requeued)
    }

    /// Runs one turn in `situation` to its end, logging it as it goes, and keeps the session it
    /// reported.
    async fn turn(&self, context: &mut Job, situation: &Situation) -> anyhow::Result<TurnOutcome> {
        let system_prompt = self.system_prompt(context, situation).await?;
        let batch = &context.slot.batch;
        let grant = self
            .deps
            .grants
            .grant(
                &CallScope::Worker(WorkerScope {
                    batch_id: batch.id,
                    project_id: batch.project_id,
                }),
                &tool_servers(self.deps.passwords.is_ready()),
            )
            .await?;
        let mut mcp_servers = grant.mcp_servers();
        mcp_servers.push(McpServer::Stdio {
            name: BLENDER_SERVER.to_owned(),
            command: BLENDER_COMMAND.to_owned(),
            arguments: Vec::new(),
            environment: Vec::new(),
        });
        mcp_servers.push(desktop::server_at(
            &WorkspacePath::new(DESKTOP_SOCKET).expect("a fixed relative path"),
        ));
        let model = {
            let mut connection = self.deps.database.get().await?;
            // Read as the turn starts, so a change applies from the worker's next turn.
            agent_models::for_agent(&mut connection, Agent::BatchWorker, Some(batch.project_id))
                .await?
                .choice
        };
        let request = TurnRequest {
            conversation: ConversationId::new(batch.id),
            label: format!("worker:{}", batch.id),
            prompt: brief::prompt(),
            working_directory: WorkspacePath::new(setup::BATCH_DIRECTORY)
                .expect("a fixed relative path"),
            resume: batch
                .worker_session_id
                .as_deref()
                .and_then(|session| session.parse::<HarnessSessionId>().ok()),
            mcp_servers,
            allowed_tools: None,
            system_prompt_append: Some(system_prompt),
            environment: context.environment.clone(),
            model: Some(model.model),
            effort: Some(model.effort),
            mode: TurnMode::Agent,
        };
        let mut ticket = context
            .runner
            .submit(request)
            .map_err(|_shutting_down| anyhow::anyhow!("the worker is stopping"))?;

        let scrubber = self.scrubber(context);
        let mut last_flush = Instant::now();
        let mut outcome = None;
        while let Some(runner_event) = ticket.events.recv().await {
            let runner_event = scrubber.scrub(runner_event);
            if let RunnerEvent::Finished(finished) = runner_event {
                outcome = Some(finished);
                break;
            }
            context.log.record(&runner_event);
            if last_flush.elapsed() >= LOG_FLUSH_INTERVAL {
                self.store_log(context).await;
                last_flush = Instant::now();
            }
        }
        grant.revoke().await;
        let outcome = outcome.unwrap_or_else(|| {
            TurnOutcome::without_result(
                TurnStatus::Interrupted,
                "the turn ended without reporting how",
            )
        });
        if let Some(error) = &outcome.error {
            context.log.record(&RunnerEvent::AgentMessage {
                text: format!("The turn ended {:?}: {error}", outcome.status),
            });
        }
        self.store_log(context).await;

        if let Some(session) = &outcome.harness_session_id {
            let mut connection = self.deps.database.get().await?;
            batch_worker::set_session(&mut connection, &context.slot, session.as_str()).await?;
            context.slot.batch.worker_session_id = Some(session.as_str().to_owned());
        }
        Ok(outcome)
    }

    /// The system prompt a turn in `situation` appends (`super::brief`), with where each pull request
    /// the worker recorded stands, as the last look left it. Who merges is read again every turn,
    /// for the plan's repositories too, so a change to the project's setup reaches the next turn.
    async fn system_prompt(
        &self,
        context: &mut Job,
        situation: &Situation,
    ) -> anyhow::Result<String> {
        let batch = &context.slot.batch;
        let mut connection = self.deps.database.get().await?;
        let declared = worker_setup::find(&mut connection, batch.project_id).await?;
        for repository in &mut context.plan.repositories {
            repository.merge_policy = merging::policy_for(&declared, &repository.url);
        }
        let pull_requests: Vec<PullRequestStanding> =
            pull_request::of_batch(&mut connection, batch.id)
                .await?
                .into_iter()
                .map(|pull_request| PullRequestStanding {
                    policy: merging::policy_for(&declared, &pull_request.github_url()),
                    pull_request,
                })
                .collect();
        let action_items = batch.action_items();
        Ok(brief::system_prompt(
            &Batch {
                id: batch.id,
                title: &batch.title,
                goal: &batch.goal,
                project_id: batch.project_id,
                project_name: &context.project_name,
                repositories: &context.plan.repositories,
                action_item_ids: &action_items,
                pull_requests: &pull_requests,
            },
            situation,
        ))
    }

    /// Acts on the report the worker filed, if any: finishes the batch, or waits. `None` when it
    /// filed none.
    async fn act_on_report(&self, context: &mut Job) -> anyhow::Result<Option<Next>> {
        let report = {
            let mut connection = self.deps.database.get().await?;
            batch_worker::take_report(&mut connection, &context.slot).await?
        };
        match report {
            None => Ok(None),
            Some(Report::Finished { succeeded, result }) => {
                // The last look, while the batch still runs: a pull request ready to merge
                // merges, unless the worker said the batch cannot be done, and one a person
                // merged meanwhile is recorded.
                let intent = if succeeded {
                    Intent::Merge
                } else {
                    Intent::RecordOnly
                };
                self.deps
                    .merging
                    .look(&context.slot.batch, context.github_token.as_ref(), intent)
                    .await;
                Ok(Some(Next::Ended(
                    self.finish(context, succeeded, &result).await?,
                )))
            }
            Some(Report::Waiting { on, detail, until }) => {
                let waiting = {
                    let mut connection = self.deps.database.get().await?;
                    batch_worker::wait(&mut connection, &context.slot, on, &detail, until).await?
                };
                let Some(waiting) = waiting else {
                    return Ok(Some(Next::Ended(Ended::Lost)));
                };
                context.slot.batch = waiting;
                announce(&self.deps.events, &context.slot.batch);
                if on == batch_worker::Wait::Question {
                    self.deps.events.signal(&Signal::ProjectManagers);
                }
                event!(
                    name: "worker_pool.worker.waiting",
                    Level::INFO,
                    work_batch.id = %self.batch_id,
                    work_batch.waiting_on = ?on,
                    "a worker waits, keeping its container",
                );
                Ok(Some(self.wait(context).await?))
            }
        }
    }

    /// Waits until the worker's wait is over (its time comes, or its question is answered),
    /// then ends it. A signal for the pool, which an answer sends, makes it read the wait again.
    async fn wait(&self, context: &mut Job) -> anyhow::Result<Next> {
        let mut wake = self
            .deps
            .events
            .wake_on(|signal| *signal == Signal::WorkerPool);
        loop {
            let batch = {
                let mut connection = self.deps.database.get().await?;
                work_batch::find(&mut connection, self.batch_id).await?
            };
            if batch.state != BatchState::Running {
                return Ok(Next::Ended(Ended::Lost));
            }
            let (Some(on), Some(detail), Some(until)) = (
                batch.waiting_on,
                batch.waiting_detail.clone(),
                batch.waiting_until,
            ) else {
                context.slot.batch = batch;
                return Ok(Next::Turn(Situation::AfterInterruption));
            };
            let now = Utc::now();
            if until <= now {
                // Before the turn that follows, so it reads where its pull requests stand.
                self.deps
                    .merging
                    .look(&batch, context.github_token.as_ref(), Intent::Merge)
                    .await;
                let answers = {
                    let mut connection = self.deps.database.get().await?;
                    let answers =
                        batch_worker::answers_since_waiting(&mut connection, self.batch_id).await?;
                    let Some(resumed) =
                        batch_worker::stop_waiting(&mut connection, &context.slot).await?
                    else {
                        return Ok(Next::Ended(Ended::Lost));
                    };
                    context.slot.batch = resumed;
                    answers
                };
                announce(&self.deps.events, &context.slot.batch);
                return Ok(Next::Turn(Situation::AfterWait {
                    on,
                    detail,
                    answers,
                }));
            }
            let until_due = (until - now).to_std().unwrap_or(Duration::ZERO);
            tokio::select! {
                () = self.stop.cancelled() => return Ok(Next::Ended(Ended::Stopped)),
                () = wake.woken() => {}
                () = tokio::time::sleep(until_due.min(WAIT_LOOK_INTERVAL)) => {}
            }
        }
    }

    /// Waits for a usage limit to reset at `until`, or [`USAGE_RETRY`] when no reset is known.
    /// `true` when the worker was stopped meanwhile.
    async fn wait_out_usage(&self, until: Option<chrono::DateTime<Utc>>) -> bool {
        let wait = until
            .and_then(|until| (until - Utc::now()).to_std().ok())
            .unwrap_or(USAGE_RETRY);
        event!(
            name: "worker_pool.worker.usage_limited",
            Level::INFO,
            work_batch.id = %self.batch_id,
            "every Claude credential's usage is spent; the worker waits for a reset",
        );
        tokio::select! {
            () = self.stop.cancelled() => true,
            () = tokio::time::sleep(wait) => false,
        }
    }

    /// Ends the batch with `result`, gives its slot back, and removes its container, volume,
    /// and network.
    async fn finish(
        &self,
        context: &mut Job,
        succeeded: bool,
        result: &str,
    ) -> anyhow::Result<Ended> {
        let result: String = result.trim().chars().take(RESULT_MAX_CHARACTERS).collect();
        let result = if result.is_empty() {
            "The worker reported nothing.".to_owned()
        } else {
            result
        };
        self.store_log(context).await;
        let finished = {
            let mut connection = self.deps.database.get().await?;
            work_batch::finish(
                &mut connection,
                &context.slot,
                succeeded,
                &result,
                CODING_AGENT_USER_ID,
            )
            .await?
        };
        let Some(finished) = finished else {
            return Ok(Ended::Lost);
        };
        announce(&self.deps.events, &finished);
        self.deps.events.signal(&Signal::WorkerPool);
        self.deps.events.signal(&Signal::ProjectManagers);
        event!(
            name: "worker_pool.worker.finished",
            Level::INFO,
            work_batch.id = %finished.id,
            work_batch.succeeded = succeeded,
            "a worker finished its batch",
        );

        self.ready
            .store(false, std::sync::atomic::Ordering::Release);
        context.runner.shutdown().await;
        if let Err(error) = self.deps.containers.remove(self.batch_id).await {
            // The scheduler's looks remove it later, since no worker runs its batch any more.
            event!(
                name: "worker_pool.worker.remove_failure",
                Level::WARN,
                work_batch.id = %self.batch_id,
                error.message = %format!("{error:#}"),
                "could not remove a finished worker's container; it is removed on a later look",
            );
        }
        Ok(Ended::Finished)
    }

    /// Stores the worker's log, scrubbed already. A failure is logged: the log is a trail to
    /// follow, and the next store carries what this one missed.
    async fn store_log(&self, context: &Job) {
        let stored = async {
            let mut connection = self.deps.database.get().await?;
            anyhow::Ok(
                batch_worker::set_log(&mut connection, &context.slot, context.log.as_str()).await?,
            )
        };
        if let Err(error) = stored.await {
            event!(
                name: "worker_pool.worker.log_failure",
                Level::WARN,
                work_batch.id = %self.batch_id,
                error.message = %format!("{error:#}"),
                "could not store a worker's log",
            );
        }
    }

    /// What scrubs the worker's output: its secrets, and every password the vault handed out.
    fn scrubber(&self, context: &Job) -> Scrubber {
        let secrets = context
            .secrets
            .iter()
            .map(|secret| SecretString::from(secret.expose_secret().to_owned()));
        Scrubber::new(secrets).with_revealed(self.deps.passwords.revealed())
    }
}

/// What follows a report, or a wait.
enum Next {
    Ended(Ended),
    Turn(Situation),
}

/// A turn's environment as an exec takes it, for setup's scripts.
fn exec_environment(environment: &[EnvironmentEntry]) -> Vec<String> {
    environment
        .iter()
        .map(|entry| format!("{}={}", entry.key, entry.value.expose_secret()))
        .collect()
}

/// The tool servers a worker's turn may call: its own, the knowledge base's, and the password
/// vault's while it is up. Not the work tools, whose writes are recorded as a session's or a
/// Project Manager's, and not storage, whose tools move files in the shared workspace.
fn tool_servers(has_passwords: bool) -> Vec<&'static ToolServer> {
    let mut servers = vec![&tools::worker::SERVER, &tools::knowledge::SERVER];
    if has_passwords {
        servers.push(&tools::passwords::SERVER);
    }
    servers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worker_reports_and_reads_and_signs_in_while_the_vault_is_up() {
        let names = |has_passwords| -> Vec<&'static str> {
            tool_servers(has_passwords)
                .iter()
                .map(|server| server.name)
                .collect()
        };
        assert_eq!(
            names(true),
            ["elysium_worker", "elysium_knowledge", "elysium_passwords"]
        );
        assert_eq!(names(false), ["elysium_worker", "elysium_knowledge"]);
    }
}
