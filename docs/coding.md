# Coding

The Coding area runs agent sessions on Arsox satellites. A satellite is a self-hosted worker
(`JalapenoLabs/arsox-satellites`) that runs Claude or Codex in a workspace and reports everything it does as one
typed event stream. Satellites are registered under Settings, Manage satellites; sessions are started and followed
on the Coding page.

## Terms

| Elysium          | Arsox   | Meaning                                                                   |
|------------------|---------|---------------------------------------------------------------------------|
| Project          | —       | What sessions are grouped under, stored in `projects`                     |
| Satellite        | —       | A satellite's URL and bearer secret, stored in `satellites`               |
| Coding session   | Thread  | An agent conversation with its own workspace; Elysium stores a pointer    |
| Prompt           | Turn    | One request to the agent. Turns queue behind the one running              |
| Session event    | Event   | Something the thread did: a message, a tool call, a turn finishing        |

## The API is the only satellite client

Satellites speak protobuf and authenticate with a bearer secret that must never reach a browser. The frontend
therefore never contacts a satellite. The API talks to them with the Rust SDK, `arsox-sdk`, a git dependency
pinned to a commit of the satellites repository's `develop` branch. It converts every contract type into JSON views
in `api/src/fleet/views.rs`, so no route or client depends on the wire format.

To upgrade the SDK, change the `rev` in `api/Cargo.toml`. `api/.cargo/config.toml` sets `git-fetch-with-cli`,
because Cargo's built-in git client cannot fetch a pinned revision of that repository.

## Satellites

A satellite has a name, description, URL, sealed secret, and active flag. The URL is where the API reaches it, so
inside compose it is a container address such as `http://arsox:8080`.

`POST /api/v1/satellites/{id}/test` connects with the saved settings and reports the satellite version, proto
version, and thread load. It works for inactive satellites too.

## The fleet

`api/src/fleet/mod.rs` keeps one client per active satellite, created on first use and dropped whenever the
satellite changes or stops answering. Two kinds of watcher run under it.

- **Satellite watcher**, one per active satellite. Every 3 seconds it reads the satellite's status and lists the
  threads Elysium opened (metadata `elysium.managed=true`). It publishes `satellite.status` when reachability or
  load changes, and `session.upserted` when a session's thread changes state.
- **Session watcher**, one per session on an active satellite. It follows the thread's event stream from the
  latest event and publishes each one as `session.event`. After a dropped stream it resumes from the last event it
  delivered. If it cannot, it tails from the latest event and publishes `session.resync`. When the satellite no
  longer has the thread, it marks the session `expired` or `destroyed` and stops.

Polling stands in for the satellite's control stream, which the Rust SDK does not expose yet. Watchers start at
boot for every active satellite, restart when a satellite is saved, and stop when it is deactivated or deleted.

## Tool relay

Some of an agent's tools answer from Elysium's own data, such as the storage tools in `docs/storage.md`. A satellite
cannot reach Elysium, so Elysium declares those tools on the thread as relayed MCP servers
(`ThreadSettings.relayed_mcp_servers`) and answers their calls over a socket it opens to the satellite. Every byte
travels Elysium to satellite, as every other call does; Elysium listens for nothing new.

- **Relay**, one per session on an active satellite, beside its session watcher and under the same cancellation, so
  both start, restart, and stop together (`api/src/fleet/relay.rs`). It opens `GET /v1/threads/{id}/relay` and runs
  each call in its own task, so a slow download holds up nothing. A cancellation from the satellite (the call's
  deadline passed, its turn ended) aborts that call's task. Calls are never retried: an agent that wants one again
  calls again.
- A dropped socket reconnects with the session watcher's backoff, 1 to 30 seconds. Calls in flight on it are
  abandoned; the satellite has already failed them to the agent.
- A thread the satellite no longer has ends its relay, as it ends its watcher.
- A thread that declared no relayed servers, such as one whose project had no storage location, refuses the socket
  with `409 RELAY_NOT_DECLARED`. A thread's settings never change, so that refusal ends its relay for good instead
  of reconnecting. Elysium keeps no record of what a thread declared, and reading the thread's settings would cost
  the same one request, so the refusal is how it finds out.

The satellite keeps no calls for a client that is not attached. While Elysium is down or reconnecting, a call to a
relayed tool fails at once, and the agent reads that no client is attached to answer it. Nothing is replayed later.

Logs name each call by session, server, tool, and call id (`coding.relay.call.completed`), never by its arguments,
which can be large.

## Projects

Every coding session belongs to one project, chosen when the session is created. Projects have a unique name and
a description, and are managed on the Projects page in the sidebar. A project cannot be deleted while any
session belongs to it: each session owns a running thread, which is destroyed deliberately by deleting the
session. Deleting a satellite still forgets its sessions, whatever project they belong to.

## Sessions

Sessions are numbered 1, 2, 3, ... in the order they are created, and the number is the session's id everywhere:
routes, events, and the Coding page's URLs. Creating a session reserves its number from the table's identity sequence
(`nextval`) before the thread is opened, then records the row with that number. A create that fails after the
reservation, such as a satellite refusing the thread, leaves a gap in the numbering; numbers are never reused.

The thread carries the number in its metadata as `elysium.session_id`, for anyone reading the satellite directly.
Nothing matches on it: two Elysium installs can share a satellite, and each numbers its sessions from 1. The satellite
watcher finds a session's thread by its thread id among the threads of the session's own satellite.

The satellite's idempotency key is a fresh UUIDv7 for each create request, never the number, since numbers repeat
across installs and after a database reset. It makes the SDK's retries of that one request safe; a client that sends
the create again opens a second thread. If the row cannot be recorded, the API destroys the thread instead of leaving
it running unrecorded.

Every thread is opened with Elysium's policy, constants in `api/src/routes/v1/coding_sessions/mod.rs`:

| Setting                   | Value     | Why                                                               |
|---------------------------|-----------|-------------------------------------------------------------------|
| Idle TTL                  | 7 days    | Satellites require one so forgotten workspaces are collected      |
| Cost ceiling per thread   | 25 USD    | Satellites require an explicit budget; unbounded must be typed    |
| Wall clock per turn       | 60 min    | Ends a stuck turn                                                 |
| Tokens per turn           | unlimited | Bounded by the two ceilings above                                 |

### Repositories

A session clones up to 16 repositories (https, http, ssh, or `git@` URLs), in the order given, each into the
workspace's `repos/` directory under a name taken from the URL's last segment without `.git`. Each may name the base
branch its work starts from; without one the remote's default branch is used.

- **Limit.** The satellite sets none, but clones run one after another before the thread is usable, and the first
  that fails parks the thread with `REPO_CLONE_FAILED`. Sixteen (`MAX_REPOSITORIES`) covers a service with its
  libraries and keeps that bounded.
- **Directory names.** The satellite checks that each name is safe, not that the names differ, so two repositories
  that would share a directory, ignoring case, answer `400` naming both URLs. The same repository listed twice
  (github.com names compared ignoring case, over HTTPS or SSH alike) is refused the same way and says so.
- **SSH.** Elysium holds no SSH keys: a github.com SSH remote is cloned over HTTPS (see `docs/github.md`), and an
  SSH remote on any other host is refused.

The New session form mirrors every rule before sending (`createSessionFormSchema.ts`, `sessionRepositories.ts`).

## Model credentials

A thread is opened with Elysium's stored LLM credentials as its model endpoints, in priority order. The satellite
tries the first and moves to the next when it will not answer. The list is failover, never a pool, so later entries
are only reached when the ones above them are spent.

### Rolling over a spent credential

The point of the stack is a credential that runs out, not one that is briefly busy. A subscription whose quota is
spent keeps refusing for hours, and every second spent waiting on it is a second the credential behind it would
have served. So Elysium declares a shorter retry policy than the satellite's own default of ten attempts over
about six minutes:

| Setting            | Value | Why                                                                        |
|--------------------|-------|----------------------------------------------------------------------------|
| Attempts           | 3     | Enough that a momentary burst limit does not cost a failover               |
| First wait         | 5s    | The satellite's own first wait                                             |
| Longest wait       | 15s   | Also the ceiling a provider's `Retry-After` is clamped to                  |

Only 429, the rate and usage limit, and 529, Anthropic's overload, are waited on at all. Every other refusal rolls
over at once, including a rejected credential (401 or 403) and an unreachable host, because a credential that is
wrong is wrong on the tenth attempt too.

Nothing about a rollover is silent. A request a later credential answers records a `recovered` incident naming
every credential given up on, so a stack quietly running on its fallback is visible. When the list runs out the
turn ends with `LLM_ALL_ENDPOINTS_EXHAUSTED`; the thread and its workspace survive, so a turn submitted after
adding a credential resumes from there.

One list carries one request shape, because failover relays the harness's request body unchanged. The highest
priority usable credential therefore decides the thread's harness and family: `claude-api-token` and
`claude-code-oauth` run Claude, `chatgpt-api-token` and `chatgpt-oauth` run Codex, and the other family's
credentials are left out of that thread.

Inactive and expired credentials are skipped. A `chatgpt-oauth` credential holds a whole `~/.codex/auth.json`, and
the API reads the access and refresh tokens out of it; a file it cannot parse is skipped rather than sent. With no
usable credential the thread declares no endpoint, and the satellite falls back to whatever credential it holds
itself.

Credentials never reach an agent either way: the satellite's proxy strips what the CLI presents and attaches the
real one on the way out. The shaping rules live in `api/src/routes/v1/coding_sessions/model_stack.rs`.

A thread is also opened with an environment: every workspace environment variable, then the session's GitHub token
as `GH_TOKEN` with the git config that lets `git` use it, and that token as the clone credential for each
`https://github.com/` repository. Unlike LLM credentials, the agent can read all of it. See `docs/environment.md`
and `docs/github.md`.

Deleting a session destroys its thread first. A thread that already expired or was destroyed does not block the
delete; any other satellite failure does, so a session is never forgotten while its thread still runs. Deleting a
satellite forgets its sessions without destroying their threads, which end on their idle TTL. Otherwise an
unreachable satellite could never be deleted.

## History

The satellite has no paged history call, so `GET /api/v1/coding-sessions/{id}/events` replays the thread's stream
from the start, stops at the sequence the thread reported when the request began, and returns the latest 5000
events. It gives up after 10 seconds with a 502. Clients merge history with live events on `sequence`.

## Frontend

`frontend/src/pages/Coding/` holds the page. It is a Dockview workspace (`dockview-react`) with two panel types:

- **Sessions** is the overview: every session, its number, project and satellite, live thread state, and last
  activity, in uikit's `SmartTable`. A title opens that session's conversation.
- **Conversation** is one session, headed by its title, satellite, and number ("Local · Thread 12"). It loads history through SWR when it opens and drops its events from Redux when
  it closes; a reopened panel shows SWR's buffered history while the fresh copy loads. Prompts and agent messages
  render as chat bubbles; tool calls, thinking, and turn results as compact lines with details folded away. Event
  types without a renderer show their wire name. Enter sends the composer's prompt; Shift+Enter adds a line. A
  thread that has ended shows no composer.

The first conversation opens beside Sessions; later ones open as tabs in its group. Panels can be dragged, split,
and floated. The layout is saved per browser under `elysium.coding.layout.v2`; Reset layout clears it. A restored
panel whose session was deleted says so.

Panels render through portals, so they read page actions (open, create, rename, delete) from
`CodingActionsContext` rather than props. `src/theme/dockview.css` maps Dockview's variables to the Matter tokens,
so the workspace follows light and dark mode.

## Running a satellite locally

The satellite image is not published. Build it from a clone of `JalapenoLabs/arsox-satellites` (`develop`) with
`docker build --tag arsox-satellite:<commit> .`, then run it from its own compose file outside this repository,
published only on the `docker0` bridge address:

```yaml
services:
  arsox-local:
    image: arsox-satellite:<commit>
    restart: unless-stopped
    ports: [ "172.17.0.1:8090:8080" ]
    env_file:
      - path: arsox-local-secret.conf    # ARSOX_SECRET=<a long random value>
      - path: model-credentials.conf     # ANTHROPIC_AUTH_TOKEN or ANTHROPIC_API_KEY
        required: false
    volumes: [ "arsox-local-db:/var/arsox", "arsox-local-workspace:/workspace" ]
volumes:
  arsox-local-db:
  arsox-local-workspace:
```

Register it as `http://172.17.0.1:8090` with the same secret. The API container reaches the host's bridge address,
so the satellite neither listens on the LAN nor joins Elysium's compose network, which would stop
`docker compose down` from removing that network. Satellites on other machines publish a port on the LAN and are
registered by that address. Without the volumes, a restarted satellite forgets its threads, and Elysium marks
their sessions destroyed.

## Roadmap

- Rendering agent messages as Markdown.
- Answering an agent's questions and deciding proposed plans from the conversation.
- Cancelling a running turn.
- Following the satellite control stream once the Rust SDK exposes it, replacing the status poll.
