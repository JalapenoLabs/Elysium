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
- Every thread declares the satellite's Blender MCP server (`blender`, `http://127.0.0.1:9877/`), which satellites
  built from Arsox's Blender image run on loopback, so agents can 3D model whenever they need. See `docs/coding.md`.

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

## Jira

- Elysium holds any number of Jira Cloud credentials, managed under Settings, Jira. A credential is a site, an
  account email, and an API token sealed in Postgres. Jira Data Center is not supported. See `docs/jira.md`.
- A credential names the projects and boards it may touch, or `*` for all, stored the way storage locations store
  their projects. The user picks from what Jira reports their token can reach; no key or id is ever typed.
- Every call is bounded by that list in one place, `api/src/routes/v1/jira_credentials/allowlist.rs`: an issue
  key's project is checked before the call, the project Jira answers with is checked after it, and a search is
  bounded by rewriting its JQL rather than filtering results. Outside the list answers `403`.
- JQL stays free-form, so the list bounds what is returned, read, and written, never what Jira evaluates on the way
  there. It is a guardrail over a token that can already read everything it reaches, not a security boundary. See
  `docs/jira.md`.
- `api/src/jira/` is the only caller. Each credential names its own site, so the host is validated rather than
  fixed in code: https on `.atlassian.net`, stored as the origin alone.
- Issue text is Atlassian Document Format. Elysium takes plain text and wraps it, and returns both the stored
  document and a plain rendering.

## Environment variables

- Global environment variables, managed under Settings, Environment variables, are passed into every coding session's
  satellite thread. See `docs/environment.md`.
- Every value is sealed in Postgres, secret or not. Secret means Elysium never returns the value and the satellite
  scrubs it from thread output; the agent can always read it, so nothing that must stay hidden from the agent belongs
  here.
- Keys that Elysium sets, the satellite refuses, or the satellite overrides are refused, by one rule table in
  `api/src/environment/` that the frontend mirrors.

## Action items

The core and its frontend are built: items, initiatives, memberships, comments, history, Next, and progress, under
`/api/v1/action-items` and `/api/v1/initiatives`, and the Action items area that uses them. So are the `elysium_work`
tools, coding sessions started from an item, and links to Jira and GitHub with the watcher and their frontend. So are
changesets under `/api/v1/changesets`, the `work_propose_changes` tool, and their review in the Action items area.

- Action items are the one list of what the user owes attention to, from Jira, GitHub, email, meetings, or typed by
  hand. Initiatives group items toward a goal that ends and carry the progress bar; projects never end and have
  none. Items belong to many projects and many initiatives. See `docs/action-items.md`.
- An item is a commitment, not a copy. The provider stays the source of truth for a linked issue's fields; the item
  holds triage, priority, dates, membership, provenance, and history. Resolving an item moves every linked issue
  still open automatically, and the provider closing a linked issue resolves its item. A pull request is never
  merged or closed from Elysium.
- Progress is resolved over total with a burnup of both, never a percentage alone. Initiatives link to containers
  (a Jira epic or search, a GitHub milestone or label) rather than issues one by one, so nothing is tracked twice.
- Anything the user did not do directly (Elysia, Elysium's AI assistant; email triage; coding agents) proposes a
  changeset. Nothing is written in Elysium or any provider until the user approves it, wholly or in part. The
  watcher only records what already happened in a provider, so it needs no changeset.
- A changeset's operations are decided one by one; rejecting one rejects what depends on it. Applying runs the
  approved ones in order as the proposer, each in its own savepoint, so one that fails is marked with the reason and
  the rest apply; provider writes go through the same outbox as the user's. The history it records names the
  changeset, so it can be undone as a whole, never overwriting what the user changed since. What already left Elysium
  (a posted comment, a closed issue) is not reversed, and the review says so. Rules and queries:
  `api/src/action_items/changesets.rs` and `api/src/models/changeset.rs`.
- The watcher polls providers for changes; webhooks come later. Link providers sit behind one trait
  (`api/src/action_items/links/`), so routes and tools never match on the provider. Every Jira call goes through the
  credential's allowlist, as the Jira routes' calls do.
- The watcher acts on a change of what a link last recorded, never on the provider's state alone, and its interval is a
  constant in code. Its cursor per credential is persisted, and it stops with the shutdown token.
- Provider writes (closing a resolved item's issues, posting a comment to the primary link) are owed in the same
  transaction as the change and landed by the watcher. One that fails stays pending on its link, retried every pass,
  until it lands or the user cancels it. The item's own change always stands.
- A Jira project's done transition is stored as the `done` status it leads into, chosen by the user only when the
  project has more than one.
- Coding agents use one relayed MCP server, `elysium_work`, declared on every thread, in Elysium's terms and scoped to
  the session's project, re-checked on every call. Its direct writes are comments and linking the pull request the
  agent opened to its session's item; an agent's link never becomes the primary. Every other change it proposes as a
  changeset with `work_propose_changes`.
- A session started from an item records it, and its first turn carries the item's context ahead of the user's
  prompt.
- There is no users table yet. Ownership and actors are recorded as text and become references when users land.
- Every write records a history entry with its actor in the same transaction, with before and after values so it can
  be shown and undone. Deleting items and initiatives is soft. Next's order and the progress rules are pure functions
  in `api/src/action_items/`.

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
