# Engine

The engine is the roadmap for everything Elysium does without being asked: it ingests, cleans up, attributes, plans,
and dispatches work, so that a meeting with leadership or a message from a coworker ends with "I'm already on it" and,
hours later, done. Elysium is built around one company or major prospect per instance, and the engine works for that
company.

This doc is the design. Little of it is built; the roadmap is the issues in the Engine milestone. Built so far: the
event stream through Redis pub/sub, the engine as a service of its own that runs every background loop, workflow runs
on a Postgres queue of step jobs with the Runs page (Runs, below), the step and workflow framework over that queue,
with the model and effort every kind of agent, step, and agent runs on, workspace-wide and per project (Workflows and
steps, and Models, below), the meeting workflow, which fetches each meeting, transcribes its recording with a local
Whisper, corrects its transcript against everything Elysium knows, waiting for its person's approval when asked, and
segments it onto projects' timelines, a step that segments a conversation too (Meetings, below), the request workflow,
which gathers every direct message and mention per conversation and starts work once it goes quiet (Conversations,
below), and the company doc's scheduled review (The company doc, below).

## A service of its own

- The engine is a separate process, `elysium-engine`, a second binary of the API's crate (`api/src/bin/`), shipped in
  the same image and run as its own compose service, `engine`, in every stack. The crate is a library with two thin
  binaries, so each module is compiled once and checked for dead code across both (`api/src/lib.rs`).
- The API serves clients alone: the web app, its `/api`, and MCP clients, and agents' calls to Elysium's tools
  (`/api/agent-tools`). It starts no loop. The engine does all background work and serves no HTTP beyond its health
  check (`GET /health`): the runner and the workspace it prepares, sessions' turns, Elysia's intake and dispatcher,
  the Claude account keeper, the link watcher, connector ingest, the conversation loop, the Confluence and YouTrack
  readers, meetings, every
  read someone asked for, the knowledge base's embedder, backfill, and answerer, the browser reconciler, the mail server brought back up, the password vault, update checks,
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
| Asks the knowledge base a question        | `knowledge_asks`                         | Takes each queued ask, answers it, ends it   |

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
  mail server's reconcile, browsers' containers, backups' dumps, and a container per transcription. The API works in
  what exists while a person waits: files in the workspace, cloning a new session's repositories, Claude sign-ins,
  creating the mail server, and resetting or removing a browser. Every change to a browser's containers, in either process, holds one Postgres
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
- the rate limiter, whose buckets are per process;
- the agent guidance kept in memory, which a save on another replica would not replace (`api/src/agent_guidance/`).

Each moves to Postgres or Redis when the API scales; until then it runs one replica.

## Workflows and steps

- A step is one reusable unit of work with typed input and output: fetch a transcript, transcribe audio, correct a
  transcript, segment it onto projects, triage a conversation, plan for a project, dispatch workers, follow up.
- A workflow is steps composed in code to solve one problem, in whatever order and subset it needs. Steps are named,
  never numbered. A meeting workflow starts by fetching; a Slack request workflow starts at segmenting.
- A run is one workflow on one source. Each step of a run is its own job row with state, attempts, input, and output,
  so one step can be retried, skipped, or resumed alone. Runs show in the web app the way GitHub Actions shows jobs.
- Work with no source of its own is a loop of the engine, not a workflow. Embedding the knowledge base's passages is
  the same small job for every passage of every source, so the embedder is a loop over the index itself
  (`docs/knowledge.md`, The embedder); a run per document would fill the Runs page with one-step runs. The backfill
  that indexes the timeline records stored before they were indexed on write is a loop for the same reason
  (`docs/knowledge.md`, The backfill).

Built: the framework (`api/src/workflows/step.rs` and `workflow.rs`), on the queue below, and three production
workflows: `meeting` (`api/src/meetings/workflow.rs`), whose steps are `fetch_meeting`, `transcribe_meeting`,
`correct_transcript`, and `segment_meeting`; `request` (`api/src/conversations/workflow.rs`), which starts at
segmenting, `segment_meeting` then `handle_request`; and `company_doc_review` (`api/src/company_doc/workflow.rs`), one
step, `review_company_doc`.
The engine registers every production step in `api/src/engine/mod.rs` and checks each workflow in
`workflows::declared` against them there, and every step it registers against those workflows. Tests declare their
own.

### A step

- A step is a type implementing `workflows::Step`: its name (`NAME`, `snake_case`), the kind of agent its work is when
  it is an agent turn (`AGENT_KIND`, Models below), the input it takes and the output it gives (serde types), and
  `run`, one attempt,
  which answers `Done::Succeeded` (its output and a one-line summary), `Done::Held` (its output and what a person should look at), `Done::Skipped` (why it had nothing to do), or a
  `StepFailure` (its reason, and whether another attempt may get past it).
- A step that skips itself, such as transcription for a meeting with no recording, ends `skipped` with its reason as
  its summary and gives nothing, like a person's skip: the step after it receives its input. So a step that may skip
  itself gives its input's type, or a type its input also reads as.
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
  what a person should look at, which stops the run without failing it; skipped, with why it had nothing to do, which
  the run passes over as it does a person's skip; or failed, with its reason and whether it may be retried.

### A person's actions

- **Retry** a failed step: it is queued again with a fresh set of attempts, and its run goes on from it.
- **Skip** a step that has not run, failed, or is held: the step after it receives the input it would have.
- **Resume** a held step: it succeeds with the output it was held with, and the run goes on. What knows how a
  person's decision changes that output resumes it with the changed one instead (`step_job::resume_with`), as
  approving a corrected transcript does, so the step after it is told it was approved.
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
  meeting changes nothing. The meetings loop only lists meetings and starts their runs; whether a meeting failed, and
  why, is its run's, so a person acting on the run's page moves the meeting at once. See `docs/meetings.md`,
  Processing.
- Built: when there is audio, the run's `transcribe_meeting` step produces a second transcript, word by word, with a
  local Whisper (faster-whisper with Whisper large-v3, pinned and checked) in a container of its own with no network,
  and stores it beside the source's. Anthropic offers no transcription for recorded audio, and nothing leaves the host.
  A meeting without audio skips the step, saying so. See `docs/meetings.md`, Transcription. Every transcript a meeting
  has is an input to correction; more sources mean better corrections.
- Built: correction, the run's `correct_transcript` step, is an agent that reconciles the transcripts against the
  company doc and other meetings and writes a corrected copy beside the raw ones, which it never changes. Its text
  turns' answers are checked word by word, so the copy keeps the transcript's every line, speaker, and time and only
  fixes misheard names and terms. The web app shows the two as a diff on the meeting's page, and the person may
  approve it. Approval is optional: every later step reads the corrected copy either way, told whether it was
  approved (its output's `approved`). See `docs/meetings.md`, Correction.
- Built: each meeting source has "Wait for my approval before acting on meetings", off by default. On, a run stops
  after correction (`Done::Held`) with an inbox item to review the diff, and continues once approved, the output
  saying so; Resume on the Runs page passes the held output on unapproved. One meeting can also be held by hand.
- Built: segmenting, the run's `segment_meeting` step, cuts the corrected transcript into topics in one text turn, each
  with its span, summary, and projects with Elysia's confidence, cuts each topic's clip at the words Whisper heard
  nearest its ends, and puts every topic on the timeline as a record of its own, attributed to its projects, beside
  agent work and every other record, so a project reads top to bottom across meetings. The person corrects what a
  topic is about from its meeting's page or a project's timeline, every change recorded in history and undoable, and
  Elysia never overrides them. Its input also takes a conversation, records already on a person's timeline, and the
  request workflow starts at this step (Conversations, below); a conversation's topics attribute its messages. See
  `docs/meetings.md`, Segmentation.

## Conversations

Built: every direct message and mention starts work after a 10 second quiet window, as a run of the `request`
workflow. The agent handling it may wait another 30 seconds for more before proceeding, and later messages in that
conversation go to the same agent session rather than starting another. The code is `api/src/conversations/`; the
rows are `conversations` and `conversation_messages` (`docs/database.md`).

### Gathering

- Each reader of messages names the conversation a message was said in (`connectors::Conversation`: a key in its
  kind's own terms, and what the conversation is called), so a request gathers with the messages answering it:

  | Kind              | One conversation                                                        | Key                                  |
  |-------------------|-------------------------------------------------------------------------|--------------------------------------|
  | Slack             | A direct or group message; in a channel, one thread                     | `team/conversation`, or `team/channel/<thread's first ts>` |
  | Teams             | A chat (one-on-one, group, or a meeting's); in a channel, one thread    | The chat's id, or `<channel>;messageid=<thread's first message>` |
  | Outlook and Gmail | One sender's mail under one subject, its `Re:`, `Fwd:`, `AW:` aside     | The sender and the subject, lowercased |

  A channel's top-level message starts its own thread, which is where replies to it go. Mail is threaded by sender and
  subject because the page scripts read no thread id a reply shares with what it answers.
- A message that wants attention (a direct or group message, a mention, a reply in a thread the person follows, an
  unread mail; `docs/connectors.md`, Ingest) joins its conversation in the transaction that records it, making the
  conversation when it is new (`conversations::gather`). One that wants none, such as the person's own reply, joins
  only a conversation being worked on (a message waits in it, or its run is live), as context; otherwise it starts
  nothing. Only a baselined connector's messages gather, as only they raise items, so connecting an account starts no
  work on its backlog. Confluence and YouTrack records are not messages and gather nowhere.
- A message that wakes signals `conversations`, and its `timelineRecord.created` names the conversation
  (`conversationId`), which is how Elysia's intake leaves it to the run; she skips the inbox item its watcher raised
  for it too (`docs/elysia.md`, Triggers).

### The quiet window

- A conversation is owed a run once it has gone 10 seconds (`QUIET_WINDOW`) without a new message and holds a
  message no run has taken that wakes, or that a live run's agent was given (below). The window runs from when
  Elysium read the newest message, kept on its row, not a timer: the engine's conversation loop
  (`conversations::Conversations`) looks when ingest signals, when the next conversation goes quiet, and every 30
  seconds, and an engine that restarts finds exactly the conversations its predecessor would have
  (`conversations::quiet`, a pure function).
- Starting a run is a claim: under the conversation's row lock, in one transaction, the loop reads its waiting
  messages again, starts the run, and marks them taken by it (`conversation_messages.run_id`). Two looks never take
  one message, and none is taken by a run that was not stored. A run takes at most 50 messages, and reads up to 20
  taken before them again for context.
- A conversation has one live run, as every workflow's source does. A quiet conversation whose run is still working
  is looked at again every 5 seconds; one whose run failed or is held waits, as a meeting's does, until a person
  retries or skips its step.

### The request workflow

`request` (`api/src/conversations/workflow.rs`) is about one conversation (source `conversation`, its id) and starts
at segmenting, since a request has nothing to fetch, transcribe, or correct. Its run is started by the system user.

| Step              | Kind         | What happens                                                                  |
|-------------------|--------------|-------------------------------------------------------------------------------|
| `segment_meeting` | `segmenting` | Cuts the messages it reads into topics, attributing each to its topic's projects (`docs/meetings.md`, Segmentation) |
| `handle_request`  | `triage`     | Gives the conversation's agent what it has not seen, and raises an inbox item for each question or request the person owes |

`handle_request` (`api/src/conversations/handle.rs`; its prompt and the checks on its answers are `handling.rs`):

- **One session per conversation.** The agent is one Claude session, kept on the conversation's row and resumed by
  every turn of every run, so it remembers what it raised and what the person answered. Its turns are text in, text
  out with the session kept (`TurnMode::ResumableText`, `docs/coding.md`): no tools and no settings, so it can neither
  send nor change anything. Its runner conversation is the conversation's id, so two turns of one never overlap.
- **Turns.** Each turn gives the agent every message of the conversation it has not been given, oldest first, up to
  50, numbered for that turn; a run's first turn also names what segmenting found (each topic's title, summary, and
  projects). Then the step looks again: a message that arrived while the turn ran goes to the same session in the next
  turn, not to a new run. The step ends when nothing is left to give, after at most six turns (`MAX_TURNS`), so a chat
  that never stops holds a worker slot for a bounded time and its newest messages pass to the next run. The messages
  it gave that no run took are what that next run's segmenting reads; its agent, which has seen them, skips the step.
- **The answer** is `{ "wait": true }`, or `{ "items": [{ "title", "notes", "messages" }] }`: one item for each
  question someone asked the person and each thing someone asked them to do that the person has not answered or done
  in the conversation, naming the messages it comes from. Greetings, thanks, and information raise nothing. Nothing in
  it is trusted: an item needs a title (at most 200 characters), names only messages its turn gave, and one answer
  raises at most five. An answer with no JSON, or of the wrong shape, fails the attempt, which the run's backoff tries
  again.
- **Waiting.** When the newest message reads as unfinished ("one sec", "also..."), the agent may ask to wait, once per
  run (`conversations.waited_in_run_id`). The step records the wait's end on the conversation (`waiting_until`, 30
  seconds on), waits it out, then gives the agent what arrived meanwhile, or tells it nothing did, and asks again. The
  end is a row, so an attempt that takes over after its engine stopped waits only what is left of it.
- **Once.** Each turn's session, the messages it was given (`delivered_at`), its wait, and the items it raised land in
  one transaction under the conversation's lock. An attempt that takes over gives the agent only what no landed turn
  gave, so no message is given twice and no item raised twice; a turn cut short before it landed is asked again,
  resuming the session as it was.
- **Items.** Each is an inbox item raised by Elysia, owned by the person: what they owe as its title, the agent's
  notes, then the messages it came from quoted under the conversation's title with a link to the last, in the
  projects segmenting attributed those messages to with a confidence of at least 0.5. An inbox item is the person's to
  accept or dismiss, so raising one changes nothing in their name; nothing is ever sent, and answering is the person's
  own Send (`docs/connectors.md`, Sending).
- **What it logs.** Turn and message counts and how many items each turn raised, never a title or a word said.

What it gives (`Handled`) is the conversation, its turns and messages, and the items it raised.

### Roadmap

- Ingest still raises an inbox item for each message that wants attention, so a direct message's question shows as
  that message's item and as the agent's. Whether the agent's item replaces the message's is open.
- Drafting a reply with each item, for the person to Send.
- A reply the person sends through Elysium joining its conversation as context: today a sent message's record is
  written by `crate::drafts`, which gathers nothing, and reading it back records nothing twice, so the agent never
  sees it.
- Readers that hear a message as it arrives (IMAP `IDLE`, webhooks): the window is already measured from when a
  message is read, so a reader faster than once a minute makes it matter.

## The company doc

- One document per instance knows the company: who it is, its leadership, people and how their names are spelled,
  stakeholders, products, projects, and a glossary of names and jargon. Correction leans on it most. The doc itself,
  its proposals, and its history are designed in `docs/knowledge.md`, The company doc.
- Built: a scheduled workflow, `company_doc_review`, reads what was ingested since it last ran and proposes changes.
  Nothing changes the doc until a person approves each one, and people edit it directly at any time.

### The review

- **One run at a time.** The workflow runs on one source, the doc (`company_doc` / `company_doc`), so it has one live
  run. A run's input is the instant it reads up to, two minutes before it started: a record is stamped when the
  transaction that ingests it starts, so one committed a moment late is never passed by.
- **Where it reads from.** `company_doc_review_cursor` holds the last record read, by ingestion time and id. A run
  reads every owner's records after it, oldest first (`timeline_records.created_at`), never further back than 14 days
  before its window ends; the first review, which has no cursor, reads those 14 days.
- **A turn.** Each turn is one text turn on the runner (`TurnMode::Text`: no tools, no settings) on the step's model
  setting, which it reads once there is something to read. Its prompt is the doc whole, the newest 30 pending and 30
  rejected proposals as compact diffs, and as many records as fit, each body cut to 1,500 characters, under 120,000
  bytes, since the runner passes the prompt as one argument and Linux allows one argument 128 KiB. A doc that leaves
  too little room fails the step for good, saying so. The prompt and the checks on the answer are
  `api/src/company_doc/review.rs`.
- **Nothing in the answer is trusted.** Each proposal must name a section, hold edits and a reason, and be the first
  for its section; it cites a record only when that record was offered, and its source is written from the record.
  Each is then proposed like any agent's, so an edit whose text the section does not hold is left out, and so is a
  change people already rejected. What was left out, and why, is written to the step's log.
- **Once each.** A turn's proposals and the cursor's move to the last record it read commit in one transaction, and
  the cursor moves only from where the turn started, so an attempt that runs again after its engine stopped proposes
  nothing twice.
- **How much.** A run takes at most three turns; what is left waits for the next run, so one run never spends a day's
  usage at once. A run that read nothing skips its step, saying so.
- **When.** The engine's schedule (`company_doc::workflow::schedule`) looks every 10 minutes and starts a run, as
  Elysia, when none ran yet or the last one succeeded at least six hours before, and something was ingested since
  the cursor. A run still live, failed and waiting on a person, holds the next one back, as every workflow's live run
  does. People start one at once from the Company doc page's Look for updates (`POST /api/v1/company-doc/review`),
  which answers the live run when there is one.
- **What it logs.** How many records it read and what it proposed to which section, never a word of the records:
  every member reads every run's log, and a record is its owner's. The proposals themselves quote what they cite,
  which the knowledge base shares with every member (`docs/knowledge.md`, Who sees what).
- The limits (interval, window, turns, prompt budget) are constants in `api/src/company_doc/`.

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

Every agent runs on a model and effort people may choose, through the same credential failover stack and
usage-aware skipping as every turn (`docs/coding.md`, Credentials). Built: the kinds of agent, the settings at each
level, their resolver, the admin page, and each project's own page (`api/src/agent_models.rs`).

- **What has a setting.** An agent is anything whose work is a turn on the runner, named as it is in code: a workflow
  step whose work is an agent turn, by its step name, and the agents outside the step framework: Elysia
  (`elysia`), coding sessions (`coding_session`), Studio sessions (`studio_session`), turn summaries
  (`turn_summary`), and the knowledge base's answers to questions (`knowledge_ask`).
- **Kinds.** Every agent belongs to one kind (`agent_models::AgentKind`, stored as the enum `agent_kind`), the one
  typed list of what sort of work a turn is:

  | Kind              | Its agents today                                   | Runs for one project |
  |-------------------|----------------------------------------------------|----------------------|
  | `worker`          | Coding sessions, Studio sessions                   | Yes                  |
  | `project_manager` | None yet (#84, #85)                                | Yes                  |
  | `correction`      | Transcript correction (`correct_transcript`)       | No                   |
  | `segmenting`      | Segmenting meetings and conversations (`segment_meeting`) | No            |
  | `triage`          | Handling a conversation's messages (`handle_request`), and answers asked of the knowledge base (`knowledge_ask`) | No |
  | `elysia`          | Elysia, and the company doc's review, done in her name | No               |
  | `summaries`       | Turn summaries                                     | Yes                  |

  An answer from the knowledge base is triage's: like deciding what a message asks for, it reads what was said across
  the workspace for no one project, and a person acts on what it says, so it runs on Opus 5.5 at high effort, which
  cites faithfully, unless an admin picks a faster model for it on its own row. A worker today is a coding or Studio
  session in the shared workspace; the pooled workers in containers of their own
  (#82) are workers too, so they name `worker` rather than a kind of their own. Correction, segmenting, and triage read
  meetings and conversations that may concern any number of projects, and Elysia works for the whole workspace, so
  only workers, Project Managers, and summaries run for one project.
- **Attaching to a kind.** A step says which kind its work is (`Step::AGENT_KIND`, `None` for one that only fetches,
  transcribes with the local Whisper, or indexes: it runs no model of Claude's, has no setting, and the page says so;
  one that reads its model while declaring none is a programming error). An agent outside the step framework says so
  in `Agent::kind`. A new kind is added to `AgentKind`, the enum's migration, and the web app's words.
- **The catalog.** `agent_models::catalog` lists every step of every workflow in `workflows::declared`, then every
  agent, each with its kind. That list is also the one the engine checks its registered steps against as it starts,
  and it refuses to start while a step it registers is named by none of them, so no step runs unlisted. A new workflow
  is added there.
- **Defaults** belong to kinds: Opus 5.5 (`claude-opus-5-5`) at high effort, except summaries, which run on Haiku 5.5
  at low effort: a summary is a sentence or two about a turn already done, written for every turn.
- **Resolving.** `agent_models::for_step` and `for_agent` are the one place a turn's model is resolved, given the
  project the turn works for. The first of these that is set wins (`agent_models::resolve`, a pure function tested on
  its own and against Postgres):

  1. the project's override of the agent's kind;
  2. the agent's own setting;
  3. the kind's setting for the workspace;
  4. the kind's default.

  The narrower choice wins at every level. A project is narrower than the workspace, so its override beats everything
  set for the workspace. Within the workspace one agent is narrower than its kind, so an agent's own setting stays the
  exception it was set to be: an admin who later sets the kind does not silently undo it, and the page never shows a
  setting that no longer does anything.
- **Who resolves.** Each caller resolves as its turn starts, so a change applies from the next turn. A coding or Studio
  session's turn and its summary resolve with the session's project; Elysia's dispatcher with none. A step's turn
  resolves through `StepContext::model`, with no project, since every step today reads meetings, conversations, or
  the company doc; it writes "Runs on X at Y effort, from Z" to the step's log, a retry's included. A step that
  works for one project, such as a Project Manager's, carries the project in its input and resolves with it through
  `for_step`. Every caller passes the result to the runner (`TurnRequest::model` and `effort`), so `--model` and
  `--effort` override whatever the agents' configuration repository's settings name.
- **Stored.** `model_settings` holds a row for each agent an admin set, and `kind_model_settings` one for each kind an
  admin set for the workspace (`project_id` null) and each kind a project overrides: a model, and an effort as the
  CLI's `--effort` names it (`low`, `medium`, `high`, `xhigh`, or `max`). A choice equal to what it would follow
  without one removes the row instead (for an agent, its kind's setting or default; for a kind, its default; for a
  project, the workspace's choice for the kind), so it keeps following that should it change.
- **Who may change what.** The workspace's kinds, steps, and agents are admins' alone. A project's overrides are set
  by whoever may edit the project, today any signed-in person, as `PATCH /api/v1/projects/{id}` allows.
- **History and events.** Every change is recorded in the workspace's history in the same transaction (`user_events`:
  `model_setting_changed` with `{ agent, before, after }`, and `kind_model_setting_changed` with
  `{ kind, projectId, before, after }`, `null` for what it followed) and announced by name and id alone:
  `modelSetting.changed { agent }` and `kindModelSetting.changed { kind, projectId }`.
- **Models.** Any id or alias the CLI's `--model` takes, such as `opus[1m]`: a letter or digit, then letters, digits,
  dots, underscores, hyphens, and brackets, so never a flag. The pages suggest the current Claude models (Fable 5.1,
  Opus 5.5, Sonnet 5.5, Haiku 5.5), and the API and the columns' checks both refuse anything else.
- **What it costs.** The pages show each suggested model's price per million tokens (`agent_models::MODEL_PRICES`,
  Anthropic's published first-party prices), so one model's cost reads beside another's; a subscription's usage
  windows fill in proportion to the same tokens. Effort changes how many tokens a turn spends, not their price. They
  also total what each kind's turns spent over the last 30 days (`USAGE_WINDOW`), the cost and tokens Claude Code
  reported, where turns keep them: today only session turns do, in their `finished` events (`session_events`, read
  through the partial index `session_events_finished_at`), so workers' spend is shown, workspace-wide and per project,
  and every other kind says it is not recorded. The Activity log, which records every turn the runner runs
  (`docs/activity.md`), is what lets every kind show its spend.
- **The pages.** Settings, Models (`/settings/models`), admins only, and a project's Models page
  (`/projects/:projectId/models`), for anyone who may edit it (`docs/frontend.md`, Models).

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
