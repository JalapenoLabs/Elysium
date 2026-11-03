// Copyright © 2026 Jalapeno Labs

//! Claude accounts signed in through OAuth, kept alive by Elysium, with their usage.
//!
//! Elysium holds three kinds of Claude credential: API keys and `claude setup-token` tokens,
//! which a person types in, and accounts signed in here through the official CLI. An account
//! differs in two ways: its access token lasts hours, not a year, so Elysium refreshes it, and
//! Anthropic reports its usage, so Elysium shows each window and keeps turns off an account
//! whose window is spent.
//!
//! - [`sign_in`]: `claude auth login` in the workspace container, as two steps a person drives
//!   (open a link, paste back a code).
//! - [`keeper`]: the one refresher. Turns get the access token alone; Elysium alone ever spends
//!   the refresh token, under a row lock.
//! - [`oauth`]: the refresh, profile, and usage endpoints Claude Code itself calls.
//! - [`usage`]: reading usage, and when it keeps an account out of the failover stack.
//!
//! The rules are ported from `~/.claude/accounts` (the desktop account switcher this replaces
//! for Elysium), simplified by Elysium being the only holder of each login: there is no live
//! credentials file to reconcile, so no grace period is needed before refreshing.

#[cfg(test)]
pub(crate) mod fake;
pub mod keeper;
pub mod oauth;
pub mod sign_in;
pub mod usage;

use std::fmt;
use std::sync::Arc;

use secrecy::SecretString;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use self::oauth::{Anthropic, OAuthError};
use self::sign_in::{SignInError, SignIns};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::claude_account::{self, NewCredential, SavedSignIn, SignedIn};
use crate::realtime::EventBus;
use crate::runner::workspace::WorkspaceContainer;

/// Rings when an agent turn finishes, so the keeper reads usage while it is fresh. Cheap to
/// clone; clones ring the same bell. Ringing while nobody listens leaves one ring waiting.
#[derive(Debug, Clone, Default)]
pub struct TurnSignal(Arc<Notify>);

impl TurnSignal {
    /// Tells the keeper a turn just finished.
    pub fn turn_finished(&self) {
        self.0.notify_one();
    }
}

/// Everything the accounts service is built from.
#[derive(Clone)]
pub struct ClaudeAccountsDeps {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub events: EventBus,
    pub anthropic: Anthropic,
    /// Where sign-ins run the CLI.
    pub workspace: WorkspaceContainer,
    /// The runner's signal that a turn finished.
    pub turn_signal: TurnSignal,
}

/// Why a sign-in could not be stored.
#[derive(Debug, thiserror::Error)]
pub enum FinishError {
    #[error(transparent)]
    SignIn(#[from] SignInError),
    /// The CLI signed in, but Anthropic would not say whose account it is.
    #[error("the account could not be identified: {0}")]
    Profile(#[from] OAuthError),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    #[error("no database connection is available: {0}")]
    NoDatabase(String),
}

/// Claude accounts: sign-ins, the keeper, and usage. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct ClaudeAccounts {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
    anthropic: Anthropic,
    sign_ins: SignIns,
    /// Asks the keeper for a pass now.
    wake: Notify,
    turn_signal: TurnSignal,
}

impl fmt::Debug for ClaudeAccounts {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClaudeAccounts")
    }
}

impl ClaudeAccounts {
    pub fn new(deps: ClaudeAccountsDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                cipher: deps.cipher,
                events: deps.events,
                anthropic: deps.anthropic,
                sign_ins: SignIns::new(deps.workspace),
                wake: Notify::new(),
                turn_signal: deps.turn_signal,
            }),
        }
    }

    /// Sign-ins in progress.
    pub fn sign_ins(&self) -> &SignIns {
        &self.inner.sign_ins
    }

    /// Asks the keeper for a pass now, such as right after an account was added.
    pub fn request_pass(&self) {
        self.inner.wake.notify_one();
    }

    /// Finishes `person`'s sign-in `id` with the code they pasted, learns whose account it is,
    /// and stores it: as a new credential named `name` at `priority`, or over the account's
    /// existing credential when it was signed in before.
    ///
    /// # Errors
    /// [`FinishError`] when the CLI refuses the code, Anthropic will not identify the account,
    /// or the credential cannot be stored, such as for a name already taken.
    pub async fn finish_sign_in(
        &self,
        id: Uuid,
        person: Uuid,
        code: &SecretString,
        name: Option<&str>,
        priority: i32,
    ) -> Result<SavedSignIn, FinishError> {
        let finished = self.inner.sign_ins.finish(id, person, code).await?;
        let identity = self
            .inner
            .anthropic
            .profile(&finished.tokens.access_token)
            .await?;
        let signed_in = SignedIn {
            tokens: finished.tokens,
            identity,
            subscription_type: finished.subscription_type,
        };
        let name = name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&signed_in.identity.email)
            .to_owned();

        let mut connection = self
            .inner
            .database
            .get()
            .await
            .map_err(|error| FinishError::NoDatabase(error.to_string()))?;
        let saved = claude_account::save_sign_in(
            &mut connection,
            &self.inner.cipher,
            NewCredential {
                name: &name,
                priority,
                created_by: person,
            },
            &signed_in,
        )
        .await?;
        event!(
            name: "claude_accounts.sign_in.finished",
            Level::INFO,
            llm.id = %saved.llm.id,
            sign_in.is_new = saved.is_new,
            user.id = %person,
            "a Claude account was signed in",
        );
        // Its usage is unknown until the keeper reads it.
        self.request_pass();
        Ok(saved)
    }

    /// Runs the keeper until `shutdown`: a pass at once, then every [`keeper::PASS_INTERVAL`],
    /// and whenever a turn finishes or a pass is requested.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(keeper::PASS_INTERVAL);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    _tick = ticks.tick() => {}
                    () = inner.wake.notified() => {}
                    () = inner.turn_signal.0.notified() => {}
                }
                keeper::run_pass(&inner).await;
            }
            event!(
                name: "claude_accounts.keeper.stopped",
                Level::INFO,
                "the Claude account keeper stopped",
            );
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::fake::FakeAnthropic;
    use super::*;

    /// A service against `database_url` whose OAuth calls go to `fake`. Its workspace points
    /// nowhere, so no test reaches Docker through it.
    pub(crate) async fn accounts_against(
        database_url: &SecretString,
        fake: &FakeAnthropic,
        cipher: &Arc<Cipher>,
    ) -> ClaudeAccounts {
        let database = crate::connections::connect_postgres(database_url, 4)
            .await
            .expect("Postgres accepts connections");
        let docker = bollard::Docker::connect_with_http(
            "http://127.0.0.1:9",
            5,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("a static address parses");
        ClaudeAccounts::new(ClaudeAccountsDeps {
            database,
            cipher: Arc::clone(cipher),
            events: EventBus::new(),
            anthropic: fake.client(),
            workspace: WorkspaceContainer::new(docker, "elysium-workspace:test".to_owned()),
            turn_signal: TurnSignal::default(),
        })
    }
}
