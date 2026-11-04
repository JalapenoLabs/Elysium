// Copyright © 2026 Jalapeno Labs

//! The event bus's link to Redis: one publisher and one subscription per process.
//!
//! [`RedisLink::start`] opens two connections, because a Redis connection that has
//! subscribed may only subscribe, unsubscribe, and ping:
//!
//! - The **publisher** is a self-reconnecting [`ConnectionManager`] fed by a bounded queue.
//!   [`RedisLink::send`] never waits, so publishing stays a plain call at every write; one
//!   task drains the queue in order and sends each batch as one pipeline of `PUBLISH`es, so
//!   a process's events reach the channel in the order it published them.
//! - The **subscription** is a dedicated pub/sub connection whose messages are relayed into
//!   the process's local `broadcast` channel. It is pinged, so a connection that silently
//!   stopped answering is found as surely as one that closed.
//!
//! The subscription is held explicitly rather than through the connection manager's
//! automatic resubscription, because the bus must know exactly when it is subscribed:
//! that is what lets a new event stream promise, with its `hello`, that it misses nothing,
//! and what decides when listeners are told to `resync`.
//!
//! # Gaps
//!
//! An event lost on its way through Redis always ends in a `resync`, never in silence:
//!
//! - **The subscription drops.** It is logged, reconnected with backoff, and confirmed;
//!   only then is `resync` sent to local listeners, since a listener that reloaded any
//!   earlier could still miss what is published before the subscription returned. While it
//!   is down, [`RedisLink::is_subscribed`] is false and new event streams are refused.
//! - **A publish fails.** The batch is lost; it is logged, and `resync` is published on the
//!   channel as soon as Redis answers again, so every process's listeners reload.
//! - **The queue overflows** (Redis slow or away for long). Events that do not fit are
//!   dropped, the overflow is logged once, and `resync` follows the events still queued.
//!
//! A `resync` always goes out after the events it stands in for would have, so a listener
//! never applies an event older than what its reload read.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt as _;
use redis::aio::{ConnectionManager, PubSub};
use redis::{ErrorKind, Msg, RedisError, RedisResult};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::{MissedTickBehavior, interval, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Level, event};

use super::ServerEvent;
use crate::connections;

/// Events that may wait for Redis before new ones are dropped. As many as a slow listener
/// may fall behind locally, which covers a Redis restart of several seconds under load.
const OUTBOX_CAPACITY: usize = super::BUS_CAPACITY;

/// The most events sent in one pipeline. Large enough that a burst of agent output costs a
/// few round trips rather than hundreds; small enough that one pipeline stays well under
/// the connection's response timeout.
const PUBLISH_BATCH: usize = 256;

/// How often the subscription is pinged. A connection Redis stopped answering without
/// closing it is found within this plus [`PING_TIMEOUT`], rather than never.
#[cfg(not(test))]
const PING_INTERVAL: Duration = Duration::from_secs(15);
/// Short under test, so every test that holds a subscription for a moment pings it.
#[cfg(test)]
const PING_INTERVAL: Duration = Duration::from_millis(100);

/// How long Redis may take to answer a ping, or to open and confirm a subscription.
const PING_TIMEOUT: Duration = Duration::from_secs(5);

/// First delay between attempts to reach Redis again; doubles up to [`MAX_BACKOFF`]. Retries
/// never give up: the process stays up, and streams are refused, until Redis returns.
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// How long closing may spend publishing what is still queued. Bounded, so a Redis that is
/// away at shutdown cannot hold the process past Docker's stop timeout.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// One process's link to the events channel. Cheap to clone; clones share it.
#[derive(Debug, Clone)]
pub(super) struct RedisLink {
    outbox: mpsc::Sender<Arc<str>>,
    /// Set when an event did not fit in the outbox; the publisher sends `resync` after what
    /// is queued, and clears it.
    overflowed: Arc<AtomicBool>,
    subscribed: watch::Receiver<bool>,
    /// Ends both tasks, once shutdown has stopped everything that publishes.
    closing: CancellationToken,
    tasks: TaskTracker,
}

impl RedisLink {
    /// Connects the publisher, subscribes to `channel`, and starts relaying into `local`.
    ///
    /// Returns once Redis has confirmed the subscription.
    ///
    /// # Errors
    /// Fails when the URL is malformed, or Redis stays unreachable through the startup
    /// retries of `crate::connections`.
    pub(super) async fn start(
        redis_url: &SecretString,
        channel: &str,
        local: broadcast::Sender<Arc<str>>,
    ) -> Result<Self> {
        let client = redis::Client::open(redis_url.expose_secret())
            .context("REDIS_URL is not a valid Redis URL")?;
        let publisher = connections::connect_redis(redis_url).await?;
        let subscription =
            connections::with_retry("redis", || open_subscription(&client, channel)).await?;
        event!(
            name: "realtime.subscription.open",
            Level::INFO,
            messaging.destination.name = channel,
            "subscribed to the events channel",
        );

        let (outbox, queued) = mpsc::channel(OUTBOX_CAPACITY);
        let overflowed = Arc::new(AtomicBool::new(false));
        let (subscribed_sender, subscribed) = watch::channel(true);
        let closing = CancellationToken::new();
        let tasks = TaskTracker::new();

        tasks.spawn(publish_until_closed(Publisher {
            connection: publisher,
            channel: channel.to_owned(),
            queued,
            overflowed: Arc::clone(&overflowed),
            closing: closing.clone(),
        }));
        tasks.spawn(relay_until_closed(
            Relay {
                client,
                channel: channel.to_owned(),
                local,
                subscribed: subscribed_sender,
                closing: closing.clone(),
            },
            subscription,
        ));
        tasks.close();

        Ok(Self {
            outbox,
            overflowed,
            subscribed,
            closing,
            tasks,
        })
    }

    /// Queues `payload` for the channel, without waiting.
    pub(super) fn send(&self, payload: Arc<str>) {
        match self.outbox.try_send(payload) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                // Logged once per overflow, not once per dropped event: during an outage
                // agent output alone would flood the log.
                if !self.overflowed.swap(true, Ordering::AcqRel) {
                    event!(
                        name: "realtime.publish.overflow",
                        Level::WARN,
                        "events are arriving faster than Redis takes them; dropping them until \
                         the queue drains, then sending resync",
                    );
                }
            }
            Err(TrySendError::Closed(_)) => {
                event!(
                    name: "realtime.publish.closed",
                    Level::WARN,
                    "an event was published after the bus closed and was dropped",
                );
            }
        }
    }

    pub(super) fn is_subscribed(&self) -> bool {
        *self.subscribed.borrow()
    }

    /// Flushes the queue, then ends both tasks and waits for them.
    pub(super) async fn close(&self) {
        self.closing.cancel();
        self.tasks.wait().await;
    }
}

/// Opens a pub/sub connection and subscribes it to `channel`, which Redis has confirmed by
/// the time this returns.
async fn open_subscription(client: &redis::Client, channel: &str) -> RedisResult<PubSub> {
    let subscribing = async {
        let mut subscription = client.get_async_pubsub().await?;
        subscription.subscribe(channel).await?;
        Ok(subscription)
    };
    timeout(PING_TIMEOUT, subscribing)
        .await
        .map_err(|_elapsed| {
            RedisError::from((
                ErrorKind::Io,
                "Redis did not confirm the subscription in time",
            ))
        })?
}

/// What the publishing task owns.
struct Publisher {
    connection: ConnectionManager,
    channel: String,
    queued: mpsc::Receiver<Arc<str>>,
    overflowed: Arc<AtomicBool>,
    closing: CancellationToken,
}

/// Publishes queued events in order until the link closes, then flushes what is left.
async fn publish_until_closed(mut publisher: Publisher) {
    let mut batch = Vec::with_capacity(PUBLISH_BATCH);
    loop {
        tokio::select! {
            // `recv_many` is cancel safe: nothing is taken off the queue unless it returns.
            received = publisher.queued.recv_many(&mut batch, PUBLISH_BATCH) => {
                if received == 0 {
                    break;
                }
                publisher.publish_batch(&mut batch).await;
            }
            () = publisher.closing.cancelled() => break,
        }
    }

    // Whatever was published before closing still goes out, so the last changes of a
    // process that is stopping reach every other process's clients.
    publisher.queued.close();
    let flush = async {
        while publisher.queued.recv_many(&mut batch, PUBLISH_BATCH).await > 0 {
            publisher.publish_batch(&mut batch).await;
        }
    };
    if timeout(FLUSH_TIMEOUT, flush).await.is_err() {
        event!(
            name: "realtime.publish.flush.timeout",
            Level::ERROR,
            "Redis did not take the last events before shutdown; they are lost",
        );
    }
    event!(name: "realtime.publish.stopped", Level::INFO, "the event publisher stopped");
}

impl Publisher {
    /// Publishes `batch`, then empties it. A batch that fails is lost, and `resync` goes out
    /// in its place once Redis answers again.
    async fn publish_batch(&mut self, batch: &mut Vec<Arc<str>>) {
        // After the events still queued, never before: a listener that reloads before
        // applying them would apply changes older than what it read.
        if self.overflowed.swap(false, Ordering::AcqRel) {
            batch.push(ServerEvent::Resync.to_json());
        }

        if let Err(error) = self.publish_all(batch).await {
            event!(
                name: "realtime.publish.failure",
                Level::WARN,
                realtime.lost = batch.len(),
                error.message = %error,
                "could not publish {{realtime.lost}} events; sending resync once Redis answers",
            );
            self.publish_resync().await;
        }
        batch.clear();
    }

    async fn publish_all(&mut self, payloads: &[Arc<str>]) -> RedisResult<()> {
        let mut pipeline = redis::pipe();
        for payload in payloads {
            pipeline
                .cmd("PUBLISH")
                .arg(&self.channel)
                .arg(&**payload)
                .ignore();
        }
        pipeline.query_async(&mut self.connection).await
    }

    /// Publishes `resync`, retrying with backoff until Redis takes it or the link closes.
    async fn publish_resync(&mut self) {
        let resync = [ServerEvent::Resync.to_json()];
        let mut backoff = INITIAL_BACKOFF;
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            tokio::select! {
                () = sleep(backoff) => {}
                () = self.closing.cancelled() => {
                    event!(
                        name: "realtime.publish.resync.abandoned",
                        Level::ERROR,
                        "shutting down before Redis took the resync owed for lost events",
                    );
                    return;
                }
            }
            match self.publish_all(&resync).await {
                Ok(()) => {
                    event!(
                        name: "realtime.publish.recovered",
                        Level::INFO,
                        attempt,
                        "Redis takes events again; resync published",
                    );
                    return;
                }
                Err(error) => {
                    event!(
                        name: "realtime.publish.retry",
                        Level::WARN,
                        attempt,
                        error.message = %error,
                        retry.delay_ms = backoff.as_millis(),
                        "Redis still refuses events, retrying",
                    );
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
        }
    }
}

/// What the relaying task owns.
struct Relay {
    client: redis::Client,
    channel: String,
    local: broadcast::Sender<Arc<str>>,
    subscribed: watch::Sender<bool>,
    closing: CancellationToken,
}

/// Why relaying one subscription ended.
enum RelayEnd {
    Closing,
    Lost(String),
}

/// Relays `subscription` into the local channel, holding a subscription until the link
/// closes: one that is lost is reconnected, confirmed, and followed by a local `resync`.
async fn relay_until_closed(relay: Relay, mut subscription: PubSub) {
    loop {
        let reason = match relay.relay(subscription).await {
            RelayEnd::Closing => break,
            RelayEnd::Lost(reason) => reason,
        };
        relay.subscribed.send_replace(false);
        event!(
            name: "realtime.subscription.lost",
            Level::WARN,
            error.message = %reason,
            "lost the events subscription; reconnecting, and refusing new event streams until then",
        );

        let Some(restored) = relay.resubscribe().await else {
            break;
        };
        subscription = restored;
        // Only now, subscribed again: a listener told to reload any earlier could still miss
        // what is published before the subscription returned.
        if relay.local.send(ServerEvent::Resync.to_json()).is_err() {
            event!(name: "realtime.publish.unobserved", Level::TRACE, "nobody is listening");
        }
        relay.subscribed.send_replace(true);
    }
    relay.subscribed.send_replace(false);
    event!(name: "realtime.subscription.stopped", Level::INFO, "the events subscription stopped");
}

impl Relay {
    /// Relays one subscription until it is lost or the link closes.
    async fn relay(&self, subscription: PubSub) -> RelayEnd {
        let (mut sink, mut messages) = subscription.split();
        let mut heartbeat = interval(PING_INTERVAL);
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // The first tick completes at once; the subscription was just confirmed.
        heartbeat.tick().await;

        loop {
            tokio::select! {
                () = self.closing.cancelled() => return RelayEnd::Closing,
                message = messages.next() => match message {
                    Some(message) => self.forward(&message),
                    None => return RelayEnd::Lost("the connection closed".to_owned()),
                },
                _ = heartbeat.tick() => {
                    match timeout(PING_TIMEOUT, sink.ping::<redis::Value>()).await {
                        Ok(Ok(_pong)) => {}
                        Ok(Err(error)) => return RelayEnd::Lost(error.to_string()),
                        Err(_elapsed) => {
                            return RelayEnd::Lost("Redis did not answer a ping".to_owned());
                        }
                    }
                }
            }
        }
    }

    /// Hands one message from the channel to local listeners.
    fn forward(&self, message: &Msg) {
        let payload = match message.get_payload::<String>() {
            Ok(payload) => Arc::from(payload),
            // Only a process that is not Elysium's could publish this; listeners reload
            // rather than miss whatever it stood for.
            Err(error) => {
                event!(
                    name: "realtime.subscription.unreadable",
                    Level::WARN,
                    error.message = %error,
                    "a message on the events channel was not text; sending resync in its place",
                );
                ServerEvent::Resync.to_json()
            }
        };
        if self.local.send(payload).is_err() {
            event!(name: "realtime.publish.unobserved", Level::TRACE, "nobody is listening");
        }
    }

    /// Subscribes again, with backoff, until Redis confirms it or the link closes.
    async fn resubscribe(&self) -> Option<PubSub> {
        let mut backoff = INITIAL_BACKOFF;
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            tokio::select! {
                () = sleep(backoff) => {}
                () = self.closing.cancelled() => return None,
            }
            match open_subscription(&self.client, &self.channel).await {
                Ok(subscription) => {
                    event!(
                        name: "realtime.subscription.restored",
                        Level::INFO,
                        attempt,
                        "subscribed to the events channel again; listeners resync",
                    );
                    return Some(subscription);
                }
                Err(error) => {
                    event!(
                        name: "realtime.subscription.retry",
                        Level::WARN,
                        attempt,
                        error.message = %error,
                        retry.delay_ms = backoff.as_millis(),
                        "could not subscribe to the events channel, retrying",
                    );
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
        }
    }
}
