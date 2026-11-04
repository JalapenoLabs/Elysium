// Copyright © 2026 Jalapeno Labs

//! Token-bucket rate limiting: per client IP across `/api`, and per person where a route asks.
//!
//! Clients are keyed by IP. Because the API only ever sits behind nginx, the IP
//! comes from the `X-Forwarded-For` / `X-Real-IP` headers nginx sets, falling back
//! to the peer address. Never expose the API port directly: a client that can
//! reach it without nginx could forge those headers and dodge its bucket.
//!
//! [`Limiter`] is keyed by anything hashable, so a route with a budget of its own, such as
//! the global search's per person (`crate::search`), holds one keyed by the person's id and
//! asks it [`Limiter::decide`] from its handler. Such a limiter needs no sweeper: its keys are
//! signed-in people, whom the `users` table already bounds.
//!
//! # The clock
//!
//! Buckets are timed by a [`LimiterClock`], which outside tests is always
//! [`LimiterClock::Monotonic`]: `std::time::Instant`, and on Linux `CLOCK_MONOTONIC`.
//! That clock pauses while the host is suspended, so a bucket is exactly as full after a
//! night asleep as it was before. governor's default quanta
//! clock reads the CPU's time stamp counter instead, which does not hold across a
//! suspend: on waking, every stored bucket sat hours ahead of "now", and every client,
//! including ones that had never called, was refused with a `Retry-After` the length
//! of the night. `tower_governor` fixes that clock in its types, so this module drives
//! governor directly.
//!
//! A refusal asking for longer than an empty bucket takes to refill is one no working
//! clock can produce, so the limiter treats it as a clock that moved underneath it:
//! it logs, starts every bucket afresh, and admits the request.
//!
//! Tests time a limiter by a [`HeldClock`] instead, which stands still until the test
//! moves it. A test counting a budget out through the router takes long enough per
//! request on a busy runner that a real bucket refills about as fast as the test spends
//! it, so the refusal it waits for may never come. With time held, exactly the burst is
//! admitted, and refilling happens only when the test says time has passed.

use std::hash::Hash;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use governor::Quota;
use governor::clock::Clock;
use governor::middleware::StateInformationMiddleware;
use governor::state::keyed::DefaultKeyedStateStore;
use tokio::task::JoinHandle;
use tracing::{Level, event};

/// Sustained requests per second allowed per client IP.
///
/// Fixed in code rather than read from the environment so every deployment enforces
/// the same limit. Ten per second is well above what a person clicking through the UI
/// produces, while still throttling scripted abuse.
const REQUESTS_PER_SECOND: u32 = 10;

/// Requests a client may spend at once before the sustained rate applies. Covers a
/// page load that fires several API calls in parallel.
pub(crate) const BURST_SIZE: u32 = 30;

/// Every client IP's budget across `/api`.
pub(crate) const PER_CLIENT: Rate = Rate {
    per_second: REQUESTS_PER_SECOND,
    burst: BURST_SIZE,
};

/// Buckets for clients that stopped talking are dropped on this cadence so the
/// key map cannot grow without bound under a scan.
const STALE_KEY_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

const LOCK_POISONED: &str = "rate limit buckets lock poisoned";

static RATE_LIMIT_LIMIT: HeaderName = HeaderName::from_static("x-ratelimit-limit");
static RATE_LIMIT_REMAINING: HeaderName = HeaderName::from_static("x-ratelimit-remaining");
static RATE_LIMIT_AFTER: HeaderName = HeaderName::from_static("x-ratelimit-after");

type Buckets<Key> = governor::RateLimiter<
    Key,
    DefaultKeyedStateStore<Key>,
    LimiterClock,
    StateInformationMiddleware,
>;

/// What a limiter's buckets are timed by.
///
/// One enum rather than a type parameter, so the limiters `AppState` holds keep one type
/// whichever clock a test gives them.
#[derive(Debug, Clone)]
pub enum LimiterClock {
    /// The host's monotonic clock, `std::time::Instant`. Every limiter outside tests.
    Monotonic,
    /// A clock that stands still until a test moves it.
    #[cfg(test)]
    Held(HeldClock),
}

impl Clock for LimiterClock {
    type Instant = Instant;

    fn now(&self) -> Instant {
        match self {
            Self::Monotonic => Instant::now(),
            #[cfg(test)]
            Self::Held(clock) => clock.now(),
        }
    }
}

/// Time a test holds still, moved only by [`HeldClock::set`]. Clones share one time.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct HeldClock {
    /// What the clock reads when set to zero.
    origin: Instant,
    /// How far past `origin` the clock reads, in nanoseconds.
    elapsed: Arc<AtomicU64>,
}

#[cfg(test)]
impl HeldClock {
    /// A clock held at the moment it is made.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            elapsed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Moves the clock to `elapsed` past where it started, in either direction, as a CPU's
    /// time stamp counter can move across a suspend.
    pub fn set(&self, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).expect("a test's instants fit in u64");
        self.elapsed.store(nanos, Ordering::SeqCst);
    }

    fn now(&self) -> Instant {
        self.origin + Duration::from_nanos(self.elapsed.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
impl Default for HeldClock {
    fn default() -> Self {
        Self::new()
    }
}

/// A bucket's budget: how many requests may be spent at once, and how fast they come back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// Requests that come back each second. Never zero.
    pub per_second: u32,
    /// Requests that may be spent at once before the sustained rate applies. Never zero.
    pub burst: u32,
}

/// What the limiter decided about one request.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Admitted { remaining: u32 },
    Refused { wait: Duration },
}

/// Every key's bucket, and the clock they are measured against.
pub struct Limiter<Key: Clone + Eq + Hash> {
    clock: LimiterClock,
    quota: Quota,
    /// How long an empty bucket takes to refill. No honest refusal waits longer.
    longest_honest_wait: Duration,
    /// Behind a lock only so a clock that moved can be answered by starting over.
    buckets: RwLock<Arc<Buckets<Key>>>,
}

impl<Key: Clone + Eq + Hash> Limiter<Key> {
    /// A limiter giving every key `rate`, timed by the monotonic clock.
    ///
    /// # Panics
    /// Panics when either half of `rate` is zero, which only a constant written wrong can be.
    pub fn new(rate: Rate) -> Self {
        Self::with_clock(LimiterClock::Monotonic, rate)
    }

    /// A limiter giving every key `rate`, timed by `clock`.
    ///
    /// # Panics
    /// Panics when either half of `rate` is zero, which only a constant written wrong can be.
    pub fn with_clock(clock: LimiterClock, rate: Rate) -> Self {
        let refill_period = Duration::from_secs(1) / rate.per_second;
        let burst = NonZeroU32::new(rate.burst).expect("a rate's burst is a non-zero constant");
        let quota = Quota::with_period(refill_period)
            .expect("a rate's per_second is a non-zero constant")
            .allow_burst(burst);

        Self {
            buckets: RwLock::new(Arc::new(fresh_buckets(quota, &clock))),
            clock,
            quota,
            longest_honest_wait: refill_period * rate.burst,
        }
    }

    /// Spends one request from `key`'s bucket, or says how long until one comes back.
    pub fn decide(&self, key: &Key) -> Decision {
        let wait = match self.check(key) {
            Ok(remaining) => return Decision::Admitted { remaining },
            Err(wait) => wait,
        };
        if wait <= self.longest_honest_wait {
            return Decision::Refused { wait };
        }

        event!(
            name: "rate_limit.clock.moved",
            Level::ERROR,
            rate_limit.wait_seconds = wait.as_secs(),
            "a refusal asked for longer than a bucket takes to refill; starting every bucket afresh",
        );
        *self.buckets.write().expect(LOCK_POISONED) =
            Arc::new(fresh_buckets(self.quota, &self.clock));

        match self.check(key) {
            Ok(remaining) => Decision::Admitted { remaining },
            Err(wait) => Decision::Refused { wait },
        }
    }

    /// One conformance check: the remaining burst, or how long to wait.
    fn check(&self, key: &Key) -> Result<u32, Duration> {
        // Cloned out so the read lock is not held across the check.
        let buckets = Arc::clone(&self.buckets.read().expect(LOCK_POISONED));
        buckets
            .check_key(key)
            .map(|snapshot| snapshot.remaining_burst_capacity())
            .map_err(|refusal| refusal.wait_time_from(self.clock.now()))
    }

    fn sweep(&self) -> usize {
        let buckets = Arc::clone(&self.buckets.read().expect(LOCK_POISONED));
        buckets.retain_recent();
        buckets.len()
    }
}

fn fresh_buckets<Key: Clone + Eq + Hash>(quota: Quota, clock: &LimiterClock) -> Buckets<Key> {
    governor::RateLimiter::new(quota, DefaultKeyedStateStore::default(), clock.clone())
}

/// `wait` in whole seconds, rounded up: a client told to retry after zero seconds retries at
/// once and is refused again.
pub fn whole_seconds(wait: Duration) -> u64 {
    wait.as_secs() + u64::from(wait.subsec_nanos() > 0)
}

/// The limiter plus the background task that keeps it bounded.
pub struct RateLimiter {
    pub limiter: Arc<Limiter<IpAddr>>,
    /// Aborted at shutdown; the sweep has nothing to flush.
    pub sweeper: JoinHandle<()>,
}

/// Builds a limiter allowing [`BURST_SIZE`] immediate requests, refilling
/// [`REQUESTS_PER_SECOND`] each second.
pub fn build() -> RateLimiter {
    let limiter = Arc::new(Limiter::new(PER_CLIENT));

    let swept = Arc::clone(&limiter);
    let sweeper = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(STALE_KEY_SWEEP_INTERVAL);
        loop {
            ticker.tick().await;
            let tracked = swept.sweep();
            event!(
                name: "rate_limit.sweep.complete",
                Level::DEBUG,
                rate_limit.tracked_clients = tracked,
                "swept stale rate limit buckets",
            );
        }
    });

    RateLimiter { limiter, sweeper }
}

/// Admits or refuses one request against its client's bucket.
pub async fn enforce(
    State(limiter): State<Arc<Limiter<IpAddr>>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(client) = client_ip(&request) else {
        event!(
            name: "rate_limit.key.missing",
            Level::ERROR,
            "request carried no client IP; nginx must set X-Real-IP or X-Forwarded-For",
        );
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Unable to identify client",
        );
    };

    match limiter.decide(&client) {
        Decision::Admitted { remaining } => {
            let mut response = next.run(request).await;
            let headers = response.headers_mut();
            headers.insert(RATE_LIMIT_LIMIT.clone(), HeaderValue::from(BURST_SIZE));
            headers.insert(RATE_LIMIT_REMAINING.clone(), HeaderValue::from(remaining));
            response
        }
        Decision::Refused { wait } => {
            let seconds = whole_seconds(wait);

            let mut response = json_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
            let headers = response.headers_mut();
            headers.insert(RATE_LIMIT_LIMIT.clone(), HeaderValue::from(BURST_SIZE));
            headers.insert(RATE_LIMIT_REMAINING.clone(), HeaderValue::from(0_u32));
            headers.insert(RATE_LIMIT_AFTER.clone(), HeaderValue::from(seconds));
            headers.insert(header::RETRY_AFTER, HeaderValue::from(seconds));
            response
        }
    }
}

/// The client address nginx reported, falling back to the peer. The first parseable
/// `X-Forwarded-For` entry wins, because nginx appends to whatever the client sent
/// only when the chain passes through another proxy first.
fn client_ip(request: &Request) -> Option<IpAddr> {
    let headers: &HeaderMap = request.headers();

    let forwarded_for = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').find_map(|entry| entry.trim().parse().ok()));
    let real_ip = || {
        headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok())
    };
    let peer = || {
        request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip())
    };

    forwarded_for.or_else(real_ip).or_else(peer)
}

fn json_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({ "message": message }).to_string();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::http::Request as HttpRequest;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::*;

    const CLIENT: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 7));

    // Regression: after the host suspended overnight, governor's quanta clock read
    // hours behind the buckets it had written, and every client was refused with a
    // Retry-After of about ten hours until the API restarted.
    #[test]
    fn a_clock_that_jumps_backwards_does_not_lock_every_client_out() {
        let clock = HeldClock::new();
        clock.set(Duration::from_hours(10));
        let limiter = Limiter::with_clock(LimiterClock::Held(clock.clone()), PER_CLIENT);

        for _ in 0..BURST_SIZE {
            assert!(matches!(limiter.decide(&CLIENT), Decision::Admitted { .. }));
        }
        assert!(matches!(limiter.decide(&CLIENT), Decision::Refused { .. }));

        // The night the counter lost: "now" is suddenly ten hours before every bucket.
        clock.set(Duration::ZERO);

        assert!(
            matches!(limiter.decide(&CLIENT), Decision::Admitted { .. }),
            "a refusal for the length of the jump is the clock's fault, not the client's"
        );
    }

    #[test]
    fn an_honest_refusal_is_left_alone() {
        let limiter = Limiter::with_clock(LimiterClock::Held(HeldClock::new()), PER_CLIENT);

        for _ in 0..BURST_SIZE {
            limiter.decide(&CLIENT);
        }

        let Decision::Refused { wait } = limiter.decide(&CLIENT) else {
            panic!("the burst is spent");
        };
        assert!(
            wait <= Duration::from_secs(1) / REQUESTS_PER_SECOND,
            "one refill period at most, found {wait:?}"
        );
    }

    // A limiter keyed by anything else, such as the search's per person, gives each key a
    // bucket of its own at its own rate.
    #[test]
    fn each_key_spends_its_own_budget() {
        let rate = Rate {
            per_second: 1,
            burst: 2,
        };
        let limiter = Limiter::with_clock(LimiterClock::Held(HeldClock::new()), rate);

        for _ in 0..rate.burst {
            assert!(matches!(limiter.decide(&"ada"), Decision::Admitted { .. }));
        }
        assert!(matches!(limiter.decide(&"ada"), Decision::Refused { .. }));
        assert!(
            matches!(limiter.decide(&"grace"), Decision::Admitted { .. }),
            "another key is untouched by the first one's spending"
        );
    }

    #[test]
    fn waits_round_up_to_whole_seconds() {
        assert_eq!(whole_seconds(Duration::ZERO), 0);
        assert_eq!(whole_seconds(Duration::from_millis(1)), 1);
        assert_eq!(whole_seconds(Duration::from_secs(2)), 2);
        assert_eq!(whole_seconds(Duration::from_millis(2001)), 3);
    }

    #[test]
    fn nginx_forwarding_headers_outrank_the_peer_address() {
        let peer = ConnectInfo(SocketAddr::from(([172, 18, 0, 5], 41_000)));
        let from = |headers: &[(&str, &str)]| {
            let mut request = HttpRequest::get("/").body(Body::empty()).expect("request");
            for (name, value) in headers {
                request.headers_mut().insert(
                    HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                    HeaderValue::from_str(value).expect("header value"),
                );
            }
            request.extensions_mut().insert(peer);
            client_ip(&request)
        };

        assert_eq!(
            from(&[
                ("x-forwarded-for", "garbage, 198.51.100.4"),
                ("x-real-ip", "192.0.2.1")
            ]),
            Some("198.51.100.4".parse().expect("ip")),
            "the first parseable forwarded entry wins"
        );
        assert_eq!(
            from(&[("x-real-ip", "192.0.2.1")]),
            Some("192.0.2.1".parse().expect("ip"))
        );
        assert_eq!(from(&[]), Some(peer.0.ip()));
    }

    async fn status(app: &Router) -> StatusCode {
        let request = HttpRequest::get("/")
            .header("x-real-ip", "203.0.113.7")
            .body(Body::empty())
            .expect("request");
        app.clone()
            .oneshot(request)
            .await
            .expect("infallible")
            .status()
    }

    // Regression: the limiter was built with `per_second(REQUESTS_PER_SECOND)`, which
    // tower_governor reads as one refill every ten seconds. A page load after a burst
    // then failed with 429s for ten seconds per request.
    #[tokio::test]
    async fn the_bucket_refills_at_the_documented_rate() {
        // Time held still, so the bucket refills only when the test says time has passed,
        // however long each request takes on the runner.
        let clock = HeldClock::new();
        let limiter = Arc::new(Limiter::with_clock(
            LimiterClock::Held(clock.clone()),
            PER_CLIENT,
        ));
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(limiter, enforce));

        for _ in 0..BURST_SIZE {
            assert_eq!(status(&app).await, StatusCode::OK);
        }
        assert_eq!(status(&app).await, StatusCode::TOO_MANY_REQUESTS);

        // Three and a half refill periods buy three requests, and no more.
        let refill_period = Duration::from_secs(1) / REQUESTS_PER_SECOND;
        clock.set(refill_period * 3 + refill_period / 2);
        for _ in 0..3 {
            assert_eq!(status(&app).await, StatusCode::OK);
        }
        assert_eq!(status(&app).await, StatusCode::TOO_MANY_REQUESTS);
    }
}
