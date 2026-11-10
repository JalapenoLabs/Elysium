# Elysium

Rust API (`api/`), Vite/React frontend (`frontend/`), Postgres, and Redis behind one nginx origin, run with
`docker compose`. Decisions are documented per topic in `docs/`; keep them current as the code changes.

## Direction

The full vision is `docs/vision.md`. Read it before designing any feature.
The engine, which does all background work, is designed in `docs/engine.md`.
Elysium as the company's knowledge base is designed in `docs/knowledge.md`.
Rooms, where people's Claude Code sessions work together, are designed in `docs/rooms.md`.
Activity, the full investigable log of every agent session, is designed in `docs/activity.md`.
Self-improvement (learning from feedback, self-healing connectors) is designed in `docs/self-improvement.md`.
Autonomy (issue intake, proposed batches, the trust dial, findings) is designed in `docs/autonomy.md`.
Meeting review (captures, the final transcript, excerpts, black-outs, uploads) is designed in `docs/meeting-review.md`.

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

- `compose.build.yml` layers over `compose.yml` to build the production images from the checkout under local tags, for
  a deployment that runs `main` instead of a release: the work instance pulls `main` and rebuilds. See
  `docs/infrastructure.md`.
- `compose.yml` is the production stack: it runs the published `jalapenolabs/elysium-api` image, which serves both
  `/api` and the web app's production build, so nginx sends every path to the API. `compose.dev.yml` layers the
  development stack over it: the API built from source (`api/Dockerfile` stage `server`) and the Vite dev server,
  with nginx swapped to `nginx/development.conf`. See `docs/infrastructure.md`.
- The API serves clients alone and starts no loop. The engine (`elysium-engine`, a second binary of the same crate, run
  from the same image as the `engine` service, one container) does all background work: the runner, sessions' turns,
  Elysia, every connector's reader, the keepers, update checks, and backups. It serves nothing but `/health`. They
  meet only in Postgres and on the bus: the API records what it asks for as a row and signals the engine
  (`crate::realtime::Signal`). See `docs/engine.md`.
- Work that takes steps is a workflow run: one workflow on one source, each step a row (`step_jobs`) the engine's
  workers claim with `FOR UPDATE SKIP LOCKED` under a lease they renew, retry with backoff, and record. A person
  retries, skips, or resumes a step on the Runs page (`/runs`); clients hear ids and states only. See
  `docs/engine.md`, Runs.
- A step implements `workflows::Step` (a name, typed input and output) and never knows its workflow; a workflow is
  declared from step types (`workflows::declare`), each taking what the one before gives, and a run may enter it at
  any step with that step's input. See `docs/engine.md`, Workflows and steps.
- Every agent's turn (a step's, such as correcting or segmenting, Elysia's, a coding or Studio session's, a turn
  summary) belongs to one kind (`agent_models::AgentKind`: worker, Project Manager, correction, segmenting, triage,
  Elysia, summaries) and runs on the model resolved by `crate::agent_models` as the turn starts: the project's
  override of its kind, its own setting, its kind's, then the kind's default (Opus 5.5 at high, summaries Haiku 5.5 at low). Admins set the
  workspace's under Settings, Models; whoever may edit a project sets its own. Every step declares `AGENT_KIND`, and
  every workflow is listed in `workflows::declared`. See `docs/engine.md`, Models.
- nginx, Kratos, Hydra, and the updater never bind-mount their configuration: each directory's `Dockerfile` builds it
  into a local image, rebuilt on every `up` (`pull_policy: build`), so a deploy that changes a file recreates exactly
  the services that use it. Each image has one building service. See `docs/infrastructure.md`, Configuration images.
- `.github/workflows/publish.yml` pushes `jalapenolabs/elysium-api`, `jalapenolabs/elysium-workspace`,
  `jalapenolabs/elysium-whisper`, `jalapenolabs/elysium-embeddings`, and `jalapenolabs/elysium-oauth-broker` to Docker
  Hub on every push to `main` and every `v*` tag. A release is a `vX.Y.Z` tag. See `docs/ci.md`.
- The knowledge base's embedding model (nomic-embed-text-v1.5 on text-embeddings-inference, `embeddings/Dockerfile`,
  pinned and checked) is the compose service `embeddings`, on an internal network only the engine and the API join,
  so no text leaves the host. The engine's embedder (`crate::knowledge::embedder`) embeds passages as they arrive.
  The API joins the network so the knowledge search (`crate::knowledge::search`) embeds its query directly, and
  searches by words alone when the model cannot answer in time. See `docs/knowledge.md`, Embeddings and Search.
- Agent turns run the official `claude` CLI in the workspace container (`workspace/Dockerfile`), which the engine
  creates through `docker-proxy` and reaches by `docker exec`, never inside the API's or the engine's own container.
  Callers in the engine submit turns to `crate::runner`. See `docs/coding.md`.
- Releases update themselves: the API decides when, and the updater (compose profile `auto-update`) carries it out,
  backing up the databases and rolling back a release that does not come up healthy. It never restarts the stack
  while work is running; anything a restart would cut short, such as an agent turn, holds an activity
  (`api/src/activity.rs`) while it runs, which the engine mirrors to Postgres for the API to read. Each API image carries the deployment's configuration, so a release brings
  its own, and the updater owns `ELYSIUM_VERSION` in `.env`. See `docs/infrastructure.md`.
- The engine backs up Elysium's, Kratos's, and Hydra's databases every night (`api/src/backups/`) to the storage
  location an admin chose, keeping 30 days, whether or not the updater runs. A night that fails raises an inbox item;
  the restore is step by step in `docs/infrastructure.md`, Backups, and needs the same `ELYSIUM_ENCRYPTION_KEY`.
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
- A session or MCP token that ends is refused on its next request, though the API caches what Kratos and Hydra say:
  sessions end only through the API (`crate::auth::sessions`, `crate::oauth::tokens`), which forgets them in the same
  step, and nginx keeps browsers from ending one through Kratos directly.
- 2FA is never off while it changes. Adding an app saves nothing until the app is verified, after its backup codes
  are downloaded. Moving to a new app is the API's own change: it holds the new secret sealed until a code from it
  checks out, then swaps it into Kratos in one admin write (`api/src/auth/authenticator_change.rs`). See
  `docs/auth.md`, Authenticator apps.
- An identity holds an email and a first and last name. A change to what identities hold is a new Kratos schema
  registered beside the old one, never an edit of it, and the API moves each identity onto it when it starts
  (`api/src/auth/identity_schemas.rs`). See `docs/auth.md`, Identity schemas.
- Kratos's configuration is derived from `ELYSIUM_PUBLIC_URL` and `ELYSIUM_ENCRYPTION_KEY` by
  `kratos/entrypoint.sh`. Nothing for Kratos goes in the bootstrap file. Its database, `kratos`, shares Elysium's
  Postgres server and credentials.
- `users` holds every principal: people, mapped to their Kratos identity, and the seeded machines (the system user,
  Elysia, and the coding agent), which can never sign in. Roles are one enum: `admin`, `member`, `guest`, `agent`,
  `system`. Guests have a member's access, except the knowledge base, until roles are built out.
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
- nginx sends client registration (`/oauth2/register`) to the API, which relays it to Hydra's public API, never the
  admin API, and leaves out the `""` and `null` fields Hydra's answer carries, which strict clients such as Claude Code
  refuse (`api/src/oauth/registration.rs`). See `docs/mcp.md`, Client registration.
- Every token is bound to `/api/mcp` and checked on every call: active, granting `workspace:read`, and naming an active,
  approved person. Writing tools also need `workspace:write` and record the person as actor and creator.
- A tool never reimplements a route: it calls the function the route's handler calls, so a change made over MCP is
  validated, recorded, and announced exactly as one made in the browser. Tools exist for projects (read), coding
  sessions, searching, opening, and listing the meetings of the knowledge base, saving and deleting learnings, reading
  and proposing to the company doc, and reading and editing pages; Studio items, action items, and initiatives are on
  the roadmap.
- The knowledge base's reading tools are also on the `elysium_knowledge` server every turn gets, under the same names
  and schemas. A turn reads for whoever started its session, checked on every call, and spends their search budget.
  See `docs/knowledge.md`, MCP.
- Agents edit pages directly (`page_save`, `page_move`, `page_label_add`, `page_label_remove`, `page_project_link`,
  `page_project_unlink`), every change naming the
  person and their agent: an MCP client as its person, a session as whoever started it, a Project Manager and Elysia as
  themselves. A pool worker only reads. Company doc pages refuse edits, pointing at `company_doc_propose`, and learnings
  refuse them, pointing at `learning_save`; both still take labels. See `docs/knowledge.md`, Who changes pages.
- Every client, and every turn's `elysium_knowledge`, is told the company's agent guidance as it connects, so a change
  reaches its next session. Admins edit it under Settings, Agent guidance, every version kept; Elysium's rules (results
  stay in the company, where a citation opens) follow every version, and no edit removes them. See
  `docs/knowledge.md`, Agent guidance.

## Agents run on the host

- Agent turns run the official Claude Code CLI, on a subscription token or an API key, in the `elysium-workspace`
  container the engine owns. The Agent SDK is never used with subscription credentials. See `docs/coding.md`.
- Agents never run in the API's or the engine's container, and the workspace shares a network with the API alone.
  Workspace files are read and written through `docker exec` as `agent`, confined by `realpath`; nothing mounts the
  volume.
- A session is a directory in the workspace (`sessions/<id>`), the Claude session its turns resume, and its events in
  `session_events`. Turns of one session run one at a time; at most eight run at once.
- A turn runs on Elysium's active Claude credentials as an ordered failover stack, highest priority first, so a
  credential that runs out hands the turn to the next one, resuming the same Claude session.
- Claude credentials are API keys, `claude setup-token` tokens, or Claude accounts signed in through the official CLI's
  own login (`claude auth login`, run in the workspace; Elysium never runs an OAuth flow of its own). Elysium is an
  account's only refresher: turns get its access token alone, and the keeper in `api/src/claude_accounts/` refreshes
  it under a row lock and reads its usage, so an account whose usage window is spent is skipped until it resets. See
  `docs/coding.md`, Claude accounts.
- A turn's prompt reaches the CLI on standard input, never as an argument, so no caller cuts what it asks to fit the
  128 KiB Linux allows one argument. See `docs/coding.md`, How a prompt reaches the CLI.
- Turn policy (heartbeat, concurrency, clone and script timeouts) is code constants, not configuration.
- Agents' `~/.claude` is checked out from the configuration repository named in the workspace settings, read with a
  GitHub token of its own chosen beside it, and adapted to the container that runs it. No worker or Project Manager
  turn starts without it: the pool and the Project Managers wait, saying why, with one inbox item. Coding sessions and
  Elysia are not held. See `docs/coding.md`, `~/.claude`.
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
- A summary of a session with a project is a timeline record of that project in the same transaction, one per turn,
  owned by the coding agent's machine user and read by every member, indexed for search, and opening the session at
  that turn. It wakes no Project Manager. See `docs/knowledge.md`, Turn summaries.

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
  volume, Docker network, and media port per browser connector. A browser starts with its connector and is removed
  with it. The engine runs and reconciles them; see `docs/browser.md`.
- A browser belongs to the person who created it. Its routes answer that person alone, and the stream relays a Neko
  member session, never an administrator's.
- nginx stays the single ingress: it passes the media ports, `59000-59007` over UDP and TCP, to the browser that owns
  each one, on every interface. Signaling never leaves the API's authenticated stream route.
- A browser connector's page carries its sign-in panel, which speaks Neko's protocol itself: Neko's own client cannot
  run under the web app's CSP.
- The API and Elysia reach each browser over the Chrome DevTools Protocol, on that browser's private network only.
- A browser always has a way out: Start over reopens its start page in a fresh tab and keeps the profile; Reset
  removes the profile volume, signing it out of everything, and starts it again. A stream whose media never connects
  says so, with the media ports behind its help, rather than spinning.

## Connectors and the timeline

- Every connector (`connectors`) owns exactly one account, its sign-in, shared with no other connector: Atlassian
  (`jira_credentials`), GitHub (`github_credentials`), a browser (`browser_sessions`), or an API key
  (`token_accounts`). Adding a connector signs in; removing it removes its account. One Connectors list holds
  everything; there is no separate accounts or browsers page. See `docs/connectors.md`.
- Adding a connector picks its kind from a searchable grid (`/settings/connectors/new`), then fills that kind's own
  page (`/settings/connectors/new/<kind>`): a token kind signs in, chooses what it reads, and runs its checks before it
  is created; a browser kind starts its browser and is signed in to on its connector's page.
- Each kind is one module under `api/src/connectors/kinds/` declaring its definition (scope, what it provides, the
  account kind it signs in with, content fields, ordered checks) and its verbs; `kinds/mod.rs` alone matches on the
  kind. What a kind provides is how a loop finds the kinds it reads, such as meetings. The API serves the definitions
  and the web app draws forms and check lists from them, naming every id from its translations.
- An Atlassian account carries one connector: Jira's, holding its allowlist, or Confluence's, which allows no Jira
  project or board and is never offered as a Jira site. Anything about the link watcher's reading is matched by kind
  as well as account, never by account alone: paused accounts by the kinds that provide issues, its pass and
  linked-issue counts by `ConnectorKind::LINKS`.
- A secret a migration copies into a new row is re-sealed for that row by the API at startup (`secret_copies`), since
  the migrator holds no key. See `docs/secrets.md`.
- Checks run in order, a dependency that did not pass skips what builds on it, and they run before a connector is
  saved and on demand. Progress is published as ids and states only; a stored connector's last results are kept.
- Syncs that record something or fail are runs (`connector_runs`), reported through `connectors::activity`; a
  connector's status is paused, syncing, error, or active. A paused issue connector's account (Jira, GitHub, or
  YouTrack) is skipped by the link watcher. `connector.changed` carries an id alone.
- Outlook and Gmail read through the connector's own browser over DevTools, with one page script per service; the
  check is cheap and deterministic, and nothing it does marks mail read.
- Confluence Cloud reads through its own Atlassian account, every five minutes, the pages and comments changed in
  the spaces its connector picked, one record per version. `api/src/confluence/` is its only caller, and
  `Confluence::changes`, the only call that reads content, is bounded by the picked spaces in its CQL and again on each
  result. A mention of the person, or a question on their page, raises an inbox item once per page or comment.
- Teams reads through its own browser signed in to Microsoft, from Teams' own local store (IndexedDB) rather than its
  page; nothing it does marks a message read. An unread chat message, a mention, or a reply in a channel thread the
  person wrote in raises an inbox item; other channel messages are only recorded.
- YouTrack reads a person's YouTrack Cloud site with their own permanent token, a token account that names its site
  (`token_accounts.site_url`), validated like a Jira site: `https`, one label on `youtrack.cloud`, the origin alone.
  `crate::youtrack` is the only caller. Every two minutes it records new issues and comments in the chosen projects;
  a comment from someone else on their issue or mentioning them raises an inbox item as `watcher:youtrack`. The first
  read records nothing. Its assigned issues come in through the watcher, as every issue connector's do.
- Slack reads through its own browser signed in to `app.slack.com`, never a Slack app: its page script calls Slack's own
  web API with the web client's session, which never leaves the page, and only read methods. A direct or group
  message, a mention, or an unread reply in a followed thread raises an inbox item; a conversation is baselined at
  its newest message the first time it is read, so no history is pulled in.
- Everything ingested is a timeline record (`timeline_records`), unique per account and provider id, attributed to
  projects with a confidence (`timeline_record_projects`). A record is its owner's alone, even about a shared
  project: only they list it, attribute it, and play its media. Its text is not: every record is indexed into the
  knowledge base for everyone in the workspace in the transaction that stores it (`timeline_record::insert_new`), a
  person's own mail and direct messages included, with no per-owner filter in any query, and guests never reach it.
  The engine's backfill indexes what was stored before. See `docs/knowledge.md`, Timeline records, and Who sees what.
- Ingest runs every minute. A new unread message raises an inbox item in the same transaction as its record,
  recorded as `watcher:<service>`: connecting a mailbox is the consent, as linking a container is for the link
  watcher. The first read baselines the mailbox without raising anything.
- Every direct message and mention joins its conversation (a Slack direct message or thread, a Teams chat or thread,
  a mail thread by sender and subject) in the transaction that records it, as rows the engine claims, never timers.
  Ten seconds after a conversation's newest message, the engine starts a `request` run: segmenting, then the
  conversation's agent, one Claude session resumed every turn, which raises an inbox item as Elysia for each question
  or request the person owes, may wait 30 seconds once per run, and is given what arrives while it runs. It never
  sends. See `docs/engine.md`, Conversations.
- `timelineRecord.created` carries ids and kinds, never content, because every client hears every event. It is the
  seam Elysia's triage builds on.
- A person answers what a connector read as themselves: each kind declares `replies` (through the connector itself,
  from a mailbox, or never). Everything sent is a draft (`drafts`), sent by the person through `crate::drafts::send`,
  the one way anything leaves in someone's name: Slack by `chat.postMessage` and Teams by its chat service, each from
  a send-only script in the connector's browser, and mail over SMTP from the person's own mailbox. A send that fails
  keeps the draft and its reason and is never retried on its own; one that lands is a timeline record named as the
  reader names it. See `docs/connectors.md`, Sending.
- A new account's secret is checked with its provider before it is stored, sealed bound to its row, and never
  returned.

## Meetings

- Meetings are read from a meeting source as their person (`api/src/meetings/`): today Fireflies, with the person's
  own API key, so nothing is installed into an organization. A source is a connector whose kind declares it provides
  meetings, managed under Connectors alone; the meetings page and loop never name one (`meetings::sources`). Every
  meeting belongs to a connector, which is what keeps it and its segments from being recorded twice. See
  `docs/meetings.md`.
- Each meeting is a run of the engine's `meeting` workflow, whose `fetch_meeting` step stores the source's raw
  transcript with its SHA-256 and uploads the recording to the account's storage location; fetching the same meeting
  again changes nothing. Nothing a source's own AI wrote (summaries, action items, notes) is read or kept.
- The run's last step, `segment_meeting`, segments the corrected transcript in one text turn on the segmenting kind's
  model and publishes it: `ffmpeg` cuts each segment's clip in the workspace container at the words Whisper heard
  nearest its ends, and each segment becomes a timeline record with its excerpt, summary, span, and clip, attributed
  to its projects. The step also takes a conversation (record ids), whose messages it attributes, which is where
  intake enters the workflow. The meetings loop only lists meetings and starts their runs; a meeting keeps how far it
  got, and its failure or skip is its run's. See `docs/meetings.md`, Processing and Segmentation.
- Every change to what a record is attributed to is recorded in `timeline_record_events` and undoable; Elysia never
  overrides a person's attribution. See `docs/connectors.md`, The timeline.
- Each meeting with audio is also transcribed by a local Whisper (faster-whisper, large-v3, pinned and checked in
  `whisper/`), the meeting workflow's `transcribe_meeting` step: one container per transcription that the engine
  creates through `docker-proxy`, with no network, the recording on standard input and the transcript, every word
  timed, on standard output. It decodes on half the host's CPUs, at most 16, one transcription at a time, and is stored
  once in `meeting_transcriptions`. A meeting without audio skips the step. See `docs/meetings.md`, Transcription.
- Each meeting's run corrects its transcript (`correct_transcript`) against every transcript it has, the company doc,
  and other meetings, into a copy beside the raw ones, which are never changed. The model only names lines and their
  fixed text, each checked word by word, so the copy keeps every line, speaker, and time. A source's "Wait for my
  approval before acting on meetings", or a meeting held by hand, holds the run with an inbox item until the person
  approves the copy, and segmenting runs once it goes on. See `docs/meetings.md`, Correction.
- Nothing the segmentation answer says is trusted: segments must lie inside the meeting (or the conversation) in order,
  and attributions must name projects that were offered. A failed step takes the run's attempts and backoff, then waits
  for the person to retry it from the Meetings page or the run's page.
- Recording downloads follow redirects only to `https` named hosts. `meeting.changed` carries a meeting's id, state,
  and owner, never its title or transcript.

## Storage

- Storage locations are where Elysium saves files, one row per location with a provider kind, used by the projects
  linked to it or by every project (`*`). Providers are Bunny Storage and S3 buckets on AWS or Google Cloud, with
  every endpoint fixed in code; local storage is planned. See `docs/storage.md`.
- Access keys are application secrets, sealed in Postgres. Routes never match on the provider; `api/src/storage/`
  does.
- Coding agents reach their project's locations through the `elysium_storage` MCP tools at `/api/agent-tools`. Every
  call re-checks the location against the project in the database. See `api/src/tools/` and `docs/storage.md`.
- Files attached to knowledge base pages go to one location an admin chooses, and each row keeps the location it landed
  in. Limits and what a page takes are code constants (`api/src/page_attachments.rs`), served to the web app. They are
  served to members alone: a proven image displays, and anything else downloads and never renders on Elysium's origin.
  An attachment is no part of a version, and a page's text refers to one as plain Markdown. Nothing is deleted for good
  yet, so a deleted page keeps its files. See `docs/knowledge.md`, Pages, Attachments, and `docs/security.md`.

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

## Problem reports

- Report, beside any error the app shows, files it on `JalapenoLabs/Elysium` (fixed in code) as the person, with a
  GitHub token they added, the default preselected. `api/src/issue_reports/` and `/api/v1/issue-reports`; any surface
  offers it through `ReportIssueButton` or `useReportIssue`. See `docs/issue-reporting.md`.
- Everything posted is redacted deterministically on the server first (known names and accounts, then patterns), set in
  code blocks, and previewed exactly as posted. A fingerprint of the redacted error puts one problem on one open issue;
  later reports comment on it. Ten reports a person a day, a code constant.

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

## Passwords

- Agents sign in to things on their own with logins from one Bitwarden or Vaultwarden account, managed under Settings,
  Passwords by admins. Its API key and master password are application secrets, sealed in Postgres. See
  `docs/passwords.md`.
- The official CLI (`bw`, pinned in the API image) runs inside the engine's own container, never the workspace: one-shot
  commands sign it in, and one `bw serve` on the container's loopback answers everything else. Its state is on the
  engine's data volume, so a restart keeps it signed in as the same device. The API asks it over the bus, sealed.
- Agents read one collection, chosen from those the account reaches, through the `elysium_passwords` tools. Every call
  re-checks the item against the collection in the database. TOTP seeds are never shown, only current codes.
- Every password and code handed to an agent is scrubbed from every turn's recorded output, as secret environment
  variables are.

## The company doc

- The doc is seven proposal-only pages under the Company page, each section's id fixed in code, with its history in
  their versions and `page_events`. Those pages, the Company page, and the Learnings page are fixed: nobody moves,
  deletes, or renames one. See `docs/knowledge.md`, Pages, Learnings and the company doc.
- One document per instance knows the company in seven fixed sections: company, leadership, people, stakeholders,
  products, projects, and glossary, the last two carrying how names and jargon are spelled. See `docs/knowledge.md`,
  The company doc.
- People edit it directly; everyone else (the scheduled review, Elysia, coding agents, MCP clients) proposes a change
  to one section, and nothing changes until a person approves each one. Proposals are the doc's own, not changesets.
  Approving merges with what changed since, three ways, and refuses overlapping lines.
- Every change is recorded in its history with before and after, and undone section by section. Each section is a
  source of the knowledge index.
- The review is the engine workflow `company_doc_review`, every six hours while something new was ingested
  (`docs/engine.md`, The company doc). Agents read and propose through `company_doc_read` and `company_doc_propose`,
  over MCP and on the `elysium_knowledge` server every turn gets.

## Action items

The core and its frontend are built: items, initiatives, memberships, comments, history, Next, and progress, under
`/api/v1/action-items` and `/api/v1/initiatives`, and the Action items area that uses them. So are the `elysium_work`
tools, coding sessions started from an item, and links to Jira, GitHub, and YouTrack with the watcher and their
frontend. So are
changesets under `/api/v1/changesets`, the `work_propose_changes` tool, and their review in the Action items area.

- Action items are the one list of what the user owes attention to, from Jira, GitHub, email, meetings, or typed by
  hand. Initiatives group items toward a goal that ends and carry the progress bar; projects never end and have
  none. Items belong to many projects and many initiatives. See `docs/action-items.md`.
- An item is a commitment, not a copy. The provider stays the source of truth for a linked issue's fields; the item
  holds triage, priority, dates, membership, provenance, and history. Resolving an item moves every linked issue
  still open automatically, and the provider closing a linked issue resolves its item. Resolving an item never merges
  or closes a pull request. Elysium merges only a worker's own pull request, only in a repository its project set to
  auto-merge, and only once every fail-closed check passes; it closes none (`docs/engine.md`, Merging).
- Progress is resolved over total with a burnup of both, never a percentage alone. Initiatives link to containers
  (a Jira epic or search, a GitHub milestone or label) rather than issues one by one, so nothing is tracked twice.
- Anything the user did not do directly (Elysia, Elysium's AI assistant; email triage; coding agents; Project
  Managers) proposes a changeset. Nothing is written in Elysium or any provider until the user approves it, wholly or
  in part, except a Project Manager's on a project that lets it write directly, which applies at once. The watcher
  only records what already happened in a provider, so it needs no changeset.
- A changeset's operations are decided one by one; rejecting one rejects what depends on it. Applying runs the
  approved ones in order as the proposer, each in its own savepoint, so one that fails is marked with the reason and
  the rest apply; provider writes go through the same outbox as the user's. The history it records names the
  changeset, so it can be undone as a whole, never overwriting what the user changed since. What already left Elysium
  (a posted comment, a closed issue) is not reversed, and the review says so. Rules and queries:
  `api/src/action_items/changesets.rs` and `api/src/models/changeset.rs`.
- A person changes many items or initiatives at once through `POST .../bulk`: every one goes through the write its own
  route makes, in one transaction with a savepoint each, so one the rules refuse is reported and the rest apply. See
  `api/src/models/action_item_bulk.rs`.
- The watcher polls providers for changes; webhooks come later. Link providers sit behind one trait
  (`api/src/action_items/links/`), so routes and tools never match on the provider. Every Jira call goes through the
  credential's allowlist, as the Jira routes' calls do.
- Every connector whose kind provides issues (Jira, GitHub, YouTrack) brings in what is assigned to its person on each
  watcher pass, bounded by its content (the Jira allowlist, chosen repositories, chosen projects): every open assigned
  issue, the first read included, raises one inbox item as `watcher:<service>`, linked to it as its primary, in no
  project, and starts only its `assigned_issue` run, which attributes it. `assigned_issues` keeps one row per connector
  and issue, so none raises a second. Being unassigned leaves the item and records `unassigned` on it. A new issue kind
  gets this by declaring `Provides::Issues` and its link provider (`kinds::link_provider`). See `docs/action-items.md`,
  Assigned issues.
- An item in any project has one lead project, and only that project's Project Manager may submit a batch for it
  (`project_managers::batches`). Its first project leads it, the surest of those left when the lead leaves,
  and a person moves it. An arrived issue is put on its projects by `segment_meeting`, the step meetings use, with
  Elysia's confidence, the surest leading; she never overrides anyone else's choice, every change is in the item's
  history and undoable, and attributing wakes no Project Manager (`api/src/models/action_item_attribution.rs`). See
  `docs/action-items.md`, Projects and the lead.
- A person assigns an item, alone or a selection, to its lead project's Project Manager, which wakes to plan it and
  starts no worker; an item no project leads is refused, and moving the lead ends the assignment. Elysia suggests
  assignments as `assign-item` changeset operations, and the person accepting one is who assigned it. The item's page
  shows where its plan stands (`api/src/models/action_item_assignment.rs`). See `docs/action-items.md`, Assigning to a
  Project Manager.
- The watcher acts on a change of what a link last recorded, never on the provider's state alone, and its interval is a
  constant in code. Its cursor per credential is persisted, and it stops with the shutdown token.
- Provider writes (closing a resolved item's issues, posting a comment to the primary link, filing an item's own
  issue) are owed in the same transaction as the change and landed by the watcher. One that fails stays pending,
  retried every pass, until it lands or the user cancels it. The item's own change always stands. A filing tried
  before looks for the issue its earlier attempt opened before opening another.
- YouTrack issues link through a person's own YouTrack account (`token_accounts`); resolving moves one to a resolved
  state of its project's state field, `Fixed` or `Done` when it has several. Initiatives contain no YouTrack issues.
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
  needs you, grey held, green done), what holds her back, her questions, her work, the conversation, and her activity. See
  `docs/elysia.md`.
- Everything that starts her is a turn in `elysia_turns`, queued by a person's message or answer, by her intake
  (relevant event-stream changes batched into one `event` turn), or by her idle loop. Turns run one at a time on the
  host runner, on one Claude Code session each turn resumes, with her standing instructions in
  `api/src/elysia/instructions.md`.
- Her hours are UTC minute-of-week windows, because the server holds no time zones. Only turns she would start herself
  wait for them; a person's message never does.
- Her own changes never trigger her, and outward actions go through changesets. Which kinds of action start in which
  autonomy tier is in `docs/elysia.md`.

## Project Managers

- Every project has a Project Manager (`api/src/project_managers/`): one Claude Code session per project, resumed for
  every turn (`claude -p --resume`) and kept indefinitely, relying on Claude Code's compaction. It plans the project's
  work and submits it as work batches (`work_batches`: a goal, the goal in its project's brief it serves, repositories,
  ASAP, normal, or a scheduled time, and its provenance) for the worker pool. See `docs/engine.md`, Project Managers.
- Every project has a brief (`api/src/models/project_brief.rs`): a fixed knowledge base page under Project briefs,
  created and renamed with the project and linked to it, holding its goals (`###` headings under `## Goals`), decisions,
  and what is out of scope. Every turn's prompt carries it whole; the Project Manager keeps it current with `page_save`
  against the version it read, recording each rejection's reason, and a batch serving no goal in it is refused. Anyone's
  publish of it but its own Project Manager's and the system user's wakes that Project Manager. See `docs/autonomy.md`,
  The project brief.
- Every batch's origin (human-rooted or found) and generation (how many batches led to it) are worked out from its
  provenance as it is submitted, never asserted (`work_batch::lineage`). One naming nothing, or past the generation
  limit (a code constant), is refused with a reason its Project Manager is told. See `docs/engine.md`, Origins and
  generations.
- It is woken by a record newly attributed to its project, a batch's result, a promise, an action item a person assigned
  it, a person, Elysia, and someone else's edit of its brief, each written as a wake in the transaction that caused it.
  Wakes are debounced into one turn per project, one turn at a time (the database refuses a second), and a turn a
  restart or a usage limit cut short gives them back.
- Its turns and batches are timeline records owned by its machine user: the workspace's, read by every member on the
  project's timeline, changed by nobody. Its model is its kind's for the project (`Agent::ProjectManager`).
- It reads its project's docs, the knowledge base pages linked to the project (`page_projects`, `docs/knowledge.md`,
  Pages, Projects): every turn's prompt names them, and `manager_pages` and `manager_page` list and read them, refusing
  any other page. A link wakes nobody.
- It changes its project's action items and files their issues in Jira, YouTrack, or GitHub with
  `manager_make_changes`: one changeset per call, as `project_manager:<project id>`, each operation naming the timeline
  record that caused it and recorded on the timeline when it applies. They apply at once only at its project's Full
  autonomy; at every other level they wait for a person's approval.
- Each project has one level of autonomy, its trust dial (`projects.autonomy`, `crate::models::project_autonomy`):
  Approve every batch, the default; Approve found work only, where human-rooted batches queue without an approval;
  and Full autonomy, where its changes apply at once too. Only a person sets it, on the project's page
  (`project_autonomy::set` takes a `MemberUser`), each change recorded with who and when, and it applies to what comes
  next. No level lets found work queue itself. See `docs/autonomy.md`, The dial.
- Its issues file in the tracker the project names, or else where most of its items' issues already are; Jira and
  YouTrack through that tracker's credential, GitHub through the project's own token or the workspace default. Filings
  are an outbox the watcher lands (`action_item_issue_filings`), and a retried filing finds the issue its earlier
  attempt opened rather than opening a second.
- It keeps the promises people make in conversations ("I'll let you know when it's live", `promises`), which
  segmenting finds: it ties each to a batch or an action item, and once that work succeeds or resolves it drafts the
  follow-up in the person's voice, a draft of theirs beside an inbox item they send, edit, or dismiss. Nothing is sent
  without their Send, which keeps the promise and resolves the item. See `docs/engine.md`, Follow-ups.

- Agent-found work never queues itself. Workers and Project Managers report work nobody asked for as a finding
  (`worker_report_finding`, `manager_report_finding`, `api/src/findings.rs`): what was seen, where, and what it would
  cost, one per project however often it is reported. A person accepts it from the project's Findings tab, which makes
  an item as them, owes its issue in the project's tracker, and submits a batch, human-rooted and generation 0, which
  follows the project's level of autonomy, or
  dismisses it for good, with a reason its reporters are told. Findings raise no item each and never wake Elysia; she
  reads each project's count in `elysia_overview`. See `docs/engine.md`, Findings.

## The worker pool

- A batch a Project Manager submits starts `proposed` and waits for a person to approve it, edit and approve it, or
  reject it with a reason, which wakes its Project Manager; the pool never gives a proposal a slot. Only a human-rooted
  batch on a project whose level of autonomy lets it starts `queued` instead (`work_batch::submit`). Only a person
  decides (`worker_pool::actions::approve` and `reject` take a `MemberUser`); Elysia and Project Managers list
  proposals. Each project with proposals waiting has one inbox item counting them. See `docs/engine.md`, Proposed
  batches.
- One global pool of worker slots (`api/src/worker_pool/`); Project Managers never start containers. The engine's
  scheduler hands each free slot to the first approved batch in order: run next, then due scheduled, ASAP, and normal, oldest
  first, round robin across projects by the slots each holds (`worker_pool::order`, a pure function). None is handed
  out while every Claude account is at its usage limit. See `docs/engine.md`, Workers and the pool.
- Its size is an admin setting (`workspace_settings.worker_pool_size`), by default one worker per CPU core the engine
  sees minus two, at least one, with no maximum.
- A slot is a batch `assigned` under a lease the engine renews, fenced by its token and taken over by the next engine
  once it lapses or is released at shutdown. A worker takes it with `work_batch::start` and gives it back with
  `work_batch::finish`.
- Each slot is one worker (`api/src/worker_pool/worker/`): Claude Code in a container of its own from the workspace
  image, `elysium-worker-<batch id>`, with its own volume and a bridge network only the API joins, limited to its share
  of the cores the pool leaves workers (`worker_pool::worker_nano_cpus`). It runs the project's tools script as root,
  clones the batch's repositories with `--recurse-submodules` and runs each declared repository's setup script
  (`project_repositories`, `projects.tools_setup_script`), then runs `/goal` turns resuming one Claude Code session
  until it reports through its `elysium_worker` tools: `worker_finish` ends the batch and removes the container;
  `worker_wait` (a review, CI, or a question its Project Manager answers) keeps it. The API files a report; only the
  engine holding the slot acts on it. See `docs/engine.md`, Workers.
- Each declared repository says who merges its workers' pull requests (`project_repositories.merge_policy`): a person,
  the default, or Elysium (`auto`, github.com alone). A worker records each pull request it opens
  (`worker_pull_request`, `work_batch_pull_requests`) and never merges one. As each wait ends and before the batch
  finishes, the engine reads them through the project's token (`api/src/worker_pool/merging/`) and merges one only
  when the project's account opened it, from the repository itself, and GitHub says it merges cleanly, with checks
  that all passed, every required check among them, no open review thread, no requested changes, and no merge queue,
  naming its head commit and never bypassing the branch's rules. Anything unread or unknown holds it, saying why. A
  merge, by Elysium or a person, is a `merged_pull_request` timeline record that wakes the Project Manager.
- People, Elysia, and Project Managers change a waiting batch only through `worker_pool::actions` (approve and reject,
  for a person alone; run next, priority, pause, resume, cancel), each change recorded in `work_batch_events` with who
  made it. The work queue is `/work-queue`, for members.
- Every batch shows why it exists as one chain read from its provenance, never copied (`work_batch::why`): what asked
  for it (a message, a meeting quote, a mail, an item's issue), its items, the batch, and its pull requests, each a
  link. Every pull request a worker records carries the chain at the end of its description
  (`worker_pool::description`), saying only what each link is in a public repository. See `docs/engine.md`, Why a
  batch exists.

## One event stream keeps the frontend current

- Every page holds one server-sent event stream, `GET /api/v1/events`. Any write or watcher that changes what a
  client shows publishes to it. Components never poll. See `docs/realtime.md`.
- The bus is one Redis pub/sub channel, so any process built from the API's crate publishes to every browser,
  including one that serves no HTTP, such as the engine. Each process relays its one subscription into an in-process
  channel. Events keep their shapes, and the ones about a person's data carry ids, never content.
- `hello` is sent only while the process holds its subscription; while it is down the stream answers `503`. Any event
  that may have been lost on the way through Redis (a dropped subscription, a failed publish, a full queue) ends in
  `resync`, never in silence.
- The API runs as one replica, for the request-scoped state `docs/engine.md` lists (One replica of the API); nothing
  it serves is held in the engine's memory.

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
- Elysium saves manually, per page: no field saves itself, and a page's fields wait for its one Save, which `Page`
  runs for every way of saving. Leaving with unsaved changes asks first. See `docs/frontend.md`, Saving.
- Every user-facing string goes through i18next. `en-US` is the only locale today.
- The topbar's search is the one way to reach anything (`/` focuses it). Pages and actions register themselves in a
  `searchEntries.ts` beside them, with hidden tags; other sources join `useGlobalSearch`. See `docs/frontend.md`,
  Global search.
- Every date, duration, and number shown goes through `frontend/src/format/`: "January 1st 2026 at 8:56pm", never
  numeric months, and "1.4M" with one decimal. Lint fails on formatting anywhere else. See `docs/frontend.md`, Dates
  and numbers.
- Markdown reads through one component, `Markdown` (`src/components/markdown/`): React elements, raw HTML left as text,
  and another origin's image a link, since nothing in a page is trusted. See `docs/frontend.md`, Markdown.
- Pages are edited in a Tiptap 3 editor that stores Markdown (`src/components/markdownEditor/`): every block nobody
  touched keeps its exact text, and what the editor cannot hold opens as its Markdown. See `docs/frontend.md`, The
  Markdown editor.
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
- Postgres is pgvector's build of Postgres 18, pinned by digest, the same in `compose.yml` and
  `verify-migrations.sh`; CI compares them. `migrate run` rebuilds the indexes of any database whose collation
  changed under a new image (`docs/database.md`, Collations).
- The knowledge base's index is `knowledge_documents` and `knowledge_passages`: each source is a cascading column of
  its document, and each passage has a generated `english` tsvector and a `vector(768)` embedding. Every source enters
  and leaves it through `knowledge_document::upsert` and `remove`, in the transaction that changes the source. See
  `docs/knowledge.md`.
- A learning is a page with a category and evidence, under the Learnings page (`crate::models::learning` over
  `crate::models::page`), indexed the moment it is saved. A title names one
  learning whatever its case, so saving it again revises that learning; every change can be undone. The knowledge
  base's routes and tools refuse guests (`MemberUser`). See `docs/knowledge.md`, Learnings.
- A learning's category is a row of `learning_categories`: Elysium's nine built-in ones, fixed, and the workspace's
  own, which admins manage. Removing one that learnings use moves them first, each as a revision; agents read every
  category's description in `learning_save`'s schema. See `docs/knowledge.md`, Categories.
- Pages (`pages`, every published version in `page_versions`, history in `page_events`) are one tree of Markdown for
  the whole workspace, to any depth, soft-deleted with what is under them. Publish is the only write of a page's
  content; it names the version it was edited from, a later one answers `409 page_changed`, and identical text makes no
  version. A page is indexed in the transaction that publishes it. Every change to the tree's shape takes one advisory
  lock, and gapped positions mean a reorder writes one row. A `proposal_only` page refuses everything but a person
  acting alone. Every write takes an `Author` (`models::agent`), so agents' tools call what the routes call. See
  `docs/knowledge.md`, Pages.

## CI

- GitHub Actions on the org's self-hosted runners (`[ self-hosted, linux, rocky9, docker ]`). The runners are
  persistent and warm, so workflows never use `actions/cache`. See `docs/ci.md`.
- Every build and test run uses at most 4 cores: `jobs = 4` and `RUST_TEST_THREADS = "4"` in each crate's
  `.cargo/config.toml`, and `--maxWorkers=4` in the frontend's `yarn test`. Run one Docker image build at a time,
  with `--build-arg CARGO_BUILD_JOBS=4`.
- Changes are committed straight to `main` and pushed, with no pull request and no wait on CI. The checks run on every
  push as an alarm after the fact, and a red run is fixed by the next push. Every workflow still runs on
  `pull_request`, `merge_group`, and pushes to `main`, with no path filters, for outside contributors' pull requests.
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
