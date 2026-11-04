# Engine

The engine is the roadmap for everything Elysium does without being asked: it ingests, cleans up, attributes, plans,
and dispatches work, so that a meeting with leadership or a message from a coworker ends with "I'm already on it" and,
hours later, done. Elysium is built around one company or major prospect per instance, and the engine works for that
company.

This doc is the design. Little of it is built; the roadmap is the issues in the Engine milestone. Built so far: the
event stream through Redis pub/sub, the engine as a service of its own that runs every background loop, workflow runs
on a Postgres queue of step jobs with the Runs page (Runs, below), the step and workflow framework over that queue,
with each step's model setting (Workflows and steps, and Models, below), and the meeting workflow's first step, which
fetches each meeting (Meetings, below).

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
| Retries, skips, or resumes a run's step   | `step_jobs`                              | Claims each due step, runs it, records it    |

- **Taking work** is one engine's, so a turn, a backup, or a sync is taken with a conditional update on its state.
  A run's steps are a queue of their own, claimed with `FOR UPDATE SKIP LOCKED` under a lease, with attempts on the
  row, so they stay correct with any number of claimers (Runs, below).
- **Settling after a restart.** An engine that starts finishes what its predecessor cut short: a turn whose CLI had
  started is finished (its history ends where the turn stopped), one still waiting for a slot is queued again, a backup
  it had taken fails, every tool grant is revoked, and every connector still marked syncing is read. A run's step that
  an engine still held when it died is claimed again once its lease lapses.
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

Built: the framework (`api/src/workflows/step.rs` and `workflow.rs`), on the queue below, and the first production
workflow, `meeting` (`api/src/meetings/workflow.rs`), whose one step so far is `fetch_meeting`. The engine registers
every production step in `api/src/engine/mod.rs` and checks each declared workflow against them there. Tests declare
their own.

### A step

- A step is a type implementing `workflows::Step`: its name (`NAME`, `snake_case`), the input it takes and the output
  it gives (serde types), and `run`, one attempt, which answers `Done::Succeeded` (its output and a one-line summary),
  `Done::Held` (its output and what a person should look at), or a `StepFailure` (its reason, and whether another
  attempt may get past it).
- A step is given its input and a `StepContext`: its run's id and its own row's id, which attempt this is, a log, and
  its model setting (Models, below). It is never told which workflow runs it, or its run's source: what it needs is in
  its input. One instance runs every attempt, so it holds what it needs (a pool, a client, the runner) and keeps no
  state of one attempt, and works idempotently, since one success may take more than one attempt.
- Every step this build runs is registered once, by name, in `workflows::Steps`, with what it needs, as the engine
  starts. `Steps::assert_runs` checks a workflow against it: a step the workflow names that is not registered, or is
  registered as another type by that name, is a programming error that stops the engine at once rather than failing
  runs.

### A workflow

- A workflow is declared in code from step types, first to last:
  `workflows::declare("meeting").first::<FetchTranscript>().then::<CorrectTranscript>().build()`. Each `then` takes only
  a step whose input is the type the step before it gives, so a declaration that compiles hands every step what it
  takes. Any steps compose, in any order and subset their types allow, and one step serves many workflows.
- A workflow holds names in order, never instances, so the API can start runs as well as the engine.
- A run starts from a workflow's first step (`Workflow::new_run`) or enters it at any later step it declares
  (`Workflow::new_run_at::<Step>`), with that step's own input type, then goes to `workflows::start`. Its steps are
  the workflow's from there on; the ones before are not part of the run. Entering at a step the workflow lacks is a
  programming error.

### Where types meet the queue

- The queue stores each step's input and output as JSON. Registering a step is where the JSON becomes its types again:
  before each attempt its row's input is read as the step's input type, and its output is written back as JSON.
- Input that does not read as the step's type fails the step for good, with a reason naming it, rather than reaching
  it. Only a person's skip can cause that inside a declared workflow: the step after a skipped one receives the
  skipped step's input. A step a person may want to skip, such as correction, is written to take and give one type,
  so skipping it hands the next step what it reads.

## Runs

Built: the queue every workflow runs on (`api/src/workflows/`, `api/src/models/workflow_run.rs` and `step_job.rs`), and
the Runs page.

### A run and its steps

- A run (`workflow_runs`) is one workflow, by its name in code, on one source: a kind and an id, such as a meeting. It
  holds the input its first step receives and who or what started it (`created_by`: a person, or the system user or
  Elysia for what Elysium starts itself).
- A run is started with its steps' names in order (`workflows::start`, given what a declared workflow builds), and each
  becomes a job row (`step_jobs`). The
  first receives the run's input; each after it receives the output of the step before, or that step's own input when
  it was skipped. Steps are named; their order is a position the web app never shows.
- A workflow has one live run per source until it succeeds: starting it again answers the run already there, so
  whatever notices a source may start its run every time it does. Once a run succeeds, the source may run again.
- A run's state is derived from its steps whenever one moves, in the same transaction: `queued` until a step starts,
  `running`, `held` while a step waits for a person, `failed` while a step failed for good (the run carries its
  reason), and `succeeded` once every step succeeded or was skipped. Every write to a run or its steps locks the run's
  row first, so the engine's writes and a person's never deadlock.

### Claims, leases, and attempts

- The engine's step workers (`api/src/workflows/workers.rs`) claim due steps with `FOR UPDATE SKIP LOCKED`, four at a
  time per engine, oldest first: queued steps whose backoff has passed, and running steps whose lease lapsed. A claim
  sets a new token and a 60 second lease, renewed every 15 seconds while the step runs, and counts an attempt. Every
  write of an attempt (its log, its renewal, how it ended) is fenced by its token, so an engine whose lease lapsed
  records nothing over the attempt that replaced it, and stops its step when a renewal finds the claim gone.
- An engine that dies stops renewing, and its steps are claimed again once their leases lapse; the lost attempt
  counts. One that stops cleanly drops its running steps where they are and gives them back queued, the attempt
  uncounted, for the next engine to take at once.
- A step that fails retryably is tried again after 30 seconds, doubling each attempt up to ten minutes, three attempts
  in all. Then, or when it fails for good, the step fails and its run stops with the reason. A step whose engine died
  on every attempt is failed the same way. Every failed attempt's reason is written to the step's log.
- A step may therefore run more than once for one success, so every step does its work idempotently.
- The workers look at the queue when the API signals `workflows`, when a step they ran ends, when the next backoff
  ends, and every 30 seconds in case a signal was lost. A running step holds a `workflowStep` activity, so auto-update
  waits for it.
- The limits (attempts, lease, backoff, concurrency, the log's size) are constants in code.

### Steps on the queue

- A claimed step is found by its name in `workflows::Steps` and called with its row's input as JSON and its
  `StepContext` (A step, above), whose log is stored every second, the newest 64 KiB kept across attempts. The worker
  reads nothing else for it. A run naming a step this build lacks fails that step.
- An attempt ends succeeded, with its output and a one-line summary its row collapses to; held, with its output and
  what a person should look at, which stops the run without failing it; or failed, with its reason and whether it may
  be retried.

### A person's actions

- **Retry** a failed step: it is queued again with a fresh set of attempts, and its run goes on from it.
- **Skip** a step that has not run, failed, or is held: the step after it receives the input it would have.
- **Resume** a held step: it succeeds with the output it was held with, and the run goes on.
- Each is a write through `/api/v1/runs` (a member's session, Elysium's own origin), recorded in the run's history
  (`workflow_run_events`) with who did it, then announced, and signalled to the engine. A run's start is recorded
  there too. Entries' `data`: `started` holds `{ steps }`, `retried` the `{ error }` it had failed with, `skipped` the
  `{ from }` state, and `resumed` nothing.
- Runs read what was ingested, so they answer the company's members alone, as the knowledge base does.

### Progress

- Every change reaches clients as `workflowRun.changed` (a run's id and state) and `stepJob.changed` (a step's id, its
  run, and its state, also sent when it logs), never input, output, or logs (`docs/realtime.md`). The web app loads
  what it shows by id.
- The Runs page (`/runs`) lists runs newest first; a run's page shows each step like a GitHub Actions step, collapsed
  to its state, summary, and duration, expanding to its log (`docs/frontend.md`, Runs).

## Meetings

- Every source is read deterministically: the transcript, and the recording when there is one. Elysium never uses a
  transcription service's own AI summary or notes, since the model behind them is unknown.
- Built: each meeting is a run of the `meeting` workflow, whose `fetch_meeting` step reads every meeting source the
  same way, stores the raw transcript with its SHA-256, and uploads the recording; running it again for the same
  meeting changes nothing. Segmenting still runs in the meetings loop until it becomes a step here. See
  `docs/meetings.md`, Processing.
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

- Model and effort are global settings per step, defaulting to Opus 5.5 (`claude-opus-5-5`) at high effort, running
  through the same credential failover stack and usage-aware skipping as every turn.
- Built: the setting and where steps read it. `step_model_settings` holds a row for each step an admin changed (its
  model, and its effort as the CLI's `--effort` names it: `low`, `medium`, `high`, `xhigh`, or `max`); a step without
  one runs on the default in code (`api/src/models/step_model_setting.rs`).
- A step whose work is an agent turn asks for its setting as the turn starts (`StepContext::model`), which reads it
  then and writes it to the step's log, so a change applies from the next turn, a retry's included. It passes both to
  the runner (`TurnRequest::model` and `effort`).
- `step_model_setting::for_step` is the one place a step's model is resolved. The admin settings page that lists each
  step and agent with its model and effort is on the roadmap, and so are overrides per agent kind and per project;
  both change what it answers, never what a step does.

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
