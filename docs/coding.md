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

## Projects

Every coding session belongs to one project, chosen when the session is created. Projects have a unique name and
a description, and are managed on the Projects page in the sidebar. A project cannot be deleted while any
session belongs to it: each session owns a running thread, which is destroyed deliberately by deleting the
session. Deleting a satellite still forgets its sessions, whatever project they belong to.

## Sessions

Creating a session generates its id first and sends it as the satellite's idempotency key, so a retried create
never opens a second thread. The thread also carries metadata `elysium.session_id`. If the row cannot be recorded,
the API destroys the thread instead of leaving it running unrecorded.

Every thread is opened with Elysium's policy, constants in `api/src/routes/v1/coding_sessions/mod.rs`:

| Setting                   | Value     | Why                                                               |
|---------------------------|-----------|-------------------------------------------------------------------|
| Idle TTL                  | 7 days    | Satellites require one so forgotten workspaces are collected      |
| Cost ceiling per thread   | 25 USD    | Satellites require an explicit budget; unbounded must be typed    |
| Wall clock per turn       | 60 min    | Ends a stuck turn                                                 |
| Tokens per turn           | unlimited | Bounded by the two ceilings above                                 |

An optional repository URL (https, http, ssh, or `git@`) is cloned into the workspace, into a directory named after
the URL's last segment. Models and model credentials come from the satellite's own configuration.

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

- **Sessions** is the overview: every session, its project and satellite, live thread state, and last activity,
  in uikit's `SmartTable`. A title opens that session's conversation.
- **Conversation** is one session. It loads history through SWR when it opens and drops its events from Redux when
  it closes; a reopened panel shows SWR's buffered history while the fresh copy loads. Prompts and agent messages
  render as chat bubbles; tool calls, thinking, and turn results as compact lines with details folded away. Event
  types without a renderer show their wire name. Enter sends the composer's prompt; Shift+Enter adds a line. A
  thread that has ended shows no composer.

The first conversation opens beside Sessions; later ones open as tabs in its group. Panels can be dragged, split,
and floated. The layout is saved per browser under `elysium.coding.layout.v1`; Reset layout clears it. A restored
panel whose session was deleted says so.

Panels render through portals, so they read page actions (open, create, rename, delete) from
`CodingActionsContext` rather than props. `src/theme/dockview.css` maps Dockview's variables to the Matter tokens,
so the workspace follows light and dark mode.

## Running a satellite locally

The satellite image is not published. Build it from a clone of `JalapenoLabs/arsox-satellites` (`develop`) with
`docker build --tag arsox-satellite:dev .`, then run it on the compose network so the API can reach it:

```sh
docker run -d --name arsox --network elysium_default \
  --env ARSOX_SECRET=<a long random value> \
  --env ANTHROPIC_API_KEY=<model credential> \
  --volume arsox-db:/var/arsox --volume arsox-workspace:/workspace \
  arsox-satellite:dev
```

Register it as `http://arsox:8080` with the same secret. Without the volumes, a restarted satellite forgets its
threads, and Elysium marks their sessions destroyed.

## Roadmap

- Rendering agent messages as Markdown.
- Answering an agent's questions and deciding proposed plans from the conversation.
- Cancelling a running turn.
- Passing Elysium's stored LLM credentials to threads, instead of relying on each satellite's own.
- Following the satellite control stream once the Rust SDK exposes it, replacing the status poll.
