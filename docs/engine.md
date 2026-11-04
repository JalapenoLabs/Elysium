# Engine

The engine is the roadmap for everything Elysium does without being asked: it ingests, cleans up, attributes, plans,
and dispatches work, so that a meeting with leadership or a message from a coworker ends with "I'm already on it" and,
hours later, done. Elysium is built around one company or major prospect per instance, and the engine works for that
company.

This doc is the design. Little of it is built; the roadmap is the issues in the Engine milestone. Built so far: the
event stream through Redis pub/sub, and the engine as a service of its own that runs every background loop.

## A service of its own

- The engine is a separate process, `elysium-engine`, a second binary of the API's crate (`api/src/bin/`), shipped in
  the same image and run as its own compose service, `engine`, in every stack. The crate is a library with two thin
  binaries, so each module is compiled once and checked for dead code across both (`api/src/lib.rs`).
- The API serves clients alone: the web app, its `/api`, and MCP clients, and agents' calls to Elysium's tools
  (`/api/agent-tools`). It starts no loop. The engine does all background work and serves no HTTP beyond its health
  check (`GET /health`): the runner and the workspace it prepares, sessions' turns, Elysia's intake and dispatcher,
  the Claude account keeper, the link watcher, connector ingest, the Confluence and YouTrack readers, meetings, every
  read someone asked for, the browser reconciler, the mail server brought back up, the password vault, update checks,
  the nightly database backups, and moving identities onto the current schema (`api/src/engine/`).
- One engine runs: its container name, `elysium-engine`, holds compose to one. Elysia's intake listens to the event
  bus, which every process hears in full, so it must run in exactly one process, and the engine is it.
- Either process can crash and restart without the other. Stopping the engine leaves the web app working: every page
  reads from Postgres, and what the engine would do waits for it. Stopping the API leaves every reader, keeper, and
  turn running; only agents' tool calls, which the API answers, fail meanwhile.
- The event stream runs through Redis pub/sub, so the engine's changes reach every browser through the API exactly as
  the API's own do. The engine publishes with `EventBus::connect` and `publish`, which need no HTTP server, and calls
  `EventBus::close` last on the way out; its in-process listeners, such as Elysia's intake, subscribe the same way the
  API's do (`docs/realtime.md`, The bus).

### How the API and the engine meet

They share nothing but Postgres and the bus. What the API asks of the engine is a row the engine takes, followed by a
signal (`docs/realtime.md`, Signals) so it takes it at once rather than at its next look; a lost signal costs latency,
never work, since every loop also looks on a timer. What the engine knows that a page shows is on a row too.

| The API                                   | The row                                  | The engine                                   |
|-------------------------------------------|------------------------------------------|----------------------------------------------|
| Queues a session's turn, or stops it      | `session_turns`                          | Takes each session's next turn, runs it, finishes it |
| Checks every agent tool call's bearer     | `agent_tool_grants`                      | Grants a turn its tools, revokes them when it ends |
| Asks for a backup (Back up now, `/internal/backups`) | `database_backups.claimed_at` unset | Takes it and runs it                         |
| Asks for a connector to be read (Sync now) | `connectors.syncing_since`              | Reads it with its kind's reader, clears it   |
| Adds a browser, or resets one             | `browser_sessions`                       | Brings its container up; records its state   |
| Answers the updater                       | `process_activities`, `update_checks`    | Records what it runs, and what Docker Hub lists |
| Queues Elysia's messages and answers      | `elysia_turns`                           | Runs them                                    |

- **Taking work** is one engine's, so a row is taken with a conditional update on its state. #75's workflow runs
  generalize this into a queue of step jobs claimed with `FOR UPDATE SKIP LOCKED`, with attempts on the row.
- **Settling after a restart.** An engine that starts finishes what its predecessor cut short: a turn whose CLI had
  started is finished (its history ends where the turn stopped), one still waiting for a slot is queued again, a backup
  it had taken fails, every tool grant is revoked, and every connector still marked syncing is read.
- **The password vault** is the one thing the API asks while a request waits: its CLI and `bw serve` run in the
  engine, on its data volume and its loopback, and the API's routes and agents' password tools ask it over the bus,
  sealed with the encryption key (`docs/passwords.md`, The CLI). An engine that does not answer within 10 seconds
  (3 for its status, 90 for connecting) reads as a vault that is not answering.
- **Docker.** Both reach the filtered `docker-proxy`. The engine creates and replaces everything: the workspace, the
  mail server's reconcile, browsers' containers, and backups' dumps. The API works in what exists while a person
  waits: files in the workspace, cloning a new session's repositories, Claude sign-ins, creating the mail server, and
  resetting or removing a browser. Every change to a browser's containers, in either process, holds one Postgres
  advisory lock (`api/src/database/advisory.rs`).
- **Auto-update** waits for both: the engine mirrors what it runs that a restart would cut short into
  `process_activities` on a lease it renews every 15 seconds (`api/src/activity.rs`), and the API reads every live
  lease before it tells the updater to apply a release. An engine that dies stops counting once its lease runs out,
  60 seconds later.

### One replica of the API

The API can run as more than one replica as far as the engine is concerned: nothing it serves is held in the engine's
memory. What still holds it to one is its own request-scoped state:

- a Claude sign-in's pending CLI, which finishing must reach the replica that started it;
- connector check runs, which a page follows by id;
- the mail server's creation progress;
- the lock that keeps two problem reports of one failure from both opening an issue;
- the rate limiter, whose buckets are per process.

Each moves to Postgres or Redis when the API scales; until then it runs one replica.

## Workflows and steps

- A step is one reusable unit of work with typed input and output: fetch a transcript, transcribe audio, correct a
  transcript, segment it onto projects, triage a conversation, plan for a project, dispatch workers, follow up.
- A workflow is steps composed in code to solve one problem, in whatever order and subset it needs. Steps are named,
  never numbered. A meeting workflow starts by fetching; a Slack request workflow starts at segmenting.
- A run is one workflow on one source. Each step of a run is its own job row with state, attempts, input, and output,
  so one step can be retried, skipped, or resumed alone. Runs show in the web app the way GitHub Actions shows jobs.

## Meetings

- Every source is read deterministically: the transcript, and the recording when there is one. Elysium never uses a
  transcription service's own AI summary or notes, since the model behind them is unknown.
- When there is audio, a local Whisper (faster-whisper, pinned) in its own container produces a second transcript.
  Anthropic offers no transcription for recorded audio, and nothing leaves the host. Every transcript a meeting has is
  an input to correction; more sources mean better corrections.
- Correction is an agent that reconciles the transcripts against the company doc and other meetings and writes a
  corrected copy beside the raw one. The web app shows the two as a diff, and the person may approve it. Approval is
  optional: every later step reads the corrected copy either way, told whether it was approved.
- Each meeting source has "Wait for my approval before acting on meetings", off by default. On, a run stops after
  correction with an action item to review the diff, and continues once approved. One meeting can also be held by
  hand.
- Segmenting attributes spans of the corrected transcript to projects, with their clip and summary, onto each
  project's timeline beside agent work and every other record, so a project reads top to bottom across meetings.

## Conversations

- Every direct message and mention starts work after a 10 second quiet window. The agent handling it may wait another
  30 seconds for more before proceeding, and later messages in that conversation go to the same agent session rather
  than starting another.

## The company doc

- One document per instance knows the company: who it is, its leadership, people and how their names are spelled,
  stakeholders, products, projects, and a glossary of names and jargon. Correction leans on it most.
- A scheduled workflow reads what was ingested since it last ran and proposes changes. Nothing changes the doc until
  the person approves it, and the person edits it directly at any time. It lives in the Knowledge base milestone.

## Project Managers

- Each project has a Project Manager: one Claude Code session, resumed for every turn (`claude -p --resume`) and kept
  indefinitely, relying on Claude Code's own compaction.
- A Project Manager writes to the trackers itself: it creates issues, comments, and status changes in Jira, YouTrack,
  or GitHub, choosing the tracker by the project's precedent unless the project names one. Every write is recorded on
  the project's timeline with what caused it. A per-project switch makes it propose changesets instead.
- It turns its issues into work batches and submits them to the engine with a priority: ASAP, normal, or a scheduled
  time.

## Workers and the pool

- A worker is Claude Code in a container of its own, from the workspace image, with its own volume, desktop, and
  network namespace. A worker is what a slot in the pool means. Project Managers, triage, and correction run as turns
  in the shared workspace.
- The pool is global, not per project. Its size is an admin setting whose default is one worker per CPU core minus two
  for the API, the engine, and Postgres, at least one, with no maximum.
- Project Managers never start containers. The engine's scheduler hands out free slots: due scheduled work first, then
  ASAP, then normal, oldest first within each, round robin across projects. It hands out none while every Claude
  account is at its usage limit, and resumes at the earliest reset. The person and Elysia can reorder, pause, or
  cancel anything queued.
- A project declares its repositories, each with an optional setup script, and one setup script for tools. A batch
  names the repositories it needs; the engine clones them with `--recurse-submodules`, runs their setup, and starts
  Claude Code with `~/.claude` from the configuration repository. A submodule the token cannot read stops setup with
  a message naming it. Workers may clone more themselves.
- A worker runs its batch to completion (`/goal`, `/steamroll-issues`) and keeps its container while it waits, on a
  review, CI, or a question, for days if need be.
- Each repository sets who merges: auto-merge once CI is green with no open review threads, failing closed when review
  state cannot be read, or wait for a human. Waiting is the default.

## Models

- Model and effort are global settings per step, defaulting to Opus 5.5 at high effort, running through the same
  credential failover stack and usage-aware skipping as every turn.

## Follow-ups

- When a step finds a promise the person made ("I'll let you know on Slack when it's done"), it is recorded on the
  project with who it is for and where they asked. When the Project Manager sees that work finish, it drafts the
  message in the person's voice as an action item with Send, Edit, and Dismiss. Send posts it through the connector it
  came from, as the person. Nothing is sent without that click. The drafted message is a draft
  (`docs/connectors.md`, Sending) answering the record the promise was made in, and Send is `crate::drafts::send`.

## Elysia

- Elysia does not orchestrate the engine. Asked, she can see and change anything in it: inspect runs, pause, cancel,
  or reorder work, and talk to a project's Project Manager.
- Questions for the person, from any step, become action items.
