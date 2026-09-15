// Copyright © 2026 Jalapeno Labs

//! Per-client token-bucket rate limiting.
//!
//! Clients are keyed by IP. Because the API only ever sits behind nginx, the IP
//! comes from the `X-Forwarded-For` / `X-Real-IP` headers nginx sets, falling back
//! to the peer address. Never expose the API port directly: a client that can
//! reach it without nginx could forge those headers and dodge its bucket.
//!
//! # The clock
//!
//! Buckets are timed with [`MonotonicClock`], which is `std::time::Instant` and on
//! Linux `CLOCK_MONOTONIC`. That clock pauses while the host is suspended, so a bucket
//! is exactly as full after a night asleep as it was before. governor's default quanta
//! clock reads the CPU's time stamp counter instead, which does not hold across a
//! suspend: on waking, every stored bucket sat hours ahead of "now", and every client,
//! including ones that had never called, was refused with a `Retry-After` the length
//! of the night. `tower_governor` fixes that clock in its types, so this module drives
//! governor directly.
//!
//! A refusal asking for longer than an empty bucket takes to refill is one no working
//! clock can produce, so the limiter treats it as a clock that moved underneath it:
//! it logs, starts every bucket afresh, and admits the request.

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use governor::Quota;
use governor::clock::{Clock, MonotonicClock};
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
const BURST_SIZE: u32 = 30;

/// Buckets for clients that stopped talking are dropped on this cadence so the
/// key map cannot grow without bound under a scan.
const STALE_KEY_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

const LOCK_POISONED: &str = "rate limit buckets lock poisoned";

static RATE_LIMIT_LIMIT: HeaderName = HeaderName::from_static("x-ratelimit-limit");
static RATE_LIMIT_REMAINING: HeaderName = HeaderName::from_static("x-ratelimit-remaining");
static RATE_LIMIT_AFTER: HeaderName = HeaderName::from_static("x-ratelimit-after");

type Buckets<C> =
    governor::RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, C, StateInformationMiddleware>;

/// What the limiter decided about one request.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Admitted { remaining: u32 },
    Refused { wait: Duration },
}

/// Every client's bucket, and the clock they are measured against.
pub struct Limiter<C: Clock + Clone = MonotonicClock> {
    clock: C,
    quota: Quota,
    /// How long an empty bucket takes to refill. No honest refusal waits longer.
    longest_honest_wait: Duration,
    /// Behind a lock only so a clock that moved can be answered by starting over.
    buckets: RwLock<Arc<Buckets<C>>>,
}

impl<C: Clock + Clone> Limiter<C> {
    fn with_clock(clock: C) -> Self {
        let refill_period = Duration::from_secs(1) / REQUESTS_PER_SECOND;
        let quota = Quota::with_period(refill_period)
            .expect("REQUESTS_PER_SECOND is a non-zero constant")
            .allow_burst(NonZeroU32::new(BURST_SIZE).expect("BURST_SIZE is a non-zero constant"));

        Self {
            buckets: RwLock::new(Arc::new(fresh_buckets(quota, &clock))),
            clock,
            quota,
            longest_honest_wait: refill_period * BURST_SIZE,
        }
    }

    fn decide(&self, client: IpAddr) -> Decision {
        let wait = match self.check(client) {
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

        match self.check(client) {
            Ok(remaining) => Decision::Admitted { remaining },
            Err(wait) => Decision::Refused { wait },
        }
    }

    /// One conformance check: the remaining burst, or how long to wait.
    fn check(&self, client: IpAddr) -> Result<u32, Duration> {
        // Cloned out so the read lock is not held across the check.
        let buckets = Arc::clone(&self.buckets.read().expect(LOCK_POISONED));
        buckets
            .check_key(&client)
            .map(|snapshot| snapshot.remaining_burst_capacity())
            .map_err(|refusal| refusal.wait_time_from(self.clock.now()))
    }

    fn sweep(&self) -> usize {
        let buckets = Arc::clone(&self.buckets.read().expect(LOCK_POISONED));
        buckets.retain_recent();
        buckets.len()
    }
}

fn fresh_buckets<C: Clock + Clone>(quota: Quota, clock: &C) -> Buckets<C> {
    governor::RateLimiter::new(quota, DefaultKeyedStateStore::default(), clock.clone())
}

/// The limiter plus the background task that keeps it bounded.
pub struct RateLimiter {
    pub limiter: Arc<Limiter>,
    /// Aborted at shutdown; the sweep has nothing to flush.
    pub sweeper: JoinHandle<()>,
}

/// Builds a limiter allowing [`BURST_SIZE`] immediate requests, refilling
/// [`REQUESTS_PER_SECOND`] each second.
pub fn build() -> RateLimiter {
    let limiter = Arc::new(Limiter::with_clock(MonotonicClock));

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
    State(limiter): State<Arc<Limiter>>,
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

    match limiter.decide(client) {
        Decision::Admitted { remaining } => {
            let mut response = next.run(request).await;
            let headers = response.headers_mut();
            headers.insert(RATE_LIMIT_LIMIT.clone(), HeaderValue::from(BURST_SIZE));
            headers.insert(RATE_LIMIT_REMAINING.clone(), HeaderValue::from(remaining));
            response
        }
        Decision::Refused { wait } => {
            // Rounded up: a client told to retry after zero seconds retries at once and
            // is refused again.
            let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);

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
    use std::sync::atomic::{AtomicU64, Ordering};

    use axum::Router;
    use axum::http::Request as HttpRequest;
    use axum::routing::get;
    use governor::nanos::Nanos;
    use tower::ServiceExt;

    use super::*;

    /// A clock a test can move in either direction, the way a CPU time stamp counter
    /// moves across a suspend.
    #[derive(Clone, Default)]
    struct JumpingClock(Arc<AtomicU64>);

    impl JumpingClock {
        fn set(&self, at: Duration) {
            let nanos = u64::try_from(at.as_nanos()).expect("test instants fit in u64");
            self.0.store(nanos, Ordering::SeqCst);
        }
    }

    impl Clock for JumpingClock {
        type Instant = Nanos;

        fn now(&self) -> Nanos {
            Nanos::from(self.0.load(Ordering::SeqCst))
        }
    }

    const CLIENT: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 7));

    // Regression: after the host suspended overnight, governor's quanta clock read
    // hours behind the buckets it had written, and every client was refused with a
    // Retry-After of about ten hours until the API restarted.
    #[test]
    fn a_clock_that_jumps_backwards_does_not_lock_every_client_out() {
        let clock = JumpingClock::default();
        clock.set(Duration::from_hours(10));
        let limiter = Limiter::with_clock(clock.clone());

        for _ in 0..BURST_SIZE {
            assert!(matches!(limiter.decide(CLIENT), Decision::Admitted { .. }));
        }
        assert!(matches!(limiter.decide(CLIENT), Decision::Refused { .. }));

        // The night the counter lost: "now" is suddenly ten hours before every bucket.
        clock.set(Duration::ZERO);

        assert!(
            matches!(limiter.decide(CLIENT), Decision::Admitted { .. }),
            "a refusal for the length of the jump is the clock's fault, not the client's"
        );
    }

    #[test]
    fn an_honest_refusal_is_left_alone() {
        let limiter = Limiter::with_clock(JumpingClock::default());

        for _ in 0..BURST_SIZE {
            limiter.decide(CLIENT);
        }

        let Decision::Refused { wait } = limiter.decide(CLIENT) else {
            panic!("the burst is spent");
        };
        assert!(
            wait <= Duration::from_secs(1) / REQUESTS_PER_SECOND,
            "one refill period at most, found {wait:?}"
        );
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
        let limiter = build();
        let app = Router::new().route("/", get(|| async { "ok" })).layer(
            axum::middleware::from_fn_with_state(Arc::clone(&limiter.limiter), enforce),
        );

        for _ in 0..BURST_SIZE {
            assert_eq!(status(&app).await, StatusCode::OK);
        }
        assert_eq!(status(&app).await, StatusCode::TOO_MANY_REQUESTS);

        // Three refill periods (plus slack) buy three requests, and no more.
        let refill_period = Duration::from_secs(1) / REQUESTS_PER_SECOND;
        tokio::time::sleep(refill_period * 3 + refill_period / 2).await;
        for _ in 0..3 {
            assert_eq!(status(&app).await, StatusCode::OK);
        }
        assert_eq!(status(&app).await, StatusCode::TOO_MANY_REQUESTS);

        limiter.sweeper.abort();
    }
}
