# Elysium

Rust API (`api/`), Vite/React frontend (`frontend/`), Postgres, and Redis behind one nginx origin, run with
`docker compose`. Decisions are documented per topic in `docs/`; keep them current as the code changes.

## Application secrets live in Postgres, not environment files

`.env` holds only what is needed to bootstrap the stack:

- `ELYSIUM_ENCRYPTION_KEY`
- `POSTGRES_USER`, `POSTGRES_PASSWORD`, and `POSTGRES_DB`
- `REDIS_PASSWORD`
- `RUST_LOG`, which is optional

Everything else follows these rules:

- Application secrets (LLM tokens, OAuth credentials, third-party API keys) are stored in Postgres, sealed with
  the encryption key. They never go in `.env`. See `docs/secrets.md`.
- Non-secret configuration, such as `CORS_ALLOWED_ORIGINS`, is written inline in `compose.yml`.
- Policy values that must not vary per deployment, such as rate limits, are constants in the code, not
  configuration.

## Satellites are reached only through the API

- Arsox satellites run coding sessions. The frontend never contacts one: satellites speak protobuf and hold a
  bearer secret that must not reach a browser. The API is their only client, through `arsox-sdk`.
- Satellite secrets are application secrets, sealed in Postgres like LLM tokens.
- Thread policy (idle TTL, budget ceilings) is code constants, not configuration. See `docs/coding.md`.
- A thread is opened with Elysium's active LLM credentials as an ordered failover stack, highest priority first,
  so a credential that runs out hands the turn to the next one.

## Mail

- Mailboxes are Gmail, Outlook, or self-hosted on Elysium's Stalwart server, all read and sent over one IMAP and
  SMTP transport. Servers are fixed per kind in code. See `docs/mail.md`.
- Gmail and Outlook connect through the OAuth broker in `oauth-broker/`, a stateless, separately deployable
  Apache-2.0 service that holds the OAuth apps so self-hosters need none. Its secrets belong to its own deployment,
  never to Elysium's bootstrap file.
- One Stalwart server hosts any number of mail domains on one set of ports. The API creates and runs it through
  Docker, reached only via the filtered `docker-proxy`, never a mounted socket. It is not a compose service.
- Stalwart's administrator is issued by Stalwart when the API creates the server, and is sealed in Postgres like any
  application secret. It never goes in `.env` or `compose.yml`.
- nginx is the single ingress for web and mail. It passes the mail ports (25, 465, 993) to Stalwart as raw TCP with
  the PROXY protocol, and Stalwart trusts that header from nginx's fixed address alone. The Stalwart container
  publishes no ports of its own.
- What sits in front of the host (a VM's public address, port forwarding, tunnels, reverse DNS, certificates) is
  the operator's responsibility, and Elysium accepts whatever arrives. Elysium provides each domain's DNS records
  and checks them, and never changes DNS or issues certificates.

## Storage

- Storage locations are where Elysium saves files, one row per location with a provider kind, used by the projects
  linked to it or by every project (`*`). Providers are Bunny Storage and S3 buckets on AWS or Google Cloud, with
  every endpoint fixed in code; local storage is planned. See `docs/storage.md`.
- Access keys are application secrets, sealed in Postgres. Routes never match on the provider; `api/src/storage/`
  does.
- Coding agents reach their project's locations through the `elysium_storage` MCP tools, which Elysium answers over a
  relay socket it opens to the satellite; nothing listens for the satellite. Every call re-checks the location against
  the project in the database. See `api/src/tools/` and `docs/storage.md`.

## GitHub

- Elysium holds any number of GitHub personal access tokens, classic or fine-grained, managed under Settings, GitHub.
  Tokens are application secrets, sealed in Postgres. See `docs/github.md`.
- A coding session starts with one token or none, chosen by the workspace default, overridden by the project, and
  overridden again when the session starts. The agent gets it as `GH_TOKEN`, with git routed through `gh` by
  `GIT_CONFIG_*` variables; no satellite change is involved.
- Every write checks the token with GitHub first and stores what it answers: the account, its scopes, and the expiry.
  `api/src/github/` is the only caller, and `api.github.com` is fixed in code; GitHub Enterprise Server is not
  supported.

## Environment variables

- Global environment variables, managed under Settings, Environment variables, are passed into every coding session's
  satellite thread. See `docs/environment.md`.
- Every value is sealed in Postgres, secret or not. Secret means Elysium never returns the value and the satellite
  scrubs it from thread output; the agent can always read it, so nothing that must stay hidden from the agent belongs
  here.
- Keys that Elysium sets, the satellite refuses, or the satellite overrides are refused, by one rule table in
  `api/src/environment/` that the frontend mirrors.

## Action items

Designed, not yet built: none of the tables, routes, watcher, or tools below exist yet. Do not build against them
until their implementation lands.

- Action items are the one list of what the user owes attention to, from Jira, GitHub, email, meetings, or typed by
  hand. Initiatives group items toward a goal that ends and carry the progress bar; projects never end and have
  none. Items belong to many projects and many initiatives. See `docs/action-items.md`.
- An item is a commitment, not a copy. The provider stays the source of truth for a linked issue's fields; the item
  holds triage, priority, dates, membership, provenance, and history. Resolving an item moves its linked issue
  automatically, and the provider closing an issue resolves its item. A pull request is never merged or closed from
  Elysium.
- Progress is resolved over total with a burnup of both, never a percentage alone. Initiatives link to containers
  (a Jira epic or search, a GitHub milestone or label) rather than issues one by one, so nothing is tracked twice.
- Anything the user did not do directly (Elysia, Elysium's AI assistant; email triage; coding agents) proposes a
  changeset. Nothing is written in Elysium or any provider until the user approves it, wholly or in part. The
  watcher only records what already happened in a provider, so it needs no changeset.
- The watcher polls providers for changes; webhooks come later. Link providers sit behind one trait, so routes and
  tools never match on the provider.
- Coding agents use one relayed MCP server, `elysium_work`, in Elysium's terms and scoped to the session's project.
- There is no users table yet. Ownership and actors are recorded as text and become references when users land.

## One event stream keeps the frontend current

- Every page holds one server-sent event stream, `GET /api/v1/events`. Any write or watcher that changes what a
  client shows publishes to it. Components never poll. See `docs/realtime.md`.
- The event bus is in-process, so the API runs as a single replica until the bus moves to Redis pub/sub.

## Time is UTC on the server

- Every timestamp the server stores, computes, logs, or returns is UTC.
- Postgres columns are `TIMESTAMPTZ`, Rust types are `chrono::DateTime<Utc>`, and JSON carries ISO 8601 strings
  ending in `Z`.
- Requests must send an explicit offset. Offset-less timestamps are rejected.
- Timezones exist only in the frontend, which converts to the viewer's zone when rendering.

## Frontend

- Colors come from the Matter VS Code theme and live in `frontend/src/theme/matter.css`. Use theme tokens
  (`bg-surface`, `text-link`, `bg-sidebar`), never hardcoded colors in components.
- HeroUI v3 on Tailwind CSS v4, modeled on Stripe's dashboard layout. v3's compound API differs from v2, so check
  `docs/frontend.md` or HeroUI's v3 docs rather than relying on v2 habits.
- Redux Toolkit holds all global state: server data, the event stream connection, and the theme. Component-local
  state stays in components.
- Server data flows SWR, then Redux, then the event stream: an SWR loader fetches it once and buffers it, Redux is
  the source of truth components render, and the event stream keeps Redux current. See `docs/frontend.md`.
- Every user-facing string goes through i18next. `en-US` is the only locale today.
- Reach for `@jalapenolabs/uikit` first when it covers the need, such as `SmartTable` for data tables. Use HeroUI
  for everything the kit does not provide.
- Linting is the org-wide ruleset from `@jalapenolabs/cli/eslint`. Change rules in the shared package, not
  locally, unless the override is specific to this repo.

## Database changes

- Diesel is the ORM. Schema changes are migrations under `api/migrations/`, each with a `down.sql` that fully
  undoes its `up.sql`.
- `api/src/database/schema.rs` is generated by `diesel print-schema`. Never edit it by hand.
- Run `api/scripts/verify-migrations.sh` after any migration change. It fails on schema drift, a broken rollback,
  or a failing database test.

## CI

- GitHub Actions on the org's self-hosted runners (`[ self-hosted, linux, rocky9, docker ]`). The runners are
  persistent and warm, so workflows never use `actions/cache`. See `docs/ci.md`.
- `main` takes changes through pull requests and a merge queue. Every workflow runs on `pull_request`, `merge_group`,
  and pushes to `main`, with no path filters, so a required check can never be left pending.
- Job names are the required check names. Keep them short, stable, and unique across workflows, and update the list
  in `docs/ci.md` when one changes.
- The repository is public: trigger on `pull_request`, never `pull_request_target`; every job skips pull requests
  from forks; permissions stay `contents: read`; no workflow reads a secret; third-party actions are pinned to a
  commit SHA.
- Tools are pinned in one place and verified: Rust by each crate's `rust-toolchain.toml`, Node by `frontend/.nvmrc`,
  Yarn by `packageManager`, the Diesel CLI by `DIESEL_VERSION` in `api.yml`. Dockerfiles must use the same Rust and
  Node versions; the image checks fail when they drift.
