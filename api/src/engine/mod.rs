// Copyright © 2026 Jalapeno Labs

//! The engine: every loop, schedule, and agent turn, beside the API. See `docs/engine.md`.
//!
//! `elysium-engine` runs as its own compose service, from the same image as the API, as one
//! container (`elysium-engine`). It serves no HTTP but its own health check ([`health`]). It
//! runs:
//!
//! - **agent turns**: the runner and the workspace it prepares, sessions' turns
//!   (`crate::sessions::queue`), Elysia's intake and dispatcher (`crate::elysia`), and the Claude
//!   account keeper that refreshes what turns run on (`crate::claude_accounts`);
//! - **workflow runs**: the step workers that claim each run's steps from the queue and run them
//!   (`crate::workflows`);
//! - **readers**: the link watcher, connector ingest, the Confluence and YouTrack readers,
//!   meetings, and every read someone asked for ([`syncs`]);
//! - **keepers**: the browser reconciler, the mail server brought back up, the password vault and
//!   the requests the API sends it (`crate::passwords::remote`), update checks, nightly backups,
//!   and moving identities onto the current schema.
//!
//! Elysia's intake listens to the event bus, which every process hears in full, so it must run in
//! exactly one process; the engine is that process, and only one runs.
//!
//! The engine and the API meet only in Postgres and on the bus. What the API asks of the engine
//! (a queued turn, a backup, a sync) is a row the engine takes, followed by a
//! [`Signal`](crate::realtime::Signal) so it takes it at once rather than at its next look. What
//! the engine holds that a restart would cut short is mirrored to `process_activities`
//! (`crate::activity`), which the API reads before it lets the updater restart the stack.
//!
//! Shutdown mirrors the API's: a signal cancels the shutdown token, which every loop and turn
//! observes; the runner stops every turn, and their followers record how they ended; every loop
//! is awaited; then the event bus flushes what they published last, and the stores close.

mod health;
mod syncs;

use std::sync::Arc;

use anyhow::Result;
use tokio::task::JoinHandle;
use tracing::{Level, event};

use crate::action_items::watcher::{WatchContext, Watcher};
use crate::activity::Activities;
use crate::backups::{self, Backups, BackupsDeps};
use crate::browser::devtools::Devtools;
use crate::claude_accounts::oauth::Anthropic;
use crate::claude_accounts::{ClaudeAccounts, ClaudeAccountsDeps, TurnSignal};
use crate::confluence::Confluence;
use crate::confluence::reader::{Reader as ConfluenceReader, ReaderDeps as ConfluenceReaderDeps};
use crate::connectors::activity::Activity;
use crate::connectors::ingest::Ingest;
use crate::elysia::Elysia;
use crate::elysia::runner::TurnRunner;
use crate::meetings::fetch::{FetchMeeting, FetchMeetingDeps};
use crate::meetings::sources::Sources;
use crate::meetings::transcribe::{TranscribeMeeting, TranscribeMeetingDeps};
use crate::meetings::{self, Meetings, MeetingsDeps};
use crate::passwords::remote;
use crate::realtime::Signal;
use crate::runner::Runner;
use crate::services::{self, Foundation};
use crate::sessions::queue::{TurnQueue, TurnQueueDeps};
use crate::sessions::{Sessions, SessionsDeps};
use crate::tools::grants::Grants;
use crate::updates::{Updates, UpdatesDeps};
use crate::workflows::Steps;
use crate::workflows::workers::{Workers, WorkersDeps};
use crate::youtrack::Youtrack;
use crate::youtrack::reader::{ReaderDeps, YoutrackReader};

/// Runs the engine until a shutdown signal arrives.
///
/// # Errors
/// Fails on invalid configuration, an invalid encryption key, pending migrations, unreachable
/// stores, or a health listener that cannot bind.
#[expect(
    clippy::too_many_lines,
    reason = "startup is one ordered sequence, and reads best top to bottom in one place"
)]
pub async fn run() -> Result<()> {
    let foundation = Foundation::connect("elysium-engine").await?;
    let config = Arc::clone(&foundation.config);
    let database = foundation.database.clone();
    let cipher = Arc::clone(&foundation.cipher);
    let events = foundation.events.clone();
    let http = foundation.http.clone();
    let shutdown = foundation.shutdown.clone();
    let mut loops: Vec<(&'static str, JoinHandle<()>)> = Vec::new();

    // First, so the health check answers while the rest starts, and the stack's own waits
    // for health see the engine come up.
    let health = health::serve(config.bind_address, shutdown.clone()).await?;

    // What this engine runs that a restart would cut short, for auto-update to wait on.
    let (activity, mirror) = Activities::mirrored(database.clone(), shutdown.clone());

    // The password vault: kept up and synced, and answering the API's requests.
    let passwords =
        services::build_passwords(database.clone(), Arc::clone(&cipher), events.clone());
    loops.push(("passwords.keeper", passwords.start(shutdown.clone())));
    loops.push((
        "passwords.requests",
        remote::serve(
            passwords.clone(),
            events.clone(),
            Arc::clone(&cipher),
            shutdown.clone(),
        ),
    ));

    // Agent turns.
    let workspace = services::build_workspace(&config)?;
    let turn_signal = TurnSignal::default();
    let runner = Runner::new(
        workspace.clone(),
        database.clone(),
        Arc::clone(&cipher),
        activity.clone(),
        turn_signal.clone(),
        &shutdown,
    );
    let grants = Grants::new(database.clone());
    let sessions = Sessions::new(SessionsDeps {
        database: database.clone(),
        cipher: Arc::clone(&cipher),
        events: events.clone(),
        workspace: workspace.clone(),
    });
    let turn_queue = TurnQueue::new(TurnQueueDeps {
        sessions,
        runner: runner.clone(),
        grants: grants.clone(),
        storage: foundation.storage.clone(),
        passwords: passwords.clone(),
    });
    // Revokes what an earlier engine granted before granting anything itself.
    loops.push(("sessions.queue", turn_queue.start(shutdown.clone())));
    loops.push(("runner.prepare", prepare_workspace(runner.clone())));
    loops.push((
        "runner.configuration",
        refresh_configuration_when_changed(&foundation, runner.clone()),
    ));
    let claude_accounts = ClaudeAccounts::new(ClaudeAccountsDeps {
        database: database.clone(),
        cipher: Arc::clone(&cipher),
        events: events.clone(),
        anthropic: Anthropic::new(http.clone()),
        workspace,
        turn_signal,
    });
    loops.push((
        "claude_accounts.keeper",
        claude_accounts.start(shutdown.clone()),
    ));
    // Workflow runs, step by step. Every step a declared workflow names is registered here, once
    // by its name with what it needs, and each workflow is checked against them before the
    // workers start, so a workflow naming a step this build lacks stops the engine at once.
    let connector_activity = Activity::new(database.clone(), events.clone());
    let meeting_sources = Sources::new(http.clone());
    let meeting_workspace = services::build_workspace(&config)?;
    let mut steps = Steps::new();
    steps.register(FetchMeeting::new(FetchMeetingDeps {
        database: database.clone(),
        events: events.clone(),
        activity: connector_activity.clone(),
        cipher: Arc::clone(&cipher),
        storage: foundation.storage.clone(),
        workspace: meeting_workspace.clone(),
        sources: meeting_sources.clone(),
    })?);
    let whisper = services::build_whisper(&config)?;
    event!(
        name: "whisper.configured",
        Level::INFO,
        container.image.name = %config.whisper_image,
        whisper.threads = whisper.threads(),
        "transcriptions decode on {{whisper.threads}} threads",
    );
    steps.register(TranscribeMeeting::new(TranscribeMeetingDeps {
        database: database.clone(),
        cipher: Arc::clone(&cipher),
        storage: foundation.storage.clone(),
        whisper,
    }));
    steps.assert_runs(&meetings::workflow::workflow());
    let workers = Workers::new(WorkersDeps {
        database: database.clone(),
        events: events.clone(),
        activity: activity.clone(),
        steps,
    });
    loops.push(("workflows.workers", workers.start(shutdown.clone())));
    let elysia = Elysia::new(TurnRunner::Host(runner.clone()), grants);
    let elysia_tasks = elysia.start(
        database.clone(),
        events.clone(),
        passwords.clone(),
        shutdown.clone(),
    );

    // Readers.
    let watcher = Watcher::start(
        WatchContext {
            database: database.clone(),
            events: events.clone(),
            links: foundation.links.clone(),
            activity: connector_activity.clone(),
        },
        shutdown.clone(),
    );
    let ingest = Ingest::new(
        database.clone(),
        events.clone(),
        Devtools::new(http.clone()),
        connector_activity.clone(),
    );
    loops.push(("connectors.ingest", ingest.start(shutdown.clone())));
    let confluence = ConfluenceReader::new(ConfluenceReaderDeps {
        database: database.clone(),
        cipher: Arc::clone(&cipher),
        events: events.clone(),
        activity: connector_activity.clone(),
        confluence: Confluence::new(http.clone()),
    });
    loops.push(("confluence.reader", confluence.start(shutdown.clone())));
    let youtrack = YoutrackReader::new(ReaderDeps {
        database: database.clone(),
        events: events.clone(),
        cipher: Arc::clone(&cipher),
        youtrack: Youtrack::new(http.clone()),
        activity: connector_activity.clone(),
    });
    loops.push(("youtrack.reader", youtrack.start(shutdown.clone())));
    // Meetings: listed, each fetched and transcribed by its run of the meeting workflow, then
    // segmented by text turns on the runner and cut into clips in its workspace.
    let meetings = Meetings::new(MeetingsDeps {
        database: database.clone(),
        events: events.clone(),
        activity: connector_activity.clone(),
        cipher: Arc::clone(&cipher),
        storage: foundation.storage.clone(),
        runner: runner.clone(),
        workspace: meeting_workspace,
        sources: meeting_sources,
    });
    loops.push(("meetings", meetings.start(shutdown.clone())));
    loops.push((
        "connectors.syncs",
        syncs::start(
            syncs::Readers {
                database: database.clone(),
                events: events.clone(),
                activity: connector_activity,
                ingest,
                confluence,
                youtrack,
                meetings,
            },
            shutdown.clone(),
        ),
    ));

    // Keepers.
    let browsers = services::build_browsers(
        &config,
        http.clone(),
        database.clone(),
        Arc::clone(&cipher),
        events.clone(),
    )?;
    loops.push(("browser.reconciler", browsers.start(shutdown.clone())));
    let mail = services::build_mail(
        &config,
        http.clone(),
        database.clone(),
        Arc::clone(&cipher),
        events.clone(),
    )?;
    // Brings an existing mail server back up once; pulling its image may take a while, which
    // a shutdown does not wait for.
    let stopping = shutdown.clone();
    loops.push((
        "mail.hosting",
        tokio::spawn(async move {
            tokio::select! {
                () = stopping.cancelled() => {}
                () = mail.hosting.reconcile() => {}
            }
        }),
    ));
    let updates = Updates::new(UpdatesDeps {
        database: database.clone(),
        events: events.clone(),
        http: http.clone(),
        version: Arc::clone(&foundation.version),
        encryption_key: config.encryption_key.clone(),
    });
    loops.push(("updates.checker", updates.start(shutdown.clone())));
    let backups = Backups::new(BackupsDeps {
        database: database.clone(),
        cipher: Arc::clone(&cipher),
        storage: foundation.storage.clone(),
        events: events.clone(),
        activity,
        dumper: services::build_dumper(&config)?,
        staging_directory: backups::STAGING_DIRECTORY.into(),
        shutdown: shutdown.clone(),
    });
    loops.push(("backups.schedule", backups.start()));
    let auth = services::build_auth(&config, http);
    // Moves identities created before names were split onto the current schema.
    loops.push((
        "auth.identities",
        crate::auth::identity_schemas::start(auth.kratos, shutdown.clone()),
    ));

    event!(name: "engine.ready", Level::INFO, "the engine is running");
    foundation.shutdown_signal().await;

    // Every turn is killed and reports `interrupted`; the followers record it.
    runner.shutdown().await;
    turn_queue.shutdown().await;
    event!(name: "runner.shutdown.complete", Level::INFO, "agent turns stopped");
    watcher.stopped().await;
    elysia_tasks.stopped().await;
    for (name, task) in loops {
        if let Err(error) = task.await {
            event!(
                name: "engine.loop.join_failed",
                Level::ERROR,
                engine.loop.name = name,
                error.message = %error,
                "a loop ended abnormally",
            );
        }
    }
    if let Err(error) = mirror.await {
        event!(
            name: "activity.mirror.join_failed",
            Level::ERROR,
            error.message = %error,
            "the activity mirror ended abnormally",
        );
    }
    if let Err(error) = health.await {
        event!(
            name: "engine.health.join_failed",
            Level::ERROR,
            error.message = %error,
            "the health check ended abnormally",
        );
    }
    // Last of all, once nothing is left to publish: what the loops announced on their way out
    // still reaches every other process's clients.
    foundation.close().await;
    event!(name: "engine.shutdown.complete", Level::INFO, "shutdown complete");
    Ok(())
}

/// Pulls the workspace image, brings the workspace up, and fetches `~/.claude` ahead of the
/// first turn, which would otherwise wait for all of it.
fn prepare_workspace(runner: Runner) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(error) = runner.prepare().await {
            event!(
                name: "runner.workspace.unavailable",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "the agent workspace could not be prepared; turns retry when they start",
            );
        }
    })
}

/// Fetches `~/.claude` again whenever an admin changes the configuration repository, until
/// shutdown.
fn refresh_configuration_when_changed(foundation: &Foundation, runner: Runner) -> JoinHandle<()> {
    let mut wake = foundation
        .events
        .wake_on(|signal| *signal == Signal::AgentConfiguration);
    let shutdown = foundation.shutdown.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = wake.woken() => {}
            }
            if let Err(error) = runner.refresh_configuration().await {
                event!(
                    name: "runner.configuration.refresh_failed",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "the new configuration repository could not be fetched",
                );
            }
        }
    })
}
