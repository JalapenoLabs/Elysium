# IP geolocation

The session and sign-in views turn a raw client IP into a coarse **city and
country** so a user can recognize an unfamiliar sign-in at a glance. The backend
owns the lookup (`backend/src/geoip.rs`); the frontend renders the location when
present and the raw IP otherwise.

## How it works

- **Server-side, cached in Redis.** The backend resolves an IP through a free
  geolocation endpoint and caches the result in Redis keyed by IP, so a repeated
  address costs no external call and the provider's rate limit is respected.
  Redis's own TTL expires entries (30 days for a hit, 1 day for a miss), so a
  re-homed address refreshes with no manual invalidation.
- **Never blocks a response.** A cache hit returns immediately. A cache miss
  returns no location (the client shows the raw IP) and resolves in the
  background, so the next read shows the city. Listing sessions or sign-ins never
  waits on the network.
- **Graceful and optional.** With Redis unconfigured or unreachable, or with the
  feature disabled, the lookup returns nothing and the UI falls back to the IP.
  Nothing about the pages depends on geolocation succeeding.
- **Local addresses stay local.** Private, loopback, and link-local addresses are
  never sent to the provider and resolve to no location.

## The response

`GET /api/v1/me/security-activity` returns the `elysium.security.v1.SecurityActivity`
protobuf, in which every `Session` and `SignInEvent` carries a `location` alongside
its `ip`. Each field is optional: an unresolved address omits `location` entirely,
and a provider that resolves only part of an address omits the pieces it lacks.

```proto
message Location {
  optional string city = 1;
  optional string country = 2;
  optional string country_code = 3;
}
```

The UI renders the raw IP whenever `location` is absent.

## Provider

The default endpoint is `https://ipwho.is/` (free, keyless, HTTPS, returns
`success` / `city` / `country` / `country_code`). The IP is appended directly, so
the endpoint must end with a slash. A different provider set via
`ELYSIUM_GEOIP_ENDPOINT` must return the same JSON shape.

## Configuration

| Variable | Default | Meaning |
|----------|---------|---------|
| `ELYSIUM_GEOIP_ENABLED` | `true` | `false` disables lookups; every IP stays local. |
| `ELYSIUM_GEOIP_ENDPOINT` | `https://ipwho.is/` | Provider base URL (ipwho.is-compatible, trailing slash). |
| `REDIS_URL` | (from Compose) | The cache. Without it, geolocation is inert. |

## Privacy

Geolocation sends the client IP to a third-party service. That is a deliberate
tradeoff for a convenience feature, off with a single switch. The roadmap is a
local **MaxMind GeoLite2** database so the lookup runs entirely in-process with no
external call; it drops in behind the same `geoip::locate` contract without
touching callers.

## Redis

This is the first consumer of the deployment's Redis cache. The backend holds one
reconnecting `ConnectionManager` (`AppState::redis`), attached at startup and
absent when Redis is unreachable. Future ephemeral state (rate-limit counters,
short-lived tokens) can share the same handle.
