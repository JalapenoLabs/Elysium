// Copyright © 2026 Jalapeno Labs

//! The envelope every event is sent in, and the bus through Redis: delivery from a second
//! process, the event stream it feeds, and what a dropped subscription leads to.
//!
//! The Redis tests are `#[ignore]`d so `cargo test` stays hermetic; `scripts/verify-migrations.sh`
//! runs them against a disposable Redis. Each one works on a channel of its own, since every
//! test shares that Redis and pub/sub channels span the whole server. A second [`EventBus`] on
//! its own connections stands in for a second process, such as the engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, BodyDataStream};
use axum::http::{Request, StatusCode, header};
use futures_util::StreamExt as _;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::task::AbortHandle;
use tower::ServiceExt as _;
use url::Url;
use uuid::Uuid;

use super::{EVENTS_CHANNEL, EventBus, ServerEvent};
use crate::routes;
use crate::state::AppState;
use crate::test_support::{app_state, migrated_database, signed_in_cookie};

/// How long a test waits for something Redis delivers before failing. Generous: a
/// resubscription waits out its backoff first.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(20);

#[test]
fn events_serialize_as_a_type_and_data_envelope() {
    let id = Uuid::nil();
    let deleted: Value =
        serde_json::from_str(&ServerEvent::ProjectDeleted { id }.to_json()).expect("json");
    assert_eq!(
        deleted,
        json!({ "type": "project.deleted", "data": { "id": id } })
    );

    let session_deleted: Value =
        serde_json::from_str(&ServerEvent::SessionDeleted { id: 12 }.to_json()).expect("json");
    assert_eq!(
        session_deleted,
        json!({ "type": "session.deleted", "data": { "id": 12 } })
    );

    let action_item_id = Uuid::now_v7();
    let comment_deleted: Value = serde_json::from_str(
        &ServerEvent::ActionItemCommentDeleted { id, action_item_id }.to_json(),
    )
    .expect("json");
    assert_eq!(
        comment_deleted,
        json!({
            "type": "actionItemComment.deleted",
            "data": { "id": id, "actionItemId": action_item_id },
        })
    );

    let hello: Value = serde_json::from_str(&ServerEvent::Hello.to_json()).expect("json");
    assert_eq!(hello, json!({ "type": "hello" }));
}

#[tokio::test]
async fn every_subscriber_receives_each_published_event() {
    let bus = EventBus::in_process();
    let mut first = bus.subscribe();
    let mut second = bus.subscribe();

    bus.publish(&ServerEvent::Resync);

    assert_eq!(
        &*first.recv().await.expect("delivered"),
        r#"{"type":"resync"}"#
    );
    assert_eq!(
        &*second.recv().await.expect("delivered"),
        r#"{"type":"resync"}"#
    );
}

#[tokio::test]
#[ignore = "needs TEST_REDIS_URL; run scripts/verify-migrations.sh"]
async fn another_process_publishes_to_every_listener_in_order() {
    let channel = format!("{EVENTS_CHANNEL}:test:{}", Uuid::now_v7().simple());
    let listening = EventBus::connect_on(&redis_url(), &channel)
        .await
        .expect("the listening process subscribes");
    let engine = EventBus::connect_on(&redis_url(), &channel)
        .await
        .expect("the second process connects");
    let mut first = listening.subscribe();
    let mut second = listening.subscribe();

    let ids: Vec<Uuid> = (0..50).map(|_index| Uuid::now_v7()).collect();
    for id in &ids {
        engine.publish(&ServerEvent::ProjectDeleted { id: *id });
    }

    for listener in [&mut first, &mut second] {
        for id in &ids {
            let expected = ServerEvent::ProjectDeleted { id: *id }.to_json();
            assert_eq!(next(listener).await, expected, "the same shape, in order");
        }
    }

    // A process hears its own events through Redis too, in the same order as everyone's.
    listening.publish(&ServerEvent::Resync);
    assert_eq!(&*next(&mut first).await, r#"{"type":"resync"}"#);

    engine.close().await;
    listening.close().await;
}

#[tokio::test]
#[ignore = "needs TEST_REDIS_URL; run scripts/verify-migrations.sh"]
async fn a_quiet_subscription_stays_up_across_its_heartbeats() {
    let channel = format!("{EVENTS_CHANNEL}:test:{}", Uuid::now_v7().simple());
    let listening = EventBus::connect_on(&redis_url(), &channel)
        .await
        .expect("the listening process subscribes");
    let mut listener = listening.subscribe();

    // Ten pings under test. One misread pong would drop the subscription and resync every
    // listener on every heartbeat.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(listening.is_subscribed());
    assert!(
        matches!(
            listener.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ),
        "a healthy subscription sends nothing of its own"
    );

    listening.publish(&ServerEvent::Resync);
    assert_eq!(&*next(&mut listener).await, r#"{"type":"resync"}"#);
    listening.close().await;
}

#[tokio::test]
#[ignore = "needs TEST_REDIS_URL; run scripts/verify-migrations.sh"]
async fn a_dropped_subscription_reconnects_and_tells_listeners_to_resync() {
    let channel = format!("{EVENTS_CHANNEL}:test:{}", Uuid::now_v7().simple());
    let proxy = Proxy::start().await;
    let listening = EventBus::connect_on(&proxy.url(), &channel)
        .await
        .expect("the listening process subscribes through the proxy");
    let engine = EventBus::connect_on(&redis_url(), &channel)
        .await
        .expect("the second process connects");
    let mut listener = listening.subscribe();

    engine.publish(&ServerEvent::ProjectDeleted { id: Uuid::nil() });
    assert!(next(&mut listener).await.contains("project.deleted"));

    proxy.sever();
    wait_until(|| !listening.is_subscribed()).await;
    // Published while the listening process cannot hear it: lost to it, and never silently.
    let missed = Uuid::now_v7();
    engine.publish(&ServerEvent::ProjectDeleted { id: missed });
    // Give Redis time to deliver it to anyone still subscribed, so the next assertion would
    // see it if the subscription had survived.
    tokio::time::sleep(Duration::from_millis(200)).await;
    proxy.restore();

    assert_eq!(
        &*next(&mut listener).await,
        r#"{"type":"resync"}"#,
        "the gap ends in a resync, not in the missed event or in silence"
    );
    assert!(listening.is_subscribed());

    let after = Uuid::now_v7();
    engine.publish(&ServerEvent::ProjectDeleted { id: after });
    assert_eq!(
        next(&mut listener).await,
        ServerEvent::ProjectDeleted { id: after }.to_json(),
        "events flow again once the subscription is back"
    );

    engine.close().await;
    listening.close().await;
}

#[tokio::test]
#[ignore = "needs TEST_REDIS_URL; run scripts/verify-migrations.sh"]
async fn a_publish_redis_never_took_is_followed_by_a_resync_on_the_channel() {
    let channel = format!("{EVENTS_CHANNEL}:test:{}", Uuid::now_v7().simple());
    let proxy = Proxy::start().await;
    let engine = EventBus::connect_on(&proxy.url(), &channel)
        .await
        .expect("the publishing process connects through the proxy");
    let listening = EventBus::connect_on(&redis_url(), &channel)
        .await
        .expect("the listening process subscribes");
    let mut listener = listening.subscribe();

    proxy.sever();
    engine.publish(&ServerEvent::ProjectDeleted { id: Uuid::now_v7() });
    // Long enough for the publish to fail and its first retry of the resync to fail too.
    tokio::time::sleep(Duration::from_secs(1)).await;
    proxy.restore();

    assert_eq!(
        &*next(&mut listener).await,
        r#"{"type":"resync"}"#,
        "every process's listeners hear that events were lost"
    );

    let after = Uuid::now_v7();
    engine.publish(&ServerEvent::ProjectDeleted { id: after });
    assert_eq!(
        next(&mut listener).await,
        ServerEvent::ProjectDeleted { id: after }.to_json(),
        "the publisher carries on once Redis takes events again"
    );

    engine.close().await;
    listening.close().await;
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run scripts/verify-migrations.sh"]
async fn the_event_stream_delivers_what_another_process_publishes() {
    let (database_url, _connection) = migrated_database().await;
    let mut state = app_state(&database_url).await;
    let channel = format!("{EVENTS_CHANNEL}:test:{}", Uuid::now_v7().simple());
    state.events = EventBus::connect_on(&redis_url(), &channel)
        .await
        .expect("the API subscribes");
    let engine = EventBus::connect_on(&redis_url(), &channel)
        .await
        .expect("the second process connects");

    let response = open_stream(&state).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    assert_eq!(next_frame(&mut body).await, r#"{"type":"hello"}"#);

    let id = Uuid::now_v7();
    engine.publish(&ServerEvent::TimelineRecordCreated {
        id,
        source_kind: "mail".to_owned(),
        created_by: Uuid::nil(),
        action_item_id: None,
    });
    let delivered: Value = serde_json::from_str(&next_frame(&mut body).await).expect("json");
    assert_eq!(
        delivered,
        json!({
            "type": "timelineRecord.created",
            "data": { "id": id, "sourceKind": "mail", "createdBy": Uuid::nil(), "actionItemId": null },
        }),
        "ids and kinds, exactly as the API's own events are sent"
    );

    engine.close().await;
    state.events.close().await;
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run scripts/verify-migrations.sh"]
async fn no_event_stream_opens_while_the_subscription_is_down() {
    let (database_url, _connection) = migrated_database().await;
    let mut state = app_state(&database_url).await;
    let channel = format!("{EVENTS_CHANNEL}:test:{}", Uuid::now_v7().simple());
    let proxy = Proxy::start().await;
    state.events = EventBus::connect_on(&proxy.url(), &channel)
        .await
        .expect("the API subscribes through the proxy");

    proxy.sever();
    wait_until(|| !state.events.is_subscribed()).await;
    let refused = open_stream(&state).await;
    assert_eq!(
        refused.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "a hello now would promise events this process cannot hear"
    );

    proxy.restore();
    wait_until(|| state.events.is_subscribed()).await;
    let response = open_stream(&state).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    assert_eq!(next_frame(&mut body).await, r#"{"type":"hello"}"#);

    state.events.close().await;
}

fn redis_url() -> SecretString {
    SecretString::from(
        std::env::var("TEST_REDIS_URL").expect("TEST_REDIS_URL must point at a disposable Redis"),
    )
}

/// The next message a listener receives, failing the test if none arrives in time.
async fn next(listener: &mut broadcast::Receiver<Arc<str>>) -> Arc<str> {
    tokio::time::timeout(DELIVERY_TIMEOUT, listener.recv())
        .await
        .expect("an event arrives in time")
        .expect("the bus is open")
}

/// Polls `condition` until it holds, failing the test if it never does.
async fn wait_until(condition: impl Fn() -> bool) {
    let polling = async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(DELIVERY_TIMEOUT, polling)
        .await
        .expect("the condition holds in time");
}

/// `GET /api/v1/events` as the test person.
async fn open_stream(state: &AppState) -> axum::response::Response {
    let request = Request::builder()
        .uri("/api/v1/events")
        .header(header::COOKIE, signed_in_cookie(state).await)
        .body(Body::empty())
        .expect("a request builds");
    routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers")
}

/// The `data` of the next SSE message on `body`, skipping keep-alive comments.
async fn next_frame(body: &mut BodyDataStream) -> String {
    let mut buffered = String::new();
    loop {
        if let Some(end) = buffered.find("\n\n") {
            let frame: String = buffered.drain(..end + 2).collect();
            if let Some(data) = frame.trim_end().strip_prefix("data: ") {
                return data.to_owned();
            }
            continue;
        }
        let chunk = tokio::time::timeout(DELIVERY_TIMEOUT, body.next())
            .await
            .expect("the stream sends in time")
            .expect("the stream is open")
            .expect("the stream reads");
        buffered.push_str(std::str::from_utf8(&chunk).expect("SSE is text"));
    }
}

/// A TCP proxy in front of the test Redis that a test can cut, so one bus loses its
/// connections without touching any other test's.
struct Proxy {
    address: std::net::SocketAddr,
    open: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<AbortHandle>>>,
}

impl Proxy {
    async fn start() -> Self {
        let upstream = Url::parse(redis_url().expose_secret()).expect("TEST_REDIS_URL parses");
        let upstream_address = format!(
            "{}:{}",
            upstream.host_str().expect("TEST_REDIS_URL names a host"),
            upstream.port().unwrap_or(6379),
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the proxy binds");
        let address = listener.local_addr().expect("the proxy has an address");
        let open = Arc::new(AtomicBool::new(true));
        let connections: Arc<Mutex<Vec<AbortHandle>>> = Arc::default();

        let accepting = Arc::clone(&open);
        let tracked = Arc::clone(&connections);
        tokio::spawn(async move {
            while let Ok((mut client, _peer)) = listener.accept().await {
                // While cut, connections are accepted and dropped at once, as a Redis that
                // is restarting would.
                if !accepting.load(Ordering::Acquire) {
                    continue;
                }
                let upstream_address = upstream_address.clone();
                let relaying = tokio::spawn(async move {
                    let mut server = TcpStream::connect(upstream_address)
                        .await
                        .expect("the proxy reaches Redis");
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                });
                tracked
                    .lock()
                    .expect("the proxy's lock is never poisoned")
                    .push(relaying.abort_handle());
            }
        });

        Self {
            address,
            open,
            connections,
        }
    }

    /// The test Redis's URL, through this proxy.
    fn url(&self) -> SecretString {
        let mut url = Url::parse(redis_url().expose_secret()).expect("TEST_REDIS_URL parses");
        url.set_port(Some(self.address.port()))
            .expect("a Redis URL takes a port");
        SecretString::from(url.to_string())
    }

    /// Closes every proxied connection, and refuses new ones until [`Proxy::restore`].
    fn sever(&self) {
        self.open.store(false, Ordering::Release);
        let connections = std::mem::take(
            &mut *self
                .connections
                .lock()
                .expect("the proxy's lock is never poisoned"),
        );
        for connection in connections {
            connection.abort();
        }
    }

    fn restore(&self) {
        self.open.store(true, Ordering::Release);
    }
}
