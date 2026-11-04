// Copyright © 2026 Jalapeno Labs

//! Signals between Elysium's processes: what one asks the other to look at now.
//!
//! A [`Signal`] carries no state. Whatever it is about is already in Postgres by the time it
//! is sent: a queued turn, a backup someone asked for, a write a link owes. The signal only
//! saves the listener waiting for its next interval, so a lost signal costs latency and never
//! work. Every loop that listens for one also looks on a timer, and a lost subscription ends
//! in [`Signal::Resync`], which every listener treats as the signal it waits for.
//!
//! Signals travel on their own Redis channel, [`super::SIGNALS_CHANNEL`], beside the events
//! clients hear, so nothing internal reaches a browser. The one exception to "no state" is the
//! password vault's requests and replies ([`Signal::VaultRequest`], [`Signal::VaultReply`]),
//! which carry their payload sealed with the encryption key, so Redis never holds a secret
//! (see `crate::passwords::remote`).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tracing::{Level, event};
use uuid::Uuid;

/// One signal. Serialized as `{"type": <name>, "data": <payload>}`, the events' envelope, so a
/// lost subscription's `{"type": "resync"}` reads as [`Signal::Resync`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Signal {
    /// Signals may have been lost. Every listener looks at its work as if it were signalled.
    Resync,
    /// An action item link owes its provider a write, or a container was linked: the link
    /// watcher's next pass should not wait.
    LinkWatcher,
    /// Elysia's queue or one of her holds changed.
    Elysia,
    /// Something woke a project's Project Manager: a message, a record on its project, or a
    /// batch's result.
    ProjectManagers,
    /// A Claude account was signed in, or a turn finished: the account keeper reads usage now.
    ClaudeAccounts,
    /// The workspace settings changed, so the runner fetches `~/.claude` again.
    AgentConfiguration,
    /// A session's turn was queued or asked to stop.
    SessionTurns,
    /// Someone asked for a backup.
    Backups,
    /// A browser was added or reset, and is owed a container.
    Browsers,
    /// A workflow run was started, or a person retried, skipped, or resumed one of its steps:
    /// the engine's step workers claim what is due now.
    Workflows,
    /// Someone asked the knowledge base a question: the engine's answerer takes it now.
    KnowledgeAsks,
    /// A message that asks something of its person joined its conversation: the engine's
    /// conversation loop starts that conversation's quiet window now.
    Conversations,
    /// Someone asked for connector `connector_id` to be read now.
    ConnectorSync { connector_id: Uuid },
    /// A request to the password vault, sealed for the engine. See `crate::passwords::remote`.
    VaultRequest { id: Uuid, sealed: String },
    /// The vault's answer to request `id`, sealed for whoever asked.
    VaultReply { id: Uuid, sealed: String },
}

impl Signal {
    /// The signal as it travels.
    ///
    /// # Panics
    /// Panics if a payload fails to serialize, which only a programming error can cause:
    /// every payload is plain data.
    pub fn to_json(&self) -> Arc<str> {
        serde_json::to_string(self)
            .expect("signals serialize")
            .into()
    }
}

/// Every signal any process sends from now on, as this process hears them.
#[derive(Debug)]
pub struct SignalReceiver {
    receiver: broadcast::Receiver<Arc<str>>,
}

impl SignalReceiver {
    pub(super) const fn new(receiver: broadcast::Receiver<Arc<str>>) -> Self {
        Self { receiver }
    }

    /// The next signal. A listener that fell behind, or a message no build of Elysium sent,
    /// reads as [`Signal::Resync`]: whatever was missed is looked at anyway. `None` once the
    /// bus has closed.
    pub async fn next(&mut self) -> Option<Signal> {
        match self.receiver.recv().await {
            Ok(payload) => Some(serde_json::from_str(&payload).unwrap_or_else(|error| {
                event!(
                    name: "realtime.signal.unreadable",
                    Level::WARN,
                    error.message = %error,
                    "a signal could not be read; treating it as resync",
                );
                Signal::Resync
            })),
            Err(RecvError::Lagged(missed)) => {
                event!(
                    name: "realtime.signal.lagged",
                    Level::WARN,
                    realtime.lost = missed,
                    "a listener fell {{realtime.lost}} signals behind; treating it as resync",
                );
                Some(Signal::Resync)
            }
            Err(RecvError::Closed) => None,
        }
    }

    /// Whatever is already waiting, without waiting for more.
    fn drain(&mut self) {
        use tokio::sync::broadcast::error::TryRecvError;
        loop {
            match self.receiver.try_recv() {
                Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                Err(TryRecvError::Empty | TryRecvError::Closed) => return,
            }
        }
    }
}

/// Wakes one loop on the signals it waits for, like a `tokio::sync::Notify` that spans
/// processes. Signals that arrive while the loop is busy are kept until it next waits, and
/// any number of them wake it once, since one pass serves every request made during it.
#[derive(Debug)]
pub struct Wake {
    signals: SignalReceiver,
    wants: fn(&Signal) -> bool,
}

impl Wake {
    pub(super) const fn new(signals: SignalReceiver, wants: fn(&Signal) -> bool) -> Self {
        Self { signals, wants }
    }

    /// Resolves at the next signal this loop wants, or at a resync. Never resolves once the
    /// bus has closed, since the loop's own shutdown arm ends it then.
    pub async fn woken(&mut self) {
        while let Some(signal) = self.signals.next().await {
            if signal == Signal::Resync || (self.wants)(&signal) {
                self.signals.drain();
                return;
            }
        }
        std::future::pending::<()>().await;
    }
}
