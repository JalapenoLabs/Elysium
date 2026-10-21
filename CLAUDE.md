# Elysium

Rust API (`api/`), Vite/React frontend (`frontend/`), Postgres, and Redis behind one nginx origin, run with
`docker compose`. Decisions are documented per topic in `docs/`; keep them current as the code changes.

## Direction

The full vision is `docs/vision.md`. Read it before designing any feature.

- Elysium is a personal Jalapeno Labs project. A workplace runs its published releases as a user, updated from the
  latest release, and never contributes to it.
- Elysia should work without being fed. Events (mail, meetings, chat, provider changes) create work, Elysia starts
  it on her own, and she reports back when something is ready for review.
- Claude is the agent. Codex is for occasional automated tasks only.
- Coding sessions run Claude Code directly on Elysium's host. Remote compute for agents is optional and not built.
- Seraphim's autonomous loop (take the next work when idle, fix failing CI, address review, revive dead turns, ask
  when blocked) moves into Elysium redesigned, not ported as is.
- Elysium uses services the way its user does, signed in as that user, and never installs anything into an
  organization. Each service is a connector behind one trait: how it signs in, how it stays signed in, how it
  ingests, and how it acts. An official API, a CLI such as `gh`, and a browser session the user signed in to are
  interchangeable ways of filling those parts.
- Everything ingested lands on one timeline, attributed to projects, so a project shows every message, mail, note,
  agent turn, and meeting segment about it in order. A meeting segment is the trimmed recording, its transcript, and
  its summary.
- Agent logs read like GitHub Actions: each turn collapses to a one or two sentence summary that expands to its
  full log.
- Signing in without an app of Elysium's own happens in a real browser the host runs and streams to the user over
  WebRTC, one container and profile per account. A provider that refuses datacenter sign-ins is handled when met.
- Next on the roadmap: Outlook and Gmail through that browser, so Elysia is told when mail arrives and manages it.

## Images and stacks

- `compose.yml` is the production stack: it runs the published `jalapenolabs/elysium-api` image, which serves both
  `/api` and the web app's production build, so nginx sends every path to the API. `compose.dev.yml` layers the
  development stack over it: the API built from source (`api/Dockerfile` stage `server`) and the Vite dev server,
  with nginx swapped to `nginx/development.conf`. See `docs/infrastructure.md`.
- `.github/workflows/publish.yml` pushes `jalapenolabs/elysium-api`, `jalapenolabs/elysium-workspace`, and
  `jalapenolabs/elysium-oauth-broker` to Docker Hub on every push to `main` and every `v*` tag. A release is a `vX.Y.Z`
  tag. See `docs/ci.md`.
- Agent turns run the official `claude` CLI in the workspace container (`workspace/Dockerfile`), which the API
  creates through `docker-proxy` and reaches by `docker exec`, never inside the API's own container. Callers submit
  turns to `crate::runner`. See `docs/coding.md`.
- Releases update themselves: the API decides when, and the updater (compose profile `auto-update`) carries it out,
  backing up the databases and rolling back a release that does not come up healthy. It never restarts the stack
  while work is running; anything a restart would cut short, such as an agent turn, holds an activity
  (`api/src/activity.rs`) while it runs. Each API image carries the deployment's configuration, so a release brings
  its own, and the updater owns `ELYSIUM_VERSION` in `.env`. See `docs/infrastructure.md`.
- The web app's pages carry a strict Content Security Policy set by the API: no inline scripts, no eval, nothing
  from another origin. Frontend code keeps to it. See `docs/security.md`.

## Application secrets live in Postgres, not environment files

`.env` holds only what is needed to bootstrap the stack:

- `ELYSIUM_ENCRYPTION_KEY`
- `POSTGRES_USER`, `POSTGRES_PASSWORD`, and `POSTGRES_DB`
- `REDIS_PASSWORD`
- `RUST_LOG`, which is optional
- `ELYSIUM_PUBLIC_URL`, which is optional: the origin browsers use, `http://localhost:4000` unless set
- `ELYSIUM_VERSION`, which is optional: the published release the production stack runs, `latest` unless set
- `COMPOSE_PROFILES`, which is optional: `auto-update` turns on the updater
- `WEB_PORT` and `WEB_BIND_ADDRESS`, which are optional: where nginx publishes the web, `127.0.0.1:4000` unless set

Everything else follows these rules:

- Application secrets (LLM tokens, OAuth credentials, third-party API keys) are stored in Postgres, sealed with
  the encryption key. They never go in `.env`. See `docs/secrets.md`.
- Non-secret configuration, such as `CORS_ALLOWED_ORIGINS`, is written inline in `compose.yml`.
- Policy values that must not vary per deployment, such as rate limits, are constants in the code, not
  configuration.

## Accounts live in Ory Kratos

- Kratos (`kratos/`) owns identities: passwords, passkeys, authenticator apps, lookup codes, sessions, and recovery.
  Elysium never re-implements any of it. The frontend renders Kratos's flows through its JSON API at
  `/api/identity/`; the API calls Kratos's admin API alone. See `docs/auth.md`.
- Kratos's configuration is derived from `ELYSIUM_PUBLIC_URL` and `ELYSIUM_ENCRYPTION_KEY` by
  `kratos/entrypoint.sh`. Nothing for Kratos goes in the bootstrap file. Its database, `kratos`, shares Elysium's
  Postgres server and credentials.
- `users` holds every principal: people, mapped to their Kratos identity, and the seeded machines (the system user,
  Elysia, and the coding agent), which can never sign in. Roles are one enum: `admin`, `member`, `guest`, `agent`,
  `system`. Guests have a member's access until roles are built out.
- The first person to sign up becomes the admin; everyone after waits for an admin's approval. Admins can close
  sign-up and require an authenticator app. The workspace always keeps one active admin. Approved people are never
  deleted, only disabled.
- Every `/api/v1` route needs a session, except `/auth/status`; handlers name who they act for with `CurrentUser` or
  `AdminUser`. Unsafe requests must come from Elysium's own origin.
- Every top-level table has `created_by NOT NULL REFERENCES users`, and every new one gets it too. Machines create
  as their seeded user. There is no `updated_by`; history records edits.
- There is no mail for accounts yet: recovery links are logged by the API (`auth.recovery.issued`), or created by an
  admin.

## External MCP clients connect through OAuth

- Elysium's MCP server is at `/api/mcp`, for clients such as Claude Code and Codex. It speaks Streamable HTTP through
  the official Rust SDK (`rmcp`), statelessly. Its tools are in `api/src/mcp/`, one module per group. See `docs/mcp.md`.
- Ory Hydra (`hydra/`) is the OAuth 2.1 authorization server; Kratos still proves who people are, and the web app's
  `/oauth/login` and `/oauth/consent` pages answer Hydra's challenges. Hydra is configured like Kratos, from
  `ELYSIUM_PUBLIC_URL` and `ELYSIUM_ENCRYPTION_KEY`, with its database `hydra` on the shared Postgres.
- Any approved person may connect any client; admins gate sign-up, not OAuth. Clients register themselves.
- Every token is bound to `/api/mcp` and checked on every call: active, granting `workspace:read`, and naming an active,
  approved person. Writing tools also need `workspace:write` and record the person as actor and creator.
- A tool never reimplements a route: it calls the function the route's handler calls, so a change made over MCP is
  validated, recorded, and announced exactly as one made in the browser. Tools exist for projects (read) and coding
  sessions; Studio items, action items, and initiatives are on the roadmap.

## Agents run on the host

- Agent turns run the official Claude Code CLI, on a subscription token or an API key, in the `elysium-workspace`
  container the API owns. The Agent SDK is never used with subscription credentials. See `docs/coding.md`.
- Agents never run in the API's container, and the workspace shares a network with the API alone. Workspace files
  are read and written through `docker exec` as `agent`, confined by `realpath`; the API never mounts the volume.
- A session is a directory in the workspace (`sessions/<id>`), the Claude session its turns resume, and its events in
  `session_events`. Turns of one session run one at a time; at most eight run at once.
- A turn runs on Elysium's active Claude credentials as an ordered failover stack, highest priority first, so a
  credential that runs out hands the turn to the next one, resuming the same Claude session.
- Turn policy (heartbeat, concurrency, clone and script timeouts) is code constants, not configuration.
- Agents' `~/.claude` is checked out from the configuration repository named in the workspace settings.
- Elysium's tools reach agents at `/api/agent-tools/{server}`, under a per-turn grant scoped to the session, and every
  call re-checks the database. nginx never routes that path.
- Every agent can 3D model. Blender is baked into the workspace image, and every turn starts a Blender of its own
  through a stdio MCP launcher, so no two turns share a scene. See `docs/coding.md`, Blender.
- Every turn also has a desktop of its own (virtual display, window manager, Firefox) that it drives with mouse,
  keyboard, and screenshots through the `desktop` MCP server, started and stopped with the turn the same way. See
  `docs/coding.md`, The desktop.
- Secret values given to a turn are scrubbed from its events before they are recorded or published.
- Every completed or failed turn is summarized in one or two sentences by a small model, as a text turn on the runner
  (no tools, no settings, its own conversation), and the conversation collapses each turn to that summary. A summary
  that fails is logged and left out; it never changes the turn.

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
- nginx is the single ingress for web, mail, and browser media. It passes the mail ports (25, 465, 993) to Stalwart
  as raw TCP with the PROXY protocol, and Stalwart trusts that header from nginx's fixed address alone. The Stalwart
  container publishes no ports of its own.
- What sits in front of the host (a VM's public address, port forwarding, tunnels, reverse DNS, certificates) is
  the operator's responsibility, and Elysium accepts whatever arrives. Elysium provides each domain's DNS records
  and checks them, and never changes DNS or issues certificates.

## Remote browser

- Signing in to a service without an app of Elysium's own happens in a real browser the host runs and streams to the
  user over WebRTC: Neko (`ghcr.io/m1k1o/neko/chromium`, pinned by digest), run unchanged, one container, profile
  volume, Docker network, and media port per signed-in account. The API runs and reconciles them; see
  `docs/browser.md`.
- A browser belongs to the person who created it. Its routes answer that person alone, and the stream relays a Neko
  member session, never an administrator's.
- nginx stays the single ingress: it passes the media ports, `59000-59007` over UDP and TCP, to the browser that owns
  each one, on every interface. Signaling never leaves the API's authenticated stream route.
- The web app's sign-in panel (Settings, Browser sign-ins) speaks Neko's protocol itself: Neko's own client cannot
  run under the web app's CSP.
- The API and Elysia reach each browser over the Chrome DevTools Protocol, on that browser's private network only.

## Connectors and the timeline

- A connector is a service Elysium reads as its user, behind one trait (`api/src/connectors/`): it names its kind
  and reads what its account shows now. How it signs in and stays signed in belongs to what it reads through. Today
  Outlook and Gmail read through a person's browser over DevTools, with one page script per service; the check is
  cheap and deterministic, and nothing it does marks mail read. See `docs/connectors.md`.
- Everything ingested is a timeline record (`timeline_records`), unique per account and provider id, attributed to
  projects with a confidence (`timeline_record_projects`). A record is its owner's alone, even about a shared
  project.
- Ingest runs every minute. A new unread message raises an inbox item in the same transaction as its record,
  recorded as `watcher:<service>`: connecting a mailbox is the consent, as linking a container is for the link
  watcher. The first read baselines the mailbox without raising anything.
- `timelineRecord.created` carries ids and kinds, never content, because every client hears every event. It is the
  seam Elysia's triage builds on.

## Storage

- Storage locations are where Elysium saves files, one row per location with a provider kind, used by the projects
  linked to it or by every project (`*`). Providers are Bunny Storage and S3 buckets on AWS or Google Cloud, with
  every endpoint fixed in code; local storage is planned. See `docs/storage.md`.
- Access keys are application secrets, sealed in Postgres. Routes never match on the provider; `api/src/storage/`
  does.
- Coding agents reach their project's locations through the `elysium_storage` MCP tools at `/api/agent-tools`. Every
  call re-checks the location against the project in the database. See `api/src/tools/` and `docs/storage.md`.

## Studio

- Studio (`/studio`) is where agents make single assets: a 3D model, a render, a 2D image, one per item, with no git
  repositories. Coding is for multi-repository, long-lived work with pull requests. See `docs/studio.md`.
- A Studio item runs on one coding session (`coding_sessions.studio_item_id`) whose directory keeps its work; its
  project is optional and its storage location is required. The Coding page lists only sessions without an item.
- Agents deliver files into their session's `artifacts/`. After every turn Elysium exports every `.blend` to `.glb`
  itself, then pulls each new or changed file into the item's storage location, keeping every version.
- Drawn feedback is written into the session's `feedback/` directory, and the prompt names the file.

## GitHub

- Elysium holds any number of GitHub personal access tokens, classic or fine-grained, managed under Settings, GitHub.
  Tokens are application secrets, sealed in Postgres. See `docs/github.md`.
- A coding session starts with one token or none, chosen by the workspace default, overridden by the project, and
  overridden again when the session starts. The agent gets it as `GH_TOKEN`, with git routed through `gh` by
  `GIT_CONFIG_*` variables.
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

- Global environment variables, managed under Settings, Environment variables, are passed into every turn of every
  coding session. See `docs/environment.md`.
- Every value is sealed in Postgres, secret or not. Secret means Elysium never returns the value and scrubs it from
  session events; the agent can always read it, so nothing that must stay hidden from the agent belongs here.
- Keys that Elysium sets for GitHub or the runner, and keys that would pick the model credential, are refused, by one
  rule table in `api/src/environment/` that the frontend mirrors.

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
- Coding agents use one MCP server, `elysium_work`, given to every turn of a session with a project, in Elysium's
  terms and scoped to the session's project, re-checked on every call. Its direct writes are comments and linking the
  pull request the agent opened to its session's item; an agent's link never becomes the primary. Every other change
  it proposes as a changeset with `work_propose_changes`.
- A session started from an item records it, and its first turn carries the item's context ahead of the user's
  prompt.
- Every row records who created it. Ownership (`owner_kind`, `waiting_on`) is still text; making it reference users
  is on the roadmap.
- Every write records a history entry with its actor in the same transaction, with before and after values so it can
  be shown and undone. Deleting items and initiatives is soft. Next's order and the progress rules are pure functions
  in `api/src/action_items/`.

## Elysia

- Elysia's panel sits beside every page, opened from the topbar: her status color (orange running, red errored, pink
  needs you, green done), what holds her back, her questions, her work, the conversation, and her activity. See
  `docs/elysia.md`.
- Everything that starts her is a turn in `elysia_turns`, queued by a person's message or answer, by her intake
  (relevant event-stream changes batched into one `event` turn), or by her idle loop. Turns run one at a time on the
  host runner, on one Claude Code session each turn resumes, with her standing instructions in
  `api/src/elysia/instructions.md`.
- Her hours are UTC minute-of-week windows, because the server holds no time zones. Only turns she would start herself
  wait for them; a person's message never does.
- Her own changes never trigger her, and outward actions go through changesets. Which kinds of action start in which
  autonomy tier is in `docs/elysia.md`.

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
  from forks; permissions stay `contents: read`; no check reads a secret; third-party actions are pinned to a
  commit SHA.
- The one exception is `publish.yml`, which reads the organization's Docker Hub token (`DOCKERHUB_USERNAME`
  variable, `DOCKERHUB_TOKEN` secret). It never triggers on a pull request, and leaves no login on the runners.
- `release.yml` cuts a `vX.Y.Z` tag daily from the newest green commit on `main` and starts `publish.yml` on it. Its
  token may push tags and start workflows; it reads no secret and never triggers on a pull request.
- Tools are pinned in one place and verified: Rust by each crate's `rust-toolchain.toml`, Node by `frontend/.nvmrc`,
  Yarn by `packageManager`, the Diesel CLI by `DIESEL_VERSION` in `api.yml`. Dockerfiles must use the same Rust and
  Node versions; the image checks fail when they drift.
