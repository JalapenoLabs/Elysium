# Engine

The engine is the roadmap for everything Elysium does without being asked: it ingests, cleans up, attributes, plans,
and dispatches work, so that a meeting with leadership or a message from a coworker ends with "I'm already on it" and,
hours later, done. Elysium is built around one company or major prospect per instance, and the engine works for that
company.

This doc is the design. Little of it is built; the roadmap is the issues in the Engine milestone.

## A service of its own

- The engine is a separate process, `elysium-engine`, built from the same Rust crate as the API and run as its own
  compose service. The API serves clients alone (the web app, MCP clients); the engine does all background work.
- Every background loop the API runs today moves into the engine: the link watcher, connector ingest, the meeting
  reader, the Claude account keeper, the vault keeper, the browser reconciler, mail server hosting, update checks, the
  nightly database backups, and Elysia's dispatcher, along with the runner and the worker pool. Either process can crash and restart without the
  other.
- Postgres is the job queue: steps are claimed with `FOR UPDATE SKIP LOCKED`, with attempts and states on the row.
- The event stream moves to Redis pub/sub, so the engine's changes reach every browser through the API exactly as the
  API's own do.

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
  came from, as the person. Nothing is sent without that click. Connectors gain send actions for this.

## Elysia

- Elysia does not orchestrate the engine. Asked, she can see and change anything in it: inspect runs, pause, cancel,
  or reorder work, and talk to a project's Project Manager.
- Questions for the person, from any step, become action items.
