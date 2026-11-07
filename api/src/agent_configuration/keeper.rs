// Copyright © 2026 Jalapeno Labs

//! The engine's keeper of the configuration repository: it fetches the repository into the
//! shared workspace and records how each fetch went, which decides whether workers and Project
//! Managers may start (`super::hold`).
//!
//! It fetches as the engine starts, whenever it hears [`Signal::AgentConfiguration`] (an admin
//! changed the repository or its token, a GitHub token was edited or removed, or an admin asked
//! for a fetch), and every [`RETRY_INTERVAL`] while a hold other than an unset repository
//! remains, so a repository GitHub could not serve for a moment holds work back only until the
//! next try. Every fetch is announced (`super::announce`); one that lifts the hold resolves its
//! inbox item and wakes the pool and the Project Managers at once.

use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use super::Hold;
use crate::database::Pool;
use crate::models::agent_configuration as status_record;
use crate::realtime::{EventBus, Signal};
use crate::runner::Runner;
use crate::runner::configuration::Fetched;

/// How long a hold waits before the next fetch, when nobody asks for one sooner. Short enough
/// that work resumes soon after GitHub answers again; long enough that a repository nobody can
/// read is not fetched hundreds of times a day.
pub const RETRY_INTERVAL: Duration = Duration::from_mins(5);

/// Fetches the configuration repository into the shared workspace until `shutdown`.
pub async fn run(runner: Runner, database: Pool, events: EventBus, shutdown: CancellationToken) {
    // Taken before the first fetch, so a change made during it fetches again.
    let mut wake = events.wake_on(|signal| *signal == Signal::AgentConfiguration);
    let mut asked = true;
    loop {
        let owed = asked || retries(&database).await;
        if owed && let Err(error) = fetch(&runner, &database, &events).await {
            event!(
                name: "agent_configuration.fetch.failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "could not fetch the configuration repository or record how it went",
            );
        }
        asked = tokio::select! {
            () = shutdown.cancelled() => return,
            () = wake.woken() => true,
            () = tokio::time::sleep(RETRY_INTERVAL) => false,
        };
    }
}

/// Whether a fetch is owed without anyone asking: something other than an unset repository
/// holds work back. A database that cannot answer is tried again next time.
async fn retries(database: &Pool) -> bool {
    let hold = async {
        let mut connection = database.get().await?;
        anyhow::Ok(super::current_hold(&mut connection).await?)
    };
    match hold.await {
        Ok(hold) => matches!(hold, Some(Hold::Fetching | Hold::Failed { .. })),
        Err(error) => {
            event!(
                name: "agent_configuration.hold.unreadable",
                Level::WARN,
                error.message = %format!("{error:#}"),
                "could not read whether the configuration repository holds work back",
            );
            false
        }
    }
}

/// Fetches once, records the outcome, and announces it; a fetch that lifts the hold resolves
/// its inbox item and wakes everything it held back.
async fn fetch(runner: &Runner, database: &Pool, events: &EventBus) -> anyhow::Result<()> {
    let fetched = runner.fetch_configuration().await?;
    let mut connection = database.get().await?;
    match &fetched {
        Fetched::Unset => {}
        Fetched::Attempted {
            source,
            outcome: Ok(checkout),
        } => {
            status_record::record_fetched(&mut connection, source, checkout).await?;
        }
        Fetched::Attempted {
            source,
            outcome: Err(error),
        } => {
            event!(
                name: "agent_configuration.fetch.refused",
                Level::WARN,
                url.full = %source.repository,
                error.message = %error,
                "the configuration repository could not be fetched; workers and Project Managers wait",
            );
            status_record::record_failed(&mut connection, source, error, Utc::now()).await?;
        }
    }
    if super::current_hold(&mut connection).await?.is_none() {
        super::resolve_hold(&mut connection, events).await?;
        events.signal(&Signal::WorkerPool);
        events.signal(&Signal::ProjectManagers);
    }
    super::announce(&mut connection, events).await
}
