# API

Rust binary in `api/`, built on axum and tokio. Every route is mounted under `/api` because nginx forwards that
prefix unchanged. Health and build routes sit at the top level. Resource routes are versioned under `/api/v1`.

## Routes

| Method | Path                                  | Response                                            |
|--------|---------------------------------------|-----------------------------------------------------|
| GET    | `/api/ok`                             | `200` text `ok`                                     |
| GET    | `/api/ping`                           | `200` text `pong`                                   |
| GET    | `/api/version`                        | `200` JSON, see below                               |
| GET    | `/api/v1/events`                      | Server-sent event stream, see `docs/realtime.md`    |
| GET    | `/api/v1/llms`                        | `200` `{ llms: Llm[] }`, in priority order          |
| POST   | `/api/v1/llms`                        | `201` `{ llm }`                                     |
| GET    | `/api/v1/llms/{id}`                   | `200` `{ llm }`                                     |
| PATCH  | `/api/v1/llms/{id}`                   | `200` `{ llm }`                                     |
| DELETE | `/api/v1/llms/{id}`                   | `204`                                               |
| GET    | `/api/v1/projects`                    | `200` `{ projects: Project[] }`, by name            |
| POST   | `/api/v1/projects`                    | `201` `{ project }`                                 |
| PATCH  | `/api/v1/projects/{id}`               | `200` `{ project }`                                 |
| DELETE | `/api/v1/projects/{id}`               | `204`; `409` while sessions belong to it            |
| GET    | `/api/v1/satellites`                  | `200` `{ satellites: Satellite[] }`, by name        |
| POST   | `/api/v1/satellites`                  | `201` `{ satellite }`                               |
| GET    | `/api/v1/satellites/{id}`             | `200` `{ satellite }`                               |
| PATCH  | `/api/v1/satellites/{id}`             | `200` `{ satellite }`                               |
| DELETE | `/api/v1/satellites/{id}`             | `204`; forgets its sessions                         |
| POST   | `/api/v1/satellites/{id}/test`        | `200` `{ result }` from a fresh connection          |
| GET    | `/api/v1/coding-sessions`             | `200` `{ sessions: CodingSession[] }`, newest first |
| POST   | `/api/v1/coding-sessions`             | `201` `{ session }`; opens a thread                 |
| PATCH  | `/api/v1/coding-sessions/{id}`        | `200` `{ session }`; renames                        |
| DELETE | `/api/v1/coding-sessions/{id}`        | `204`; destroys the thread                          |
| GET    | `/api/v1/coding-sessions/{id}/events` | `200` `{ events: SessionEvent[], truncated }`       |
| POST   | `/api/v1/coding-sessions/{id}/turns`  | `201` `{ turn }`; queues a prompt                   |

Each route lives in its own file under `api/src/routes/`, and the directory mirrors the URL.

### `/api/version`

Returns the crate name and version, build profile, `rustc` version, build timestamp, and git metadata. The git
metadata is the current branch, `HEAD`, and the last 20 commits, each with hash, short hash, author, ISO 8601
date, and subject. `api/build.rs` captures all of it at compile time. A build with no git history reports an empty
history rather than failing.

### `/api/v1/llms`

An `Llm` has `id`, `name`, `description`, `type`, `priority`, `isActive`, `expiresAt`, `createdAt`, and
`updatedAt`. The secret token is never returned.

`type` is one of `chatgpt-oauth`, `chatgpt-api-token`, `claude-api-token`, or `claude-code-oauth`.

`POST` requires `name`, `type`, and `secretToken`. It accepts optional `description` (default empty), `priority`
(default 0, lower is tried first), `isActive` (default true), and `expiresAt`.

`PATCH` accepts any subset of the same fields. Omitted fields are unchanged. `expiresAt: null` clears the expiry.
A new `secretToken` is re-sealed. An empty body is rejected.

Timestamps sent to the API must include an offset. Responses are always UTC with a `Z` suffix.

### `/api/v1/projects`

A `Project` has `id`, `name`, `description`, `createdAt`, and `updatedAt`. `POST` requires `name` (1 to 120
characters, unique) and accepts `description` (up to 2000, default empty). `PATCH` accepts either; an empty body
is rejected. `DELETE` answers `409` while any coding session belongs to the project.

### `/api/v1/satellites`

A `Satellite` has `id`, `name`, `description`, `url`, `isActive`, `createdAt`, `updatedAt`, and `status`. The
secret is never returned. `status` is the fleet's latest poll, or null for an inactive satellite and until the
first poll after a change: `{ satelliteId, reachable, version, runningThreads, maxConcurrentThreads, error }`.

`POST` requires `name`, `url` (http or https), and `secret`, and accepts `description` and `isActive` (default
true). `PATCH` accepts any subset; a new `secret` is re-sealed. Saving restarts the satellite's watchers.

`test` answers `{ version, protoMajor, protoMinor, runningThreads, maxConcurrentThreads }`, or `502` with the
satellite's error.

### `/api/v1/coding-sessions`

A `CodingSession` has `id`, `projectId`, `satelliteId`, `threadId`, `title`, `createdAt`, `updatedAt`, and
`thread`: the thread as of the latest poll, or null until the first poll sees it. `thread` holds `state`,
`queueDepth`, `currentTurnId`, `latestSequence`, `lastActivityAt`, and `expiresAt`. `state` is one of `unknown`,
`provisioning`, `idle`, `running`, `awaiting-input`, `watching`, `paused`, `expired`, or `destroyed`.

`POST` requires `projectId`, `satelliteId`, and `title` (1 to 200 characters), and accepts `repositoryUrl` and
`baseBranch`. The satellite must be active (`409` otherwise). `turns` requires `prompt` (1 to 100,000 characters)
and answers `{ turnId, status, prompt, queuedAt }`; the turn's progress arrives on the event stream.

A `SessionEvent` has `sessionId`, `sequence`, `turnId`, `occurredAt`, `type` (the satellite's wire name, such as
`agent.message`), `memberId`, and `payload`. `payload` is null for event types the API does not render; otherwise
its `kind` is one of `agentMessage`, `agentThinking`, `toolStarted`, `toolCompleted`, `turnStarted`,
`turnCompleted`, `planProposed`, `questionAsked`, `budgetWarning`, or `incident`. Field shapes are in
`api/src/fleet/views.rs`. Behavior, policy, and history limits are in `docs/coding.md`.

### Errors

Every error is JSON with a `message`.

| Status | Cause                                                                                 |
|--------|---------------------------------------------------------------------------------------|
| `400`  | Malformed JSON, unknown field, unknown enum value, bad UUID, blank or oversized token |
| `404`  | No row with that id                                                                   |
| `409`  | Unique constraint, such as a duplicate LLM name, or a project that still has sessions |
| `422`  | Field validation failed; `fields` lists each failure                                  |
| `502`  | A satellite refused or could not be reached; `message` is the satellite's error       |
| `500`  | Internal fault; details are logged, never returned                                    |

## Commands

With no subcommand the binary serves HTTP.

| Command                               | Purpose                                   |
|---------------------------------------|-------------------------------------------|
| `elysium-api serve`                   | Serve HTTP (the default)                  |
| `elysium-api migrate <action>`        | Manage migrations, see `docs/database.md` |
| `elysium-api generate-encryption-key` | Print a new `ELYSIUM_ENCRYPTION_KEY`      |

## Startup and shutdown

On start, the server loads an environment file if one is present, installs tracing, and parses `Config` from the
environment. It then validates the encryption key, connects to Postgres, refuses to continue if any migration is
pending, connects to Redis, starts the fleet watchers (see `docs/coding.md`), binds, and serves. Both store
connections retry with exponential backoff, 8 attempts over roughly a minute, and prove themselves with `SELECT 1`
and `PING`.

On `SIGTERM` or `SIGINT`, a shutdown token is cancelled first. That ends every open event stream and fleet
watcher, which would otherwise keep the drain waiting forever. The listener stops accepting, in-flight requests
drain, the watchers are awaited, then the Postgres pool closes and the Redis connection drops. Compose allows 30
seconds for this before `SIGKILL`.

## Configuration

| Variable                   | Default          | Purpose                                    |
|----------------------------|------------------|--------------------------------------------|
| `DATABASE_URL`             | required         | Postgres connection string                 |
| `REDIS_URL`                | required         | Redis connection string                    |
| `ELYSIUM_ENCRYPTION_KEY`   | required         | Base64 32-byte key sealing stored secrets  |
| `HOST` / `PORT`            | `0.0.0.0` / 8080 | Bind address                               |
| `DATABASE_MAX_CONNECTIONS` | 10               | Pool ceiling                               |
| `CORS_ALLOWED_ORIGINS`     | empty            | Comma-separated origins; empty allows none |
| `REQUEST_TIMEOUT_SECONDS`  | 30               | Handler deadline, answers `408`            |
| `MAX_REQUEST_BODY_BYTES`   | 1048576          | Body cap, answers `413`                    |
| `RUST_LOG`                 | `info,...`       | tracing filter                             |
| `LOG_FORMAT`               | compact          | `json` for one JSON object per line        |

Connection strings and the key are held as `SecretString`, so they never appear in debug output.

Rate limits are deliberately not configurable. The limit is 10 requests per second per client IP, with a burst
of 30. Both are constants in `src/middleware/rate_limit.rs`.

## Code layout

| Path                     | Holds                                            |
|--------------------------|--------------------------------------------------|
| `src/server.rs`          | Startup, middleware stack, shutdown              |
| `src/routes/`            | One file per route                               |
| `src/models/`            | Diesel records and the queries for each table    |
| `src/database/`          | Pool type, migration runner, generated schema    |
| `src/crypto.rs`          | Secret sealing                                   |
| `src/errors.rs`          | `ApiError` and its mapping to HTTP responses     |
| `src/state.rs`           | `AppState`: pool, Redis, cipher, version, bus, fleet, shutdown token |
| `src/realtime.rs`        | Event bus and the `ServerEvent` envelope          |
| `src/fleet/`             | Satellite clients and watchers; JSON views of Arsox types |

## Logging

Logs are structured `tracing` events with dotted names such as `connection.open.success` and
`migration.change.success`, using OpenTelemetry attribute names. Every request gets an `x-request-id`, either
client-supplied or a generated UUID. The id is echoed on the response and attached to the request span.

## Testing

`cargo test` runs the hermetic unit tests: encryption, request parsing and validation, event envelopes, and the
Arsox view conversions.
`api/scripts/verify-migrations.sh` also runs the database-backed tests against a disposable Postgres.

## Roadmap

- Authentication. The LLM and satellite routes write credentials and drive agents, so nginx publishes on loopback
  only until auth exists.
- TLS to Postgres for deployments where the database is not on a private network.
