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
| GET    | `/api/v1/mail/capabilities`           | `200` `{ capabilities }`                            |
| GET    | `/api/v1/mail/server`                 | `200` `{ server }`                                  |
| POST   | `/api/v1/mail/server`                 | `202` `{ server }`; starts creating the mail server |
| GET    | `/api/v1/mail/domains`                | `200` `{ domains: MailDomain[] }`, by name          |
| POST   | `/api/v1/mail/domains`                | `201` `{ domain }`                                  |
| DELETE | `/api/v1/mail/domains/{id}`           | `204`; removes the domain and its DKIM keys         |
| GET    | `/api/v1/mail/domains/{id}/dns`       | `200` `{ records, zoneFile }`, checked live         |
| GET    | `/api/v1/mail/accounts`               | `200` `{ accounts: MailAccount[] }`, by address     |
| POST   | `/api/v1/mail/accounts`               | `201` `{ account }`; creates a self-hosted mailbox  |
| PATCH  | `/api/v1/mail/accounts/{id}`          | `200` `{ account }`                                 |
| DELETE | `/api/v1/mail/accounts/{id}`          | `204`; destroys a self-hosted mailbox and its mail  |
| POST   | `/api/v1/mail/accounts/{id}/test`     | `200` `{ account }` with the check recorded         |
| POST   | `/api/v1/mail/accounts/{id}/test-message` | `200` `{ sentTo }`                              |
| GET    | `/api/v1/mail/oauth/{kind}/start`     | `303` to the OAuth broker                           |
| GET    | `/api/v1/mail/oauth/callback`         | `303` to `/settings/email`                          |
| GET    | `/api/v1/github-credentials`          | `200` `{ credentials: GithubCredential[] }`, by name |
| POST   | `/api/v1/github-credentials`          | `201` `{ credential }`; checked with GitHub first   |
| GET    | `/api/v1/github-credentials/{id}`     | `200` `{ credential }`                              |
| PATCH  | `/api/v1/github-credentials/{id}`     | `200` `{ credential }`                              |
| DELETE | `/api/v1/github-credentials/{id}`     | `204`; the token itself stays valid on GitHub       |
| POST   | `/api/v1/github-credentials/{id}/test` | `200` `{ result }` from asking GitHub now          |
| POST   | `/api/v1/github-credentials/{id}/repository-access` | `200` `{ result }` for one repository |
| GET    | `/api/v1/projects`                    | `200` `{ projects: Project[] }`, by name            |
| POST   | `/api/v1/projects`                    | `201` `{ project }`                                 |
| PATCH  | `/api/v1/projects/{id}`               | `200` `{ project }`                                 |
| DELETE | `/api/v1/projects/{id}`               | `204`; `409` while sessions belong to it            |
| GET    | `/api/v1/projects/{id}/cover`         | `200` WebP image; `404` without a cover             |
| PUT    | `/api/v1/projects/{id}/cover`         | `200` `{ project }`; the body is the image file     |
| DELETE | `/api/v1/projects/{id}/cover`         | `200` `{ project }`                                 |
| GET    | `/api/v1/satellites`                  | `200` `{ satellites: Satellite[] }`, by name        |
| POST   | `/api/v1/satellites`                  | `201` `{ satellite }`                               |
| GET    | `/api/v1/satellites/{id}`             | `200` `{ satellite }`                               |
| PATCH  | `/api/v1/satellites/{id}`             | `200` `{ satellite }`                               |
| DELETE | `/api/v1/satellites/{id}`             | `204`; forgets its sessions                         |
| POST   | `/api/v1/satellites/{id}/test`        | `200` `{ result }` from a fresh connection          |
| GET    | `/api/v1/storage-locations`           | `200` `{ locations: StorageLocation[] }`, by name   |
| POST   | `/api/v1/storage-locations`           | `201` `{ location }`                                |
| GET    | `/api/v1/storage-locations/{id}`      | `200` `{ location }`                                |
| PATCH  | `/api/v1/storage-locations/{id}`      | `200` `{ location }`                                |
| DELETE | `/api/v1/storage-locations/{id}`      | `204`; files already saved stay with the provider   |
| POST   | `/api/v1/storage-locations/{id}/test` | `200` `{ result }` from listing its directory       |
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

### `/api/v1/mail`

A `MailAccount` has `id`, `kind` (`gmail`, `outlook`, `self-hosted`), `address`, `displayName`, `isActive`,
`lastCheckedAt`, `lastError`, `createdAt`, `updatedAt`, and `mailDomainId` (null for OAuth accounts). The
credential is never returned.

`capabilities` is `{ brokerConfigured, brokerError, oauthKinds }`, asking the broker live.

A mail server is `{ state, hostname, step, error }`. `state` is `not-created`, `creating` (with `step`: `preparing`,
`pulling-image`, `starting`, `configuring`, `restarting`, `adding-domain`), `failed` (with `error`), `ready`, or
`unreachable` (with `error`). `POST /server` requires `hostname` and `domain`, both domain names, and answers `409`
when a server exists or is being created; progress arrives as `mailServer.updated` events.

A `MailDomain` has `id`, `name`, `isDefault`, and `createdAt`. `POST /domains` requires `name` and answers `409` when
it exists. `DELETE` answers `409` for the default domain or one with mailboxes. `GET /domains/{id}/dns` answers
`records`, each `{ purpose, recordType, name, value, status, found, error }` as described in `docs/mail.md`, and the
full `zoneFile`. Every domain route answers `503` when no mail server exists.

`POST /accounts` creates a self-hosted mailbox. It requires `localPart` and `domainId` and accepts `displayName`; it
answers `400` for an unknown domain, `503` when no mail server exists, and `409` when the address exists. `PATCH`
accepts `displayName` and `isActive`. `test` answers the account with `lastError` set or cleared; a failing mailbox
is not an error status. `test-message` answers `502` with the server's message when sending fails. The OAuth routes
are browser navigations; see `docs/mail.md`.

### `/api/v1/projects`

A `Project` has `id`, `name`, `description`, `createdAt`, `updatedAt`, `coverUpdatedAt` (null without a cover), and
`coverFit` (`fit` or `fill`, how clients frame the cover), and `github`: `{ access, credentialId }`, how the project
picks its sessions' GitHub token. `access` is `default` (the workspace default), `none`, or `specific` with the token's
`credentialId`; `specific` with a null id means the chosen token was deleted and the project follows the default.
`POST` requires `name` (1 to 120 characters, unique) and accepts `description` (up to 2000, default empty). `PATCH`
accepts any of `name`, `description`, `coverFit`, and `github`; an empty body is rejected. A `github` whose
`credentialId` is missing for `specific`, present for another access, or names no token answers `400`. `DELETE` answers `409` while any coding session belongs to the project.

`PUT /{id}/cover` takes the image file as the raw body: PNG, JPEG, WebP, or GIF (first frame), up to 10,000,000
bytes; this route alone raises the API's 1 MiB body limit, and nginx's, to allow it. The format is read from the bytes,
not the content type. Decoding refuses images over 8192 pixels on a side or 256 MiB of pixels. The image is scaled down
to fit 2400 by 1350 when larger, keeping its shape, and stored as lossy WebP at quality 80, keeping transparency and
dropping metadata (`api/src/images.rs`). Anything refused answers `400`; a larger body answers `413`.
`GET /{id}/cover` serves it with `Cache-Control: private, max-age=31536000, immutable`: clients add `coverUpdatedAt` to
the URL, so each cover version has its own. Cover changes publish `project.upserted`.

### `/api/v1/satellites`

A `Satellite` has `id`, `name`, `description`, `url`, `isActive`, `createdAt`, `updatedAt`, and `status`. The
secret is never returned. `status` is the fleet's latest poll, or null for an inactive satellite and until the
first poll after a change: `{ satelliteId, reachable, version, runningThreads, maxConcurrentThreads, error }`.

`POST` requires `name`, `url` (http or https), and `secret`, and accepts `description` and `isActive` (default
true). `PATCH` accepts any subset; a new `secret` is re-sealed. Saving restarts the satellite's watchers.

`test` answers `{ version, protoMajor, protoMinor, runningThreads, maxConcurrentThreads }`, or `502` with the
satellite's error.

### `/api/v1/storage-locations`

A `StorageLocation` has `id`, `name`, `provider`, `pathPrefix`, `storageLimitBytes`, `projects`, `createdAt`, and
`updatedAt`. `projects` is `"*"` for every project, or an array of project ids.
The access key is never returned. `provider` is one of:

- `{ kind: "bunny", zone, region }`, with `region` one of `frankfurt`, `london`, `new-york`, `los-angeles`,
  `singapore`, `stockholm`, `sao-paulo`, `johannesburg`, or `sydney`.
- `{ kind: "s3", service, bucket, region, accessKeyId }`, with `service` either `aws` (`region` required, such as
  `us-east-1`) or `google-cloud` (`region` null).

`POST` requires `name` (1 to 120 characters, unique), `provider`, and `accessKey`, and accepts `pathPrefix` (default
the root; surrounding slashes are dropped), `storageLimitBytes` (1 to 2^53 - 1; absent or `null` for no limit), and
`projects` (default none). An unknown project id answers `400`.
`PATCH` accepts any subset; `provider` replaces all of the provider's settings, `projects` replaces the projects,
`storageLimitBytes: null` removes the limit, and a new `accessKey` is re-sealed. A `provider` with another Bunny zone,
S3 service, or access key id answers `400` unless it comes with its `accessKey`.

`test` answers `{ entries, hasMore }`: the files and directories directly inside the location's directory, up to one
page, and whether it holds more. It answers `502` with the provider's error. See `docs/storage.md`.

### `/api/v1/github-credentials`

A `GithubCredential` has `id`, `name`, `kind` (`classic` or `fine-grained`), `login`, `scopes`, `tokenExpiresAt`,
`checkedAt`, `createdAt`, and `updatedAt`. Everything from `login` on is what GitHub answered when the token was last
checked. The token is never returned.

`POST` requires `name` (1 to 120 characters, unique), `kind`, and `token` (up to 255 characters, surrounding
whitespace dropped). `PATCH` accepts any subset of `name`, `kind`, and `token`; a new token is re-sealed. A token
written the wrong way for its kind answers `400` without a call to GitHub, and a `kind` that differs from the stored
one without a `token` answers `400`.

Every write checks the token with `GET https://api.github.com/user` and stores it only if GitHub accepts it: a
rejected token answers `400` with what to check, and an unreachable GitHub `502`.

`isDefault` marks the workspace default. `PATCH` accepts `isDefault`: `true` replaces the previous default in the same
transaction, and both credentials go out on the event stream; `false` leaves no default.

`test` answers `{ login, scopes, tokenExpiresAt }` from asking GitHub now, and records it on the credential.
`repository-access` takes `{ repositoryUrl }` for a github.com remote and answers
`{ repository, canRead, canPush, isPrivate }`, with `canPush` null when unknown. See `docs/github.md`.

### `/api/v1/coding-sessions`

A `CodingSession` has `id`, `projectId`, `satelliteId`, `threadId`, `title`, `createdAt`, `updatedAt`, and
`thread`: the thread as of the latest poll, or null until the first poll sees it. `thread` holds `state`,
`queueDepth`, `currentTurnId`, `latestSequence`, `lastActivityAt`, and `expiresAt`. `state` is one of `unknown`,
`provisioning`, `idle`, `running`, `awaiting-input`, `watching`, `paused`, `expired`, or `destroyed`.

`POST` requires `projectId`, `satelliteId`, and `title` (1 to 200 characters), and accepts `repositoryUrl` and
`baseBranch`, and `githubCredentialId` (absent follows the project, `null` asks for no token, and an id names one;
an unknown id answers `400`). Sessions carry `githubCredentialId`, the token their thread started with. The satellite must be active (`409` otherwise). `turns` requires `prompt` (1 to 100,000 characters)
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
| `502`  | A satellite, mail server, storage provider, or the OAuth broker refused; `message` is its own error |
| `503`  | The mail service a request needs is not configured, or the mail server does not exist |
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
| `OAUTH_BROKER_URL`         | empty            | OAuth broker for browsers; empty disables Gmail and Outlook |
| `OAUTH_BROKER_INTERNAL_URL`| `OAUTH_BROKER_URL` | OAuth broker as the API reaches it       |
| `DOCKER_URL`               | `http://docker-proxy:2375` | Docker API the mail server runs through |
| `MAIL_INGRESS_ADDRESS`     | empty            | nginx's address on the mail network; Stalwart trusts its PROXY headers |

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
| `src/state.rs`           | `AppState`: pool, Redis, cipher, version, bus, fleet, mail, storage, shutdown token |
| `src/realtime.rs`        | Event bus and the `ServerEvent` envelope          |
| `src/fleet/`             | Satellite clients and watchers; JSON views of Arsox types |
| `src/mail/`              | IMAP and SMTP transport, OAuth broker client, mail server hosting and administration, DNS checks |
| `src/storage/`           | Storage provider clients, reached through `Storage` |

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
