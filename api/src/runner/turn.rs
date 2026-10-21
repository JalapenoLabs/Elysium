// Copyright © 2026 Jalapeno Labs

//! One turn, from the queue to its [`RunnerEvent::Finished`].
//!
//! A turn waits for a slot, prepares the workspace if this is the first turn since the API
//! started, then runs attempts: one `claude` process each, on one credential. An attempt that
//! ends because its credential is spent or refused hands the turn to the next credential,
//! which resumes the same Claude session with [`CONTINUE_PROMPT`], so the work done so far is
//! kept. With no session to resume (the CLI failed before it began one), the next attempt runs
//! the original prompt. Any other failure ends the turn at once: another credential would
//! fail the same way.
//!
//! While an attempt runs, each line it prints resets its heartbeat. Silence for
//! [`HEARTBEAT_TIMEOUT`], a cancellation, or shutdown kills the attempt's process group.

use std::pin::pin;

use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};
use tracing::{Level, event};

use crate::activity::ActivityKind;

use super::command::{AttemptCommand, KILL_SCRIPT};
use super::credentials::ClaudeCredential;
use super::events::{AttemptFailure, AttemptReport, StreamParser};
use super::workspace::{AGENT_USER, ExecCommand, SCRIPT_TIMEOUT};
use super::{
    Executor, HEARTBEAT_TIMEOUT, QueuedTurn, Runner, RunnerEvent, TURNS_DIRECTORY, TurnOutcome,
    TurnStatus,
};

/// What a resumed attempt is asked after the previous credential gave out mid-turn.
const CONTINUE_PROMPT: &str = "Your previous attempt at this turn stopped because its model \
     credential ran out. Continue exactly where you left off.";

/// How an attempt ended.
enum AttemptEnd {
    Reported(AttemptReport),
    Cancelled,
    /// Silent for longer than [`HEARTBEAT_TIMEOUT`].
    Silent,
    /// The process could not be started or followed.
    Lost(String),
}

/// Runs `queued` to its end and sends its events. Never fails: every problem becomes the
/// turn's outcome.
pub(super) async fn run(runner: &Runner, queued: QueuedTurn) {
    let QueuedTurn {
        turn_id,
        request,
        events,
        cancellation,
    } = queued;
    let inner = &runner.inner;

    let outcome = 'turn: {
        let permit = tokio::select! {
            permit = inner.slots.acquire() => permit,
            () = cancellation.cancelled() => break 'turn stopped(inner.shutdown.is_cancelled()),
        };
        let Ok(_permit) = permit else {
            break 'turn stopped(true);
        };
        // Held until the turn ends, however it ends, so auto-update waits for it.
        let _activity = inner
            .activity
            .begin(ActivityKind::AgentTurn, request.label.clone());

        send(
            &events,
            RunnerEvent::TurnStarted {
                turn_id,
                prompt: request.prompt.clone(),
            },
        )
        .await;
        event!(
            name: "runner.turn.started",
            Level::INFO,
            turn.id = %turn_id,
            turn.label = %request.label,
            conversation.id = %request.conversation,
            "turn started",
        );

        if let Err(error) = runner.prepare().await {
            break 'turn TurnOutcome::without_result(
                TurnStatus::Failed,
                format!("the workspace is not available: {error:#}"),
            );
        }
        let credentials = match inner.credentials.load().await {
            Ok(credentials) => credentials,
            Err(error) => {
                break 'turn TurnOutcome::without_result(
                    TurnStatus::Failed,
                    format!("could not read the model credentials: {error:#}"),
                );
            }
        };
        if credentials.is_empty() {
            event!(
                name: "runner.turn.no_credential",
                Level::WARN,
                turn.id = %turn_id,
                "no usable Claude credential; the turn runs on whatever login the workspace holds",
            );
        }

        run_attempts(
            inner,
            turn_id,
            &request,
            &credentials,
            &events,
            &cancellation,
        )
        .await
    };

    event!(
        name: "runner.turn.finished",
        Level::INFO,
        turn.id = %turn_id,
        turn.label = %request.label,
        turn.status = ?outcome.status,
        error.message = outcome.error.as_deref().unwrap_or(""),
        "turn finished",
    );
    send(&events, RunnerEvent::Finished(outcome)).await;
}

async fn run_attempts(
    inner: &super::Inner,
    turn_id: uuid::Uuid,
    request: &super::TurnRequest,
    credentials: &[ClaudeCredential],
    events: &mpsc::Sender<RunnerEvent>,
    cancellation: &tokio_util::sync::CancellationToken,
) -> TurnOutcome {
    // With no credential, one attempt runs on the workspace's own login, if it has one.
    let attempts: Vec<Option<&ClaudeCredential>> = if credentials.is_empty() {
        vec![None]
    } else {
        credentials.iter().map(Some).collect()
    };

    let mut prompt = request.prompt.clone();
    let mut resume = request.resume.clone();
    let mut totals = Totals::default();

    for (index, credential) in attempts.iter().enumerate() {
        let command = AttemptCommand {
            turn_directory: format!("{TURNS_DIRECTORY}/{turn_id}-{index}"),
            working_directory: request.working_directory.absolute(),
            prompt: prompt.clone(),
            resume: resume.clone(),
            model: request.model.clone(),
            allowed_tools: request.allowed_tools.clone(),
            system_prompt_append: request.system_prompt_append.clone(),
            mcp_servers: request.mcp_servers.clone(),
            credential: credential.map(|credential| credential.secret.clone()),
            environment: request.environment.clone(),
        };

        let report = match run_attempt(&inner.executor, command, events, cancellation).await {
            AttemptEnd::Reported(report) => report,
            AttemptEnd::Cancelled => {
                return totals.outcome(stopped(inner.shutdown.is_cancelled()), resume);
            }
            AttemptEnd::Silent => {
                let error = format!(
                    "the agent printed nothing for {} minutes and was stopped",
                    HEARTBEAT_TIMEOUT.as_secs() / 60
                );
                return totals.outcome(
                    TurnOutcome::without_result(TurnStatus::Interrupted, error),
                    resume,
                );
            }
            AttemptEnd::Lost(error) => {
                return totals.outcome(
                    TurnOutcome::without_result(TurnStatus::Failed, error),
                    resume,
                );
            }
        };

        totals.add(&report);
        // An attempt that reported a session ran, so the prompt is already in it.
        let ran = report.harness_session_id.is_some();
        if ran {
            resume.clone_from(&report.harness_session_id);
        }
        let next = attempts.get(index + 1).copied().flatten();
        let (from, to, reason) = match decide(&report, *credential, next) {
            Decision::Finish(outcome) => return totals.outcome(outcome, resume),
            Decision::RollOver { from, to, reason } => (from, to, reason),
        };

        event!(
            name: "runner.credential.rolled_over",
            Level::WARN,
            turn.id = %turn_id,
            llm.from = %from.name,
            llm.to = %to.name,
            error.message = %reason,
            "a credential gave out; the next one takes the turn over",
        );
        send(
            events,
            RunnerEvent::CredentialRolledOver {
                from: from.id,
                to: to.id,
                reason,
            },
        )
        .await;
        // After an attempt that ran, the next credential carries on from where it stopped;
        // after one that never started, it starts the original prompt over.
        if ran {
            CONTINUE_PROMPT.clone_into(&mut prompt);
        }
    }

    unreachable!("the last attempt always finishes the turn")
}

/// What follows an attempt that reported.
enum Decision<'credentials> {
    Finish(TurnOutcome),
    RollOver {
        from: &'credentials ClaudeCredential,
        to: &'credentials ClaudeCredential,
        reason: String,
    },
}

/// Finishes the turn, unless the attempt's credential gave out and `next` can take over.
fn decide<'credentials>(
    report: &AttemptReport,
    current: Option<&'credentials ClaudeCredential>,
    next: Option<&'credentials ClaudeCredential>,
) -> Decision<'credentials> {
    let finished = |status: TurnStatus, error: Option<String>| TurnOutcome {
        status,
        result: report.result.clone(),
        error,
        harness_session_id: None,
        cost_usd_micros: None,
        input_tokens: None,
        output_tokens: None,
    };
    let Some(failure) = &report.failure else {
        return Decision::Finish(finished(TurnStatus::Completed, None));
    };
    let reason = failure.reason().to_owned();
    if !failure.rolls_over() {
        return Decision::Finish(finished(TurnStatus::Failed, Some(reason)));
    }
    if let (Some(from), Some(to)) = (current, next) {
        return Decision::RollOver { from, to, reason };
    }
    // Out of credentials: spent ones may come back, a refused one needs fixing.
    let status = match failure {
        AttemptFailure::SpentCredential(_) => TurnStatus::UsageLimited,
        AttemptFailure::RejectedCredential(_) | AttemptFailure::Other(_) => TurnStatus::Failed,
    };
    Decision::Finish(finished(status, Some(reason)))
}

/// Runs one attempt and forwards its events as they arrive.
async fn run_attempt(
    executor: &Executor,
    command: AttemptCommand,
    events: &mpsc::Sender<RunnerEvent>,
    cancellation: &tokio_util::sync::CancellationToken,
) -> AttemptEnd {
    let turn_directory = command.turn_directory.clone();
    let (line_sender, mut lines) = mpsc::channel::<String>(256);
    let mut execution = pin!(executor.run_attempt(command, line_sender));
    let mut parser = StreamParser::new();
    let mut deadline = Instant::now() + HEARTBEAT_TIMEOUT;
    let mut stopping: Option<AttemptEnd> = None;

    let exit = loop {
        tokio::select! {
            exit = &mut execution => break exit,
            Some(line) = lines.recv() => {
                deadline = Instant::now() + HEARTBEAT_TIMEOUT;
                for parsed in parser.parse_line(&line) {
                    send(events, parsed).await;
                }
            }
            () = cancellation.cancelled(), if stopping.is_none() => {
                stopping = Some(AttemptEnd::Cancelled);
                executor.kill(&turn_directory).await;
            }
            () = sleep_until(deadline), if stopping.is_none() => {
                stopping = Some(AttemptEnd::Silent);
                executor.kill(&turn_directory).await;
            }
        }
    };
    // The process has ended, so whatever it printed is already buffered.
    while let Ok(line) = lines.try_recv() {
        for parsed in parser.parse_line(&line) {
            send(events, parsed).await;
        }
    }

    if let Some(stopped) = stopping {
        return stopped;
    }
    match exit {
        Ok(exit_code) => AttemptEnd::Reported(parser.finish(exit_code)),
        Err(error) => AttemptEnd::Lost(format!("lost the agent's process: {error}")),
    }
}

impl Executor {
    async fn run_attempt(
        &self,
        command: AttemptCommand,
        lines: mpsc::Sender<String>,
    ) -> Result<Option<i64>, String> {
        match self {
            Self::Workspace(workspace) => {
                let exec = ExecCommand {
                    argv: command.argv(),
                    environment: command.environment(),
                    user: AGENT_USER,
                };
                workspace
                    .exec(exec, lines)
                    .await
                    .map_err(|error| error.to_string())
            }
            #[cfg(test)]
            Self::Fake(fake) => fake.run_attempt(&command, lines).await,
        }
    }

    async fn kill(&self, turn_directory: &str) {
        match self {
            Self::Workspace(workspace) => {
                let killed = workspace
                    .run_script(
                        AGENT_USER,
                        KILL_SCRIPT,
                        &[turn_directory],
                        Vec::new(),
                        SCRIPT_TIMEOUT,
                    )
                    .await;
                if let Err(error) = killed {
                    event!(
                        name: "runner.turn.kill_failed",
                        Level::ERROR,
                        turn.directory = turn_directory,
                        error.message = %error,
                        "could not stop a turn's process; it may still be running in the workspace",
                    );
                }
            }
            #[cfg(test)]
            Self::Fake(fake) => fake.kill(),
        }
    }
}

/// Usage summed over a turn's attempts.
#[derive(Debug, Default)]
struct Totals {
    cost_usd_micros: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
}

impl Totals {
    fn add(&mut self, report: &AttemptReport) {
        add_to(&mut self.cost_usd_micros, report.cost_usd_micros);
        add_to(&mut self.input_tokens, report.input_tokens);
        add_to(&mut self.output_tokens, report.output_tokens);
    }

    /// `outcome` with the turn's totals and the session to resume next time.
    fn outcome(
        self,
        mut outcome: TurnOutcome,
        resume: Option<super::HarnessSessionId>,
    ) -> TurnOutcome {
        outcome.cost_usd_micros = self.cost_usd_micros;
        outcome.input_tokens = self.input_tokens;
        outcome.output_tokens = self.output_tokens;
        outcome.harness_session_id = resume;
        outcome
    }
}

fn add_to(total: &mut Option<i64>, value: Option<i64>) {
    if let Some(value) = value {
        *total = Some(total.unwrap_or(0).saturating_add(value));
    }
}

/// A turn stopped before its end: by shutdown, or by a person.
fn stopped(by_shutdown: bool) -> TurnOutcome {
    if by_shutdown {
        return TurnOutcome::without_result(TurnStatus::Interrupted, "Elysium is shutting down");
    }
    TurnOutcome::without_result(TurnStatus::Cancelled, "the turn was cancelled")
}

/// Sends `event` to the turn's reader. A reader that went away is not a reason to stop the
/// turn, so a closed channel is only noted.
async fn send(events: &mpsc::Sender<RunnerEvent>, event: RunnerEvent) {
    if events.send(event).await.is_err() {
        event!(
            name: "runner.turn.unheard",
            Level::DEBUG,
            "a turn's reader stopped listening; the turn carries on",
        );
    }
}

#[cfg(test)]
pub(super) mod fake {
    //! A scripted workspace: each attempt prints the next script's lines and exits with its
    //! status, and every command it was asked to run is recorded.

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use tokio::sync::{Notify, mpsc};

    use super::super::command::AttemptCommand;
    use super::super::lock;

    /// One attempt's output.
    #[derive(Debug, Clone)]
    pub struct Script {
        pub lines: Vec<String>,
        pub exit_code: i64,
        /// Hold the process open after printing, until killed.
        pub hangs: bool,
    }

    impl Script {
        pub fn printing(lines: &[&str], exit_code: i64) -> Self {
            Self {
                lines: lines.iter().map(|line| (*line).to_owned()).collect(),
                exit_code,
                hangs: false,
            }
        }

        pub fn hanging() -> Self {
            Self {
                lines: Vec::new(),
                exit_code: 143,
                hangs: true,
            }
        }
    }

    #[derive(Debug, Clone, Default)]
    pub struct FakeWorkspace {
        scripts: Arc<Mutex<VecDeque<Script>>>,
        pub commands: Arc<Mutex<Vec<AttemptCommand>>>,
        killed: Arc<Notify>,
        pub fail_preparation: bool,
    }

    impl FakeWorkspace {
        pub fn with_scripts(scripts: Vec<Script>) -> Self {
            Self {
                scripts: Arc::new(Mutex::new(scripts.into())),
                ..Self::default()
            }
        }

        pub fn prepare(&self) -> anyhow::Result<()> {
            if self.fail_preparation {
                anyhow::bail!("the workspace image could not be pulled");
            }
            Ok(())
        }

        pub async fn run_attempt(
            &self,
            command: &AttemptCommand,
            lines: mpsc::Sender<String>,
        ) -> Result<Option<i64>, String> {
            lock(&self.commands).push(command.clone());
            let script = lock(&self.scripts)
                .pop_front()
                .ok_or_else(|| "no script left for this attempt".to_owned())?;
            for line in script.lines {
                let _unheard = lines.send(line).await;
            }
            if script.hangs {
                self.killed.notified().await;
            }
            Ok(Some(script.exit_code))
        }

        pub fn kill(&self) {
            self.killed.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::super::command::CredentialSecret;
    use super::super::configuration::ConfigurationSource;
    use super::super::{
        ConversationId, CredentialSource, TurnRequest, TurnTicket, WorkspacePath, lock,
    };
    use super::fake::{FakeWorkspace, Script};
    use super::*;

    const SPENT: &str = r#"{"type":"result","subtype":"success","is_error":true,"result":"Claude AI usage limit reached","session_id":"s-1","total_cost_usd":0.25}"#;
    const DONE: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"All done.","session_id":"s-1","total_cost_usd":0.5,"usage":{"input_tokens":10,"output_tokens":4}}"#;

    fn credential(name: &str) -> ClaudeCredential {
        ClaudeCredential {
            id: Uuid::now_v7(),
            name: name.to_owned(),
            secret: CredentialSecret::SubscriptionToken(SecretString::from(format!(
                "token-{name}"
            ))),
        }
    }

    fn runner(fake: FakeWorkspace, credentials: Vec<ClaudeCredential>) -> Runner {
        Runner::with_parts(
            Executor::Fake(fake),
            CredentialSource::Fixed(credentials),
            ConfigurationSource::None,
            crate::activity::Activities::new(),
            tokio_util::sync::CancellationToken::new(),
        )
    }

    fn request(conversation: ConversationId, prompt: &str) -> TurnRequest {
        TurnRequest {
            conversation,
            label: "test".to_owned(),
            prompt: prompt.to_owned(),
            working_directory: WorkspacePath::new("sessions/1").expect("a path"),
            resume: None,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: None,
        }
    }

    async fn collect(mut ticket: TurnTicket) -> (Vec<RunnerEvent>, TurnOutcome) {
        let mut events = Vec::new();
        while let Some(event) = ticket.events.recv().await {
            if let RunnerEvent::Finished(outcome) = event {
                assert!(ticket.events.recv().await.is_none(), "Finished is last");
                return (events, outcome);
            }
            events.push(event);
        }
        panic!("the turn ended without Finished");
    }

    fn token_of(command: &AttemptCommand) -> Option<String> {
        match &command.credential {
            Some(CredentialSecret::SubscriptionToken(token)) => {
                Some(secrecy::ExposeSecret::expose_secret(token).to_owned())
            }
            _ => None,
        }
    }

    #[tokio::test]
    async fn a_turn_reports_its_events_then_its_outcome() {
        let fake = FakeWorkspace::with_scripts(vec![Script::printing(
            &[
                r#"{"type":"system","subtype":"init","session_id":"s-1"}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#,
                DONE,
            ],
            0,
        )]);
        let runner = runner(fake, vec![credential("first")]);
        let ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Say hello"))
            .expect("accepted");
        let (events, outcome) = collect(ticket).await;

        assert!(
            matches!(&events[0], RunnerEvent::TurnStarted { prompt, .. } if prompt == "Say hello")
        );
        assert!(matches!(&events[1], RunnerEvent::AgentMessage { text } if text == "Hello"));
        assert_eq!(outcome.status, TurnStatus::Completed);
        assert_eq!(outcome.result.as_deref(), Some("All done."));
        assert_eq!(outcome.cost_usd_micros, Some(500_000));
        assert_eq!(
            outcome.harness_session_id.as_ref().map(ToString::to_string),
            Some("s-1".to_owned())
        );
    }

    #[tokio::test]
    async fn a_spent_credential_hands_the_turn_to_the_next_which_resumes_the_session() {
        let fake = FakeWorkspace::with_scripts(vec![
            Script::printing(
                &[
                    r#"{"type":"system","subtype":"init","session_id":"s-1"}"#,
                    SPENT,
                ],
                1,
            ),
            Script::printing(&[DONE], 0),
        ]);
        let first = credential("first");
        let second = credential("second");
        let runner = runner(fake.clone(), vec![first.clone(), second.clone()]);
        let ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Build it"))
            .expect("accepted");
        let (events, outcome) = collect(ticket).await;

        assert!(events.iter().any(|event| matches!(
            event,
            RunnerEvent::CredentialRolledOver { from, to, .. } if *from == first.id && *to == second.id
        )));
        assert_eq!(outcome.status, TurnStatus::Completed);
        assert_eq!(
            outcome.cost_usd_micros,
            Some(750_000),
            "both attempts are billed"
        );

        let commands = lock(&fake.commands).clone();
        assert_eq!(token_of(&commands[0]).as_deref(), Some("token-first"));
        assert_eq!(token_of(&commands[1]).as_deref(), Some("token-second"));
        assert_eq!(commands[1].prompt, CONTINUE_PROMPT);
        assert_eq!(
            commands[1].resume.as_ref().map(ToString::to_string),
            Some("s-1".to_owned())
        );
    }

    #[tokio::test]
    async fn when_every_credential_is_spent_the_turn_is_usage_limited() {
        let fake = FakeWorkspace::with_scripts(vec![
            Script::printing(&[SPENT], 1),
            Script::printing(&[SPENT], 1),
        ]);
        let runner = runner(fake, vec![credential("first"), credential("second")]);
        let ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Build it"))
            .expect("accepted");
        let (_events, outcome) = collect(ticket).await;
        assert_eq!(outcome.status, TurnStatus::UsageLimited);
        assert_eq!(
            outcome.harness_session_id.as_ref().map(ToString::to_string),
            Some("s-1".to_owned()),
            "a later turn can resume once a credential is back"
        );
    }

    #[tokio::test]
    async fn a_refused_last_credential_fails_rather_than_waiting_on_a_limit() {
        let fake = FakeWorkspace::with_scripts(vec![Script::printing(
            &[
                r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#,
            ],
            1,
        )]);
        let runner = runner(fake, Vec::new());
        let ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Anything"))
            .expect("accepted");
        let (_events, outcome) = collect(ticket).await;
        assert_eq!(outcome.status, TurnStatus::Failed);
        assert_eq!(
            outcome.error.as_deref(),
            Some("Not logged in · Please run /login")
        );
    }

    #[tokio::test]
    async fn an_attempt_that_never_began_a_session_is_retried_with_the_original_prompt() {
        let fake = FakeWorkspace::with_scripts(vec![
            Script::printing(
                &[
                    r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key"}"#,
                ],
                1,
            ),
            Script::printing(&[DONE], 0),
        ]);
        let runner = runner(
            fake.clone(),
            vec![credential("first"), credential("second")],
        );
        let ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Build it"))
            .expect("accepted");
        let (_events, outcome) = collect(ticket).await;
        assert_eq!(outcome.status, TurnStatus::Completed);
        let commands = lock(&fake.commands).clone();
        assert_eq!(commands[1].prompt, "Build it");
        assert!(commands[1].resume.is_none());
    }

    #[tokio::test]
    async fn an_ordinary_failure_does_not_spend_another_credential() {
        let fake = FakeWorkspace::with_scripts(vec![Script::printing(
            &[r#"{"type":"result","subtype":"error_during_execution","result":"The build broke"}"#],
            1,
        )]);
        let runner = runner(
            fake.clone(),
            vec![credential("first"), credential("second")],
        );
        let ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Build it"))
            .expect("accepted");
        let (_events, outcome) = collect(ticket).await;
        assert_eq!(outcome.status, TurnStatus::Failed);
        assert_eq!(outcome.error.as_deref(), Some("The build broke"));
        assert_eq!(lock(&fake.commands).len(), 1);
    }

    #[tokio::test]
    async fn turns_of_one_conversation_run_in_order() {
        let fake = FakeWorkspace::with_scripts(vec![
            Script::printing(&[DONE], 0),
            Script::printing(&[DONE], 0),
        ]);
        let runner = runner(fake.clone(), vec![credential("first")]);
        let conversation = ConversationId::new(Uuid::now_v7());
        let first = runner
            .submit(request(conversation, "one"))
            .expect("accepted");
        let second = runner
            .submit(request(conversation, "two"))
            .expect("accepted");
        let (_events, first_outcome) = collect(first).await;
        let (_events, second_outcome) = collect(second).await;
        assert_eq!(first_outcome.status, TurnStatus::Completed);
        assert_eq!(second_outcome.status, TurnStatus::Completed);
        let prompts: Vec<String> = lock(&fake.commands)
            .iter()
            .map(|command| command.prompt.clone())
            .collect();
        assert_eq!(prompts, ["one", "two"]);
    }

    #[tokio::test]
    async fn cancelling_a_running_turn_kills_it_and_says_so() {
        let fake = FakeWorkspace::with_scripts(vec![Script::hanging()]);
        let runner = runner(fake, vec![credential("first")]);
        let mut ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Wait"))
            .expect("accepted");
        let started = ticket.events.recv().await.expect("started");
        assert!(matches!(started, RunnerEvent::TurnStarted { .. }));
        assert!(runner.cancel(ticket.turn_id));
        let (_events, outcome) = collect(ticket).await;
        assert_eq!(outcome.status, TurnStatus::Cancelled);
    }

    #[tokio::test]
    async fn a_running_turn_keeps_the_stack_busy_until_it_ends() {
        let activity = crate::activity::Activities::new();
        let runner = Runner::with_parts(
            Executor::Fake(FakeWorkspace::with_scripts(vec![Script::hanging()])),
            CredentialSource::Fixed(vec![credential("first")]),
            ConfigurationSource::None,
            activity.clone(),
            tokio_util::sync::CancellationToken::new(),
        );
        assert!(activity.is_idle());
        let mut ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Wait"))
            .expect("accepted");
        ticket.events.recv().await.expect("started");
        assert!(!activity.is_idle(), "auto-update must wait for the turn");
        runner.cancel(ticket.turn_id);
        collect(ticket).await;
        assert!(activity.is_idle());
    }

    #[tokio::test]
    async fn shutdown_interrupts_running_turns_and_refuses_new_ones() {
        let fake = FakeWorkspace::with_scripts(vec![Script::hanging()]);
        let runner = runner(fake, vec![credential("first")]);
        let mut ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Wait"))
            .expect("accepted");
        ticket.events.recv().await.expect("started");
        let stopping = runner.clone();
        let shutdown = tokio::spawn(async move { stopping.shutdown().await });
        let (_events, outcome) = collect(ticket).await;
        assert_eq!(outcome.status, TurnStatus::Interrupted);
        shutdown.await.expect("shutdown finishes");
        let refused = runner.submit(request(ConversationId::new(Uuid::now_v7()), "Later"));
        assert!(matches!(refused, Err(super::super::ShuttingDown)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_turn_is_stopped_after_the_heartbeat() {
        let fake = FakeWorkspace::with_scripts(vec![Script::hanging()]);
        let runner = runner(fake, vec![credential("first")]);
        let ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Hang"))
            .expect("accepted");
        let (_events, outcome) = collect(ticket).await;
        assert_eq!(outcome.status, TurnStatus::Interrupted);
        assert!(
            outcome
                .error
                .unwrap_or_default()
                .contains("printed nothing")
        );
    }

    #[tokio::test]
    async fn a_workspace_that_cannot_start_fails_the_turn_with_the_reason() {
        let mut fake = FakeWorkspace::with_scripts(Vec::new());
        fake.fail_preparation = true;
        let runner = runner(fake, vec![credential("first")]);
        let ticket = runner
            .submit(request(ConversationId::new(Uuid::now_v7()), "Anything"))
            .expect("accepted");
        let (_events, outcome) = collect(ticket).await;
        assert_eq!(outcome.status, TurnStatus::Failed);
        assert!(
            outcome
                .error
                .unwrap_or_default()
                .contains("could not be pulled")
        );
    }
}
