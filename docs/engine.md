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
below), the company doc's scheduled review (The company doc, below), every project's Project Manager, which
submits work batches for the worker pool and writes its project's action items and their issues in Jira, YouTrack,
and GitHub (Project Managers, below), and the pool's scheduler, which hands its slots to
those batches in order, with the work queue people and Elysia reorder, pause, and cancel from (Workers and the pool,
below), a worker for each slot, running its batch in a container of its own until it is done (Workers, below), its
pull requests merged by Elysium in the repositories that let it, failing closed (Merging, below), and the
promises people make in conversations, which their projects' Project Managers keep with a follow-up the
person sends (Follow-ups, below).

## A service of its own

- The engine is a separate process, `elysium-engine`, a second binary of the API's crate (`api/src/bin/`), shipped in
  the same image and run as its own compose service, `engine`, in every stack. The crate is a library with two thin
  binaries, so each module is compiled once and checked for dead code across both (`api/src/lib.rs`).
- The API serves clients alone: the web app, its `/api`, and MCP clients, and agents' calls to Elysium's tools
  (`/api/agent-tools`). It starts no loop. The engine does all background work and serves no HTTP beyond its health
  check (`GET /health`): the runner and the workspace it prepares, sessions' turns, Elysia's intake and dispatcher,
  the Project Managers' dispatcher, the worker pool and its workers, the Claude account keeper, the link watcher,
  connector ingest, the conversation loop, the Confluence and YouTrack readers, meetings, every read someone asked
  for, the knowledge base's embedder, backfill, and answerer, the browser reconciler, the mail server brought back up,
  the password vault, update checks, the nightly database backups, and moving identities onto the current schema
  (`api/src/engine/`).
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
| Checks every agent tool call's bearer     | `agent_tool_grants`                      | Grants a turn its tools, renews them while it runs, revokes them when it ends |
| Asks for a backup (Back up now, `/internal/backups`) | `database_backups.claimed_at` unset | Takes it and runs it                         |
| Asks for a connector to be read (Sync now) | `connectors.syncing_since`              | Reads it with its kind's reader, clears it   |
| Adds a browser, or resets one             | `browser_sessions`                       | Brings its container up; records its state   |
| Answers the updater                       | `process_activities`, `update_checks`    | Records what it runs, and what Docker Hub lists |
| Queues Elysia's messages and answers      | `elysia_turns`                           | Runs them                                    |
| Writes to a project's Project Manager     | `project_manager_wakes`                  | Starts its turn, runs it, records it         |
| Runs a waiting batch next, reprioritizes, pauses, resumes, or cancels it; sizes the pool | `work_batches`, `workspace_settings.worker_pool_size` | Hands out the slots that are free, takes back any the pool lost |
| Shows the work queue                      | `worker_pool`                            | Records the CPU cores it sees and any usage hold |
| Answers a worker's report, through its tools | `work_batches.worker_report`          | Takes it as the worker's turn ends, and finishes the batch or waits |
| Records a worker's pull request, through its tools | `work_batch_pull_requests`        | Looks at it as each wait ends and before the batch finishes: merges it where Elysium may, records a merge |
| Answers a Project Manager's answer to a worker's question | `work_batch_events` (`answered`), `work_batches.waiting_until` | Ends the worker's wait, and starts its next turn with the answer |
| Saves a project's worker setup            | `project_repositories`, `projects.tools_setup_script` | Readies the next worker's container with it |
| Retries, skips, or resumes a run's step   | `step_jobs`                              | Claims each due step, runs it, records it    |
| Asks the knowledge base a question        | `knowledge_asks`                         | Takes each queued ask, answers it, ends it   |

- **Taking work** is one engine's, so a turn, a backup, or a sync is taken with a conditional update on its state.
  A run's steps are a queue of their own, claimed with `FOR UPDATE SKIP LOCKED` under a lease, with attempts on the
  row, so they stay correct with any number of claimers (Runs, below).
- **Settling after a restart.** An engine that starts finishes what its predecessor cut short: a turn whose CLI had
  started is finished (its history ends where the turn stopped), one still waiting for a slot is queued again, a backup
  it had taken fails, and every connector still marked syncing is read. A run's step that an engine still held when it
  died is claimed again once its lease lapses. A Project Manager's turn left running is failed and gives what woke it
  back, so its next turn takes it again. A slot of the worker pool an engine held is taken over by the next once its
  lease lapses, or at once when the engine let it go as it stopped; its batch keeps its state, since a worker's
  container outlives the engine, and the engine that takes the slot finds the container and resumes its worker's
  session (Workers, below). Tool grants are not revoked at start, since the sessions queue, Elysia's
  dispatcher, and the Project Managers' dispatcher start together and each grants its first turn's tools at once: every
  grant is leased instead, renewed while its turn runs, so one a stopped engine left lapses within a minute and is
  swept by the next grant minted (`docs/coding.md`, Elysium's tools).
- **The password vault** is the one thing the API asks while a request waits: its CLI and `bw serve` run in the
  engine, on its data volume and its loopback, and the API's routes and agents' password tools ask it over the bus,
  sealed with the encryption key (`docs/passwords.md`, The CLI). An engine that does not answer within 10 seconds
  (3 for its status, 90 for connecting) reads as a vault that is not answering.
- **Docker.** Both reach the filtered `docker-proxy`. The engine creates and replaces everything: the workspace, the
  mail server's reconcile, browsers' containers, backups' dumps, a container per transcription, and each worker's
  container, volume, and network. The API works in
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
  that brings into the index what was stored before it was indexed on write (timeline records, and turn summaries not
  yet on the timeline) is a loop for the same reason (`docs/knowledge.md`, The backfill).

Built: the framework (`api/src/workflows/step.rs` and `workflow.rs`), on the queue below, and four production
workflows: `meeting` (`api/src/meetings/workflow.rs`), whose steps are `fetch_meeting`, `transcribe_meeting`,
`correct_transcript`, and `segment_meeting`; `request` (`api/src/conversations/workflow.rs`), which starts at
segmenting, `segment_meeting` then `handle_request`; `assigned_issue` (`api/src/action_items/assigned/workflow.rs`),
one step, `segment_meeting`, which attributes an issue that arrived assigned to its person to its projects (source
`action_item`, `docs/action-items.md`, Projects and the lead); and `company_doc_review`
(`api/src/company_doc/workflow.rs`), one step, `review_company_doc`.
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
  request workflow starts at this step (Conversations, below); a conversation's topics attribute its messages. It
  takes an action item too, which it puts on its projects without topics, and the assigned issue workflow is that alone
  (`docs/action-items.md`, Projects and the lead). See `docs/meetings.md`, Segmentation.

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

Built: every project's Project Manager (`api/src/project_managers/`), its wakes, turns, and work batches
(`api/src/models/project_manager.rs` and `work_batch.rs`), its tools, its writes to its project's action items and
trackers (Its writes, below), and its tabs on the project's page.

- **One session per project.** A project's Project Manager is one Claude Code session, resumed for every turn
  (`claude -p --resume`) and kept indefinitely, relying on Claude Code's own compaction as it grows. Each project is
  its own runner conversation, working in `project-managers/<project id>` in the workspace, so its files persist. The
  session it last reported is kept on its row (`project_managers.runner_session_id`), which its first wake creates.
- **What wakes it.** Five things, each written as a wake (`project_manager_wakes`) in the same transaction as what
  caused it, so no crash or lost signal loses one:
  - **a record newly on its project**: segmenting a meeting or a conversation attributing a topic or a message to it,
    a person adding a record to it, or an undo putting one back, all through `models::attribution`
    (`docs/connectors.md`, The timeline). A change of how sure an attribution is wakes nothing, and a record waits once
    however often it is attributed meanwhile. Any workflow that attributes records wakes Project Managers this way,
    the request workflow included (Conversations, above), with nothing of its own to add. What Elysium's agents put on
    the timeline wakes nothing: its own records, and a session's turn summaries (`docs/coding.md`, Turn summaries). A
    wake per turn would spend a Project Manager's turn on every prompt anyone types into a session of the project; the
    work it dispatched reports back through the batch's result below, and it reads the rest on its timeline when
    something else wakes it. A worker's merged pull request is the one exception: a merge is when the work reaches the
    project, which the Project Manager acts on (resolving the items it was for, planning what follows) before the
    batch may end, and it happens once per pull request (Merging, below);
  - **a batch's result**: a worker finishing its batch (`work_batch::finish`, which the pool's workers call), or its
    worker asking a question it waits on (Workers, below);
  - **a person**: the Project Manager tab's composer (`POST /api/v1/projects/{id}/project-manager/messages`);
  - **Elysia**: her `elysia_message_project_manager` tool (`docs/elysia.md`, Tools);
  - **a promise**: one segmenting found on its project, or one whose work just finished (Follow-ups, below); a promise
    waits once however often it is woken for meanwhile.

  Linking a page to its project wakes nothing, since nothing has to happen because of a link and a wake spends a turn:
  every turn's prompt names the project's docs, so the next turn finds it, and a person who wants it read says so.
- **Debounced and coalesced.** The dispatcher (`project_managers::dispatcher`) starts a project's turn once its wakes
  have been quiet for 30 seconds, or two minutes after the oldest arrived however busy it stays, and at once for a
  message, someone being there for the answer (`project_managers::queue`, a pure function tested alone). A turn takes
  every wake waiting for its project, up to 40 records and results and every message, so a meeting landing ten
  segments is one turn. What arrives while it runs waits for the next.
- **One at a time, durable.** A turn is created as it starts, in the transaction that takes its wakes, and the
  database refuses a project a second running turn (`project_manager_turns_one_running`). At most two Project Manager
  turns run at once across projects, since they share the runner's eight slots with sessions and Elysia. A turn the
  engine stopped in the middle of is failed at the next start and gives its wakes back; so does one refused because
  every credential is spent, which also holds its project until the limit resets (`usage_paused_until`). The next
  turn is told some of what woke it went to a turn that did not finish, so it checks before doing anything twice. A
  turn that failed of itself keeps its wakes, since trying again at once would most likely fail the same way.
- **A turn.** Its model is its kind's for the project (`agent_models::for_agent(Agent::ProjectManager, project)`,
  Models below). Its standing instructions are `api/src/project_managers/instructions.md`, appended to Claude Code's
  system prompt, and its prompt names the project, says whether its changes apply at once or wait for a person and
  where its issues file (or why nowhere), names the project's docs, the knowledge base pages linked to it, up to 50 of
  them newest published first, and carries what woke it: each message, each record's kind, title,
  summary, attribution, and an excerpt, each batch's result with the promises still waiting on one that failed, and
  each promise that woke it, under 100 KB. Every turn also carries each of the project's promises whose work is done
  and whose follow-up is not drafted yet, up to 20, woken for or not, so a turn that failed of itself never strands
  one. It ends with a `Summary:` line, which
  heads it; the text before it is its reply.
- **Its tools.** `elysium_project_manager` (`api/src/tools/project_manager.rs`): `manager_timeline` and
  `manager_record` read its project's timeline, every owner's records on it, `manager_pages` and `manager_page` list and
  read the pages linked to its project, a long one in parts, and refuse any other, `manager_batches` lists its batches,
  `manager_submit_batch` submits one, `manager_cancel_batch` withdraws one no worker has started on,
  `manager_answer_batch` answers the question a running batch's worker waits on, and
  `manager_promises`, `manager_tie_promise`, and `manager_draft_follow_up` keep its promises (Follow-ups, below), and
  `manager_make_changes` changes its project's action items and files their issues (Its writes, below). It also gets
  `elysium_work`, scoped to its project, to read items and initiatives, and `elysium_knowledge`, whose pages it edits
  as its machine user, naming its project (`docs/knowledge.md`, Who changes pages). Every call is checked
  against the project in the database, under a grant for the turn alone. `work_comment` and `work_propose_changes`
  refuse it, pointing at `manager_make_changes`, so each of its changes names what caused it.
- **On the timeline.** Each turn that did its work is a timeline record of kind `project_manager`, headed by its
  summary and holding its reply, and so is each batch it submits and each change it makes, attributed to its project
  as certain. They are the
  workspace's, owned by the Project Managers' machine user, so every member reads them on the project's timeline
  (`docs/connectors.md`, The timeline); nothing wakes on them, and Elysia's intake leaves them out.
- **Its turns' logs.** Each turn keeps what it said and which tools it called, one line each, the newest 64 KiB, which
  the page reads when a turn is opened. Its full stream is Claude Code's own transcript until Activity records every
  turn (`docs/activity.md`).
- **Who sees it.** Its turns quote what was ingested, so its routes answer the company's members alone, as the
  knowledge base does.
- The limits (debounce, longest wait, concurrency, wakes per turn, prompt budget) are constants in
  `api/src/project_managers/`.

### Its writes

A Project Manager creates, updates, resolves, dismisses, and comments on its project's action items, moves them between
initiatives, creates initiatives, and files an item's own issue in the project's tracker, all with one tool,
`manager_make_changes` (`api/src/tools/work/manager_changes.rs`). It speaks Elysium's terms, never a tracker's: an
item's issue follows the item, so a comment on the item is posted to its issue and resolving the item moves the issue
to done, through the same outbox as a person's own writes (`docs/action-items.md`, Links and the watcher).

- **One path.** Every call is one changeset proposed as `project_manager:<project id>` (`docs/action-items.md`,
  Changesets), so a person reviews, applies, and undoes a Project Manager's work as they do anyone's.
- **The switch.** Each project chooses whether its Project Manager writes directly (`projects.manager_writes_directly`,
  the Project Manager tab's "Write to trackers without review"). Off, the default, every changeset waits for a person's
  approval and nothing is written in Elysium or any tracker until then: a new project starts with its Project Manager
  proposing, the safer way, and a person turns writing on once its proposals have earned it. On, the changeset is
  approved and applied at once by the Project Managers' machine user (`decided_by`); an item it creates then starts in
  the inbox, since nobody accepted it, where a person's approval would have made it open.
- **Filing an issue.** `file-issue` gives an item with no linked issue its own: the watcher opens it with the item's
  title (cut to 255 characters, Jira's limit) and notes, and links it as the item's primary. The filing is owed in
  the transaction that applies the operation and landed by the watcher (`action_item_issue_filings`,
  `api/src/models/action_item_issue_filing.rs`); one that fails keeps the provider's answer on the item, is retried each
  pass, and can be cancelled from the item's page. An item resolved, dismissed, or deleted before it is filed drops
  its filing.
- **Never two issues.** A retried turn proposes what it proposed before, so a create from the same record with the same
  title as one an earlier changeset of the project's Project Manager still holds, pending or applied, is refused
  naming that changeset and item. A filing records each attempt before it calls the tracker, and an attempt after the
  first looks for the issue its predecessor may have opened (one its account opened there since, with exactly its
  title) before it opens another, so a crash between the tracker answering and Elysium recording it never leaves two.
- **Its cause.** Every change names the timeline record it came from (`causeRecordId`, `changeset_operations.cause_record_id`),
  which must be on the project's timeline: a meeting's segment, a message, a mail. Each change that applies is a
  timeline record of kind `project_manager` (`metadata.kind` `write`), titled with what it did ("Filed in Jira ELY:
  Annual pricing"), holding the reason and the quote, and naming its changeset, operation, item, and cause; the record of
  a filing links to the issue once it is filed. Proposals that wait record nothing until a person applies them.
- **The tracker.** Issues file in the tracker the project names (`project_issue_trackers`: a provider, the place, and
  for Jira and YouTrack the credential), chosen on the Project Manager tab from the places its credential reaches
  (`GET /api/v1/projects/{id}/project-manager/destinations`) and checked through that credential before it is stored.
  A project that names none follows its precedent: the place most of its items' linked issues are, the newest breaking
  a tie, through the credential they were linked with (`api/src/project_managers/trackers.rs`). A Jira place is a
  project key inside the credential's allowlist, a YouTrack place a project's short name reached by a person's own
  YouTrack account, and a GitHub place a repository.
- **Its credential.** Jira and YouTrack file through the tracker's own credential. GitHub files through the token the
  project's coding sessions get, its own or the workspace default (`docs/github.md`), so a project acts on GitHub as one
  account; a project whose sessions get no token cannot file on GitHub. A project with nowhere to file, or a GitHub
  tracker with no token, refuses `file-issue` saying so and how to fix it, and the prompt tells the Project Manager
  before it tries.

### Work batches

A batch (`work_batches`) is one piece of work for one worker: a title, a goal written so a worker who has seen nothing
else can finish it, the repositories it needs (`[{ url, baseBranch? }]`, at most 16, each held to a coding session's
rules), a priority (`asap`, `normal`, or `scheduled` with `scheduled_for`, in the future and within a year), and its
provenance: the turn that submitted it, the timeline records that called for it, each checked to be on its project, and
the action items it works on, each checked to be led by its project. An item can sit on several projects, but only its
lead project's Project Manager works it (`docs/action-items.md`, Projects and the lead), so a batch naming an item
another project leads is refused, telling the Project Manager which project leads it and that a person can move the
lead. Submitting stores it, puts it on the timeline, and announces both in one place (`project_managers::batches`).

Where a batch stands (`api/src/models/work_batch.rs`):

| State       | Means                                                             | Moves on by                        |
|-------------|-------------------------------------------------------------------|------------------------------------|
| `queued`    | Waits for a slot                                                  | The scheduler's `assign`; a pause; a cancel |
| `paused`    | Held back by a person, Elysia, or its Project Manager             | A resume, back to `queued`; a cancel |
| `assigned`  | Holds a slot; no worker has started on it                         | A worker's `start`; a pause or cancel, which gives the slot back; `unassign` when the pool shrank |
| `running`   | A worker has it                                                   | Its worker's `finish`              |
| `succeeded`, `failed` | Its worker ended it, saying what it did                 | Nothing                            |
| `cancelled` | Withdrawn before a worker started, with why                       | Nothing                            |

`work_batch::finish` wakes its Project Manager in the same transaction, as does a cancel by anyone but the Project
Manager itself, so it learns its work was withdrawn and why; the caller then signals `workerPool` and `projectManagers`
and publishes `workBatch.changed` and `projectManager.changed`.

- **History.** Every change is written to the batch's history (`work_batch_events`) in the transaction that makes it,
  with who made it: `submitted` by its Project Manager; `pinned` and `unpinned` (run next, and back to its place),
  `reprioritized` (`{ before, after }`, each `{ priority, scheduledFor }`), `paused` (`{ from }`), `resumed`, and
  `cancelled` (`{ reason }`) by a person, Elysia, or its Project Manager; `assigned` and `unassigned` by the pool, as the
  system user; `started`, `waiting` (`{ on, detail, until }`), and `finished` (`{ succeeded }`) by its worker, as the
  coding agent; and `answered` (`{ answer }`) by its Project Manager. A change that moves nothing writes nothing.
- **One way in.** The work queue's routes, Elysia's tools, and a Project Manager's tools all change a batch through
  `worker_pool::actions`, so each change is checked, recorded, announced, and signalled alike.

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

Built: the scheduler, its order, its leases, the usage gate, the admin's size, and the work queue people and Elysia
change it from (`api/src/worker_pool/`), and the workers, one per slot, each in a container of its own
(`api/src/worker_pool/worker/`, Workers, below).

### The order

`worker_pool::order` is a pure function of the queued batches, how many slots each project holds, and the time, tested
alone. The order is derived again on every look, so an engine that restarts hands out slots exactly as its predecessor
would have.

1. **Run next.** A batch someone asked to run next goes before everything else, the earliest asked first. Asking is
   how a person or Elysia reorders the queue: any order is reached by asking in turn, and a priority change moves a
   batch between the tiers. Only a batch that is due may run next; one scheduled for later is made ASAP instead.
2. **Tiers.** Then due scheduled batches, then ASAP, then normal. A scheduled batch is never handed a slot before its
   time.
3. **Round robin.** Within a tier the next slot goes to the project holding the fewest slots, counting those it holds
   and those it is handed ahead in this order, so a project that submits twenty batches never starves one that submits
   one. Each project's batches go oldest first (a scheduled one by when it came due), and a tie goes to the project whose
   next batch has waited longest.

### Slots and their leases

- **The hand-off.** A slot handed out is a batch moved from `queued` to `assigned`, claimed under a token of its own and
  a 60 second lease (`work_batch::assign`). The engine's pool holds it and renews its lease on every look, every 15
  seconds. A worker takes it by starting the batch (`work_batch::start`, `assigned` to `running`) and gives it
  back by finishing it (`work_batch::finish`). Every write a holder makes is fenced by its token, so an engine whose
  lease lapsed records nothing over the one that took the slot over.
- **Giving a slot back.** Finishing, failing, pausing or cancelling a batch no worker started on, and taking back the
  newest unstarted slots when an admin shrinks the pool (`work_batch::unassign`). A running batch keeps its slot until
  its worker ends it, so a smaller pool shrinks as slots come back.
- **Restarts.** An engine that stops lets its leases lapse at once (`work_batch::release`); one that dies stops
  renewing them. The next engine takes every lapsed slot over under a new token (`work_batch::adopt`), keeping the
  batch's state: unlike a step, a slot taken over is never started again from the beginning, since a running batch's
  worker lives in a container that outlives the engine, and the engine that adopts it finds that worker again.
- **Any number of claimers.** Each pass that hands out slots holds one transaction-scoped advisory lock, so two never
  count the same free slot, and skips a batch someone is changing rather than waiting on them.

### Each look

The scheduler (`worker_pool::scheduler`) looks when it is signalled (`workerPool`: a batch submitted, resumed, or
reprioritized, a slot given back, the size changed), when the next scheduled batch comes due, when a usage hold ends,
and every 15 seconds. It renews and adopts leases, records the CPU cores it sees and any usage hold on the `worker_pool`
row, which the queue's page reads, takes back slots the pool lost, then, unless a usage limit holds the pool, hands each
free slot to the next batch in order.

- **Size.** `workspace_settings.worker_pool_size`, or by default one worker per CPU core the engine sees
  (`available_parallelism`, which follows the container's CPU quota) minus two, at least one (`worker_pool::size`).
- **The usage gate.** No slot is handed out while every Claude credential that could serve a turn is an account at its
  usage limit (`runner::credentials::usage_hold`, the runner's own rule, read without decrypting anything). An API key or
  a subscription token still serves, and with no credential at all a turn runs on the workspace's login, so neither
  holds the pool. The scheduler looks again at the earliest reset.

### The work queue

`GET /api/v1/work-queue` (`worker_pool::queue`) answers the pool (its size, the size an admin chose, the default, the
CPU cores, the slots in use, and a usage hold's end), the batches in its slots, every batch it may hand a slot now in
order, those scheduled for later, and those paused. People change a batch with `PATCH /api/v1/work-batches/{id}` and
`POST /api/v1/work-batches/{id}/cancel`; admins size the pool from Settings, Worker pool. The page is `/work-queue`
(`docs/frontend.md`, Work queue). A batch's goal and title quote what was ingested, so the queue and its batches
answer the company's members alone.

### Workers

Built: each slot's worker (`api/src/worker_pool/worker/`), the project's worker setup with its Workers page, and the
worker's own tools.

- **A container of its own.** A worker is Claude Code in `elysium-worker-<batch id>`, from the workspace image of the
  release, on a volume and a bridge network of the same name. One slot is one worker. Its network is joined by the API
  alone, as `api`, so its agent calls Elysium's tools at `http://api:8080` as the shared workspace's do. Its turns are
  `docker exec` as `agent`, each with a desktop and a Blender of its own, as a session's are (`docs/coding.md`).
- **Its setup.** A project declares its repositories, each with an optional setup script, and one tools script
  (`project_repositories`, `projects.tools_setup_script`, the project's Workers page, `docs/frontend.md`). A worker
  starting runs the tools script as root, then clones each repository its batch names into `repos/<name>` with
  `--recurse-submodules`, through `gh` with the project's GitHub token, and runs that repository's setup script as the
  agent in its directory (`api/src/worker_pool/worker/setup.rs`). The tools script installs tools system-wide: the
  batch's directory is the agent's, and what root writes there would be root's. A repository the batch names that the
  project does not declare is cloned with no setup script, and the batch's base branch wins over the project's. A
  submodule that does not clone, one the token cannot read or that is gone, stops setup, and the batch fails with a
  result naming the submodule, its URL, and what git said. Setup runs once per container: a marker on its volume says
  it finished. Workers may clone more repositories themselves.
- **`~/.claude`** is fetched from the configuration repository into the worker's own volume as its container comes up,
  as the shared workspace's is (`docs/coding.md`, `~/.claude`), which is where `/steamroll-issues` and the agents'
  settings come from.
- **Its turns** run on a runner of its own, so a worker never waits for the shared workspace's eight turn slots; the
  pool's size bounds workers. Every turn's prompt is `/goal`, holding out until the worker has reported on its batch
  with `worker_finish` or `worker_wait` as the last thing it did, a condition it can always make true itself. Its
  standing instructions (`api/src/worker_pool/worker/instructions.md`) have it work the batch through with
  `/steamroll-issues`, open pull requests, record each one, address every review comment, never merge one itself, and
  end every turn with one report. The batch (its goal, repositories with who merges in each, action items, and where
  each of its pull requests stands) and why this turn starts (the first, after a wait with any answers given, after an
  interruption, after a usage limit, or after a turn that did not report) are appended to its system prompt, which a
  resumed session reads afresh (`api/src/worker_pool/worker/brief.rs`). Each turn resumes the worker's Claude Code
  session (`work_batches.worker_session_id`), on what the `batch_worker` agent resolves to for the batch's project
  (Models, below).
- **Its tools.** `elysium_worker` (`api/src/tools/worker.rs`): `worker_batch` reads the batch, its pull requests, and
  the answers given since it last began to wait, `worker_pull_request` records a pull request it opened (Merging,
  below), `worker_finish` reports the batch done or impossible with its result, and `worker_wait`
  reports what it waits on (`review`, `ci`, or `question`), what for, and when to look again, 5 minutes to a day (by
  default 15 minutes for CI, an hour for a review, four hours for a question). The API answers them and holds no claim
  on the slot, so each only files its report on the batch (`worker_report`), the last of a turn winning; the engine
  whose worker ran the turn takes it as the turn ends and acts on it under the slot's token
  (`api/src/models/work_batch/worker.rs`). A worker also gets `elysium_knowledge`, which it reads as the coding agent
  and which refuses it proposals to the company doc and changes to pages, and `elysium_passwords` while the vault is up.
  Not `elysium_work`,
  whose writes are recorded as a session's or a Project Manager's, and not `elysium_storage`, whose tools move files
  in the shared workspace.
- **Finishing.** `worker_finish` finishes the batch (`work_batch::finish`, as the coding agent), which gives its slot
  back and wakes its Project Manager in the same transaction; then the engine signals `workerPool` and
  `projectManagers`, publishes `workBatch.changed` and `projectManager.changed`, and removes the container, its volume,
  and its network. A batch that cannot be set up (its container did not come up after three tries 30 seconds apart, a
  script or a clone failed, or its repositories would share a directory) fails the same way, saying why.
- **Waiting.** `worker_wait` keeps the container and the slot: the batch stays `running`, with `waiting_on`,
  `waiting_detail`, and `waiting_until` set and a `waiting` entry in its history, and the worker starts its next turn
  when it is time to look again. A question also wakes the batch's Project Manager, whose turn shows the question under
  the batch; `manager_answer_batch` answers it, recorded as `answered`, which ends the wait at once (the waiting worker
  hears `workerPool`), and the next turn quotes the answer. A person answers through the Project Manager's composer. A
  worker waiting holds no activity, so auto-update may restart the stack meanwhile.
- **Strikes.** A turn that ends without a report, fails, or goes quiet past the runner's heartbeat is run again, told
  why, up to three turns in a row (`MAX_STRIKES`) before the batch fails. A turn every credential's usage stopped waits
  for the reset, or 15 minutes when none is known, and carries on, keeping its slot.
- **Restarts.** An engine that stops interrupts its workers' turns and leaves their containers. The engine that takes a
  slot over finds the container, stops any turn an earlier engine left in it, and acts on a report filed before the
  stop; failing that it waits out a wait that was under way, or starts a turn that resumes the session where it was cut
  short. A worker's running turn holds an `agentTurn` activity, so auto-update waits for it.
- **Each look.** After each of the scheduler's looks its workers look too: a worker for every slot it holds, a stop
  for the worker of a slot it lost (paused or cancelled before the worker started, or taken over), the API joined to
  each worker's network again (compose drops the join when it recreates the API), each container's CPU limit kept at
  the pool's share, and whatever this deployment's workers left in Docker removed for a batch no worker runs any more.
  A batch another engine may still hold, `assigned` or `running`, is left alone.
- **CPU.** The pool's default leaves the host's cores minus two for workers, at least one, and those cores are shared
  evenly across the pool's size: each worker's container is limited to its share (`worker_pool::worker_nano_cpus`),
  one core each at the default size, more in a smaller pool, less in a larger one, so every slot busy at once never
  takes more than those cores however an admin sizes the pool. A worker also has half Docker's default CPU weight, so
  the API, the engine, and Postgres win under contention, and at most 8,192 processes. Memory is not limited: what a
  build needs varies too much for one cap that never kills a good build, so a host is sized for its pool.
- **What it reaches.** The internet, through its own bridge (GitHub, package registries, the model provider, and
  whatever else its setup and work fetch), and of Elysium only the API, whose agent tools answer it under its turn's
  grant. Nothing else: not Postgres, Redis, Kratos, Hydra, `docker-proxy`, the embedding service, the shared workspace,
  or another worker (`docs/security.md`, Agents).
- **Secrets.** The project's GitHub token reaches the worker as `GH_TOKEN`, with git routed through `gh`, as a session's
  does (`docs/github.md`), with every workspace environment variable. Every turn's events, setup's output, and the log
  are scrubbed of those secrets, and of every password the vault handed out, before anything of them is stored or
  shown.
- **What it records.** Its log (`work_batches.worker_log`): what it said and which tools it called, one line each, the
  newest 64 KiB, stored every 30 seconds while a turn runs and read on the batch's page. The full stream is Claude
  Code's own transcript on the worker's volume, removed with it, until Activity records every turn
  (`docs/activity.md`).
- The limits (strikes, tries, timeouts, the log's size, the wait bounds) are constants in
  `api/src/worker_pool/worker/` and `api/src/tools/worker.rs`.

### Merging

Built: who merges each repository's worker pull requests, the pull requests workers record, the look that merges them
failing closed, and the record of every merge (`api/src/worker_pool/merging/`, `api/src/github/pulls.rs`).

- **Who merges.** Each repository a project declares says who merges the pull requests its workers open
  (`project_repositories.merge_policy`, the Workers page's "Merge when ready"): a person, the default, since merging is
  the step a team most often keeps for itself, or Elysium (`auto`), on github.com alone. A repository the project does
  not declare is a person's. The choice is read again at every look and every turn, so a change applies at once.
- **Only its own.** A worker records each pull request it opens with `worker_pull_request`, which checks with GitHub,
  through the project's token, that the project's account opened it, from a branch of the repository itself, after the
  batch started (`work_batch_pull_requests`). A pull request is one batch's: another batch naming it is refused.
  Nothing a worker did not record is ever merged, and the worker never merges one itself.
- **When it looks.** The engine looks at a batch's open pull requests as each of its worker's waits ends, before the
  turn that follows, and once more before the batch finishes, so the look follows the worker's own loop: a worker
  waiting on CI or a review keeps its container, nothing new polls GitHub, and each pull request is read once per wait.
- **What it reads.** One GraphQL call per pull request (its state, author, fork, draft, mergeability, GitHub's
  `mergeStateStatus`, review decision, latest reviews, review threads, and its head commit's checks, with the base
  branch's classic protection as a non-admin reads it), then, where Elysium may merge, the base branch's rulesets
  (`GET /rules/branches/{branch}`, `docs/github.md`, Merging).
- **Fail closed** (`merging::decide`, a pure function tested alone). It merges only when the project's account opened
  it from the repository itself, it is no draft, GitHub says it merges cleanly and that nothing the branch requires is
  missing (`CLEAN`), its head commit has checks and every one passed, every check the branch's rules require is among
  them, no reviewer's latest review asks for changes and no required review is owed, every review thread is resolved,
  the branch does not merge through a merge queue, and the repository and its rules leave a way to merge. Anything
  GitHub did not answer, answered in a shape this build does not know, or answered only in part (more than a hundred
  threads, reviews, or checks) holds it, and so does a GitHub that cannot be read or a project with no token. A pull
  request with no checks at all is never merged: "CI is green" means CI ran.
- **How it merges.** By the repository's own rules: the first of squash, merge commit, and rebase that the repository
  allows, every ruleset's allowed methods allow, and a required linear history does not rule out. The merge names the
  head commit the look read, so a push since makes GitHub refuse it rather than merge what was not read. GitHub
  enforces the branch's protection on the call itself, and nothing asks it to bypass any.
- **Settling.** GitHub works out whether a pull request merges cleanly in the background after a push, so a read
  that finds it unknown reads it once more three seconds later before holding it.
- **A failed batch.** The look before a batch finishes merges only when its worker reported it succeeded; a worker
  that says the batch cannot be done leaves its pull requests for a person, and only what already merged is recorded.
- **What it says.** A pull request held keeps every reason (`held_because`), as one sentence the worker's next turn
  reads in its brief and the batch's page shows, so the worker fixes them all at once.
- **Merged work on the timeline.** A merge, Elysium's or a person's on GitHub, is recorded, put on the project's
  timeline as a `merged_pull_request` record (the coding agent's, attributed as certain by `worker:<batch>`, linking to
  the pull request), and wakes the project's Project Manager, all in one transaction that moves the pull request from
  `open` alone, so it is recorded once however often it is read. An engine that stops between GitHub merging and that
  record finds it merged on its next look and records it then, naming whoever GitHub says merged it. Elysia's intake
  leaves it out, as it does every agent's record.
- **Promises.** A promise tied to the batch becomes due when the batch succeeds, which follows its merges; one tied to
  an action item, when the item resolves, which the Project Manager the merge woke decides (Follow-ups, below).

On the roadmap:

- Merging through a merge queue (GraphQL `enqueuePullRequest`, naming the head it read); today a branch that merges
  through one holds the pull request for a person.
- An egress allowlist for workers' networks: GitHub, the package registries, and the model provider alone.
- Waking a worker that waits on a review or CI when the link watcher sees it change, rather than at its next look,
  which would also merge a ready pull request sooner.
- A pull request still open when its batch ends is not looked at again, so a person merging it later puts nothing on
  the timeline; the link watcher reading it is the likely way.

## Models

Every agent runs on a model and effort people may choose, through the same credential failover stack and
usage-aware skipping as every turn (`docs/coding.md`, Credentials). Built: the kinds of agent, the settings at each
level, their resolver, the admin page, and each project's own page (`api/src/agent_models.rs`).

- **What has a setting.** An agent is anything whose work is a turn on the runner, named as it is in code: a workflow
  step whose work is an agent turn, by its step name, and the agents outside the step framework: Elysia
  (`elysia`), every project's Project Manager (`project_manager`), coding sessions (`coding_session`), Studio sessions
  (`studio_session`), the pool's workers (`batch_worker`), turn summaries (`turn_summary`), and the knowledge base's
  answers to questions (`knowledge_ask`).
- **Kinds.** Every agent belongs to one kind (`agent_models::AgentKind`, stored as the enum `agent_kind`), the one
  typed list of what sort of work a turn is:

  | Kind              | Its agents today                                   | Runs for one project |
  |-------------------|----------------------------------------------------|----------------------|
  | `worker`          | Coding sessions, Studio sessions, the pool's workers (`batch_worker`) | Yes |
  | `project_manager` | Every project's Project Manager (`project_manager`) | Yes                 |
  | `correction`      | Transcript correction (`correct_transcript`)       | No                   |
  | `segmenting`      | Segmenting meetings and conversations, and attributing arrived items (`segment_meeting`) | No |
  | `triage`          | Handling a conversation's messages (`handle_request`), and answers asked of the knowledge base (`knowledge_ask`) | No |
  | `elysia`          | Elysia, and the company doc's review, done in her name | No               |
  | `summaries`       | Turn summaries                                     | Yes                  |

  An answer from the knowledge base is triage's: like deciding what a message asks for, it reads what was said across
  the workspace for no one project, and a person acts on what it says, so it runs on Opus 5.5 at high effort, which
  cites faithfully, unless an admin picks a faster model for it on its own row. A worker is a coding or Studio session
  in the shared workspace, or one of the pool's workers in a container of its own, so they share the `worker` kind
  rather than each having one. Correction, segmenting, and triage read
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
- **Who resolves.** Each caller resolves as its turn starts, so a change applies from the next turn. A coding or
  Studio session's turn and its summary resolve with the session's project, a Project Manager's turn and a pool
  worker's with their project, and Elysia's dispatcher with none. A step's turn resolves through `StepContext::model`,
  with no project, since every step today reads meetings, conversations, or the company doc; it writes "Runs on X at Y
  effort, from Z" to the step's log, a retry's included. A step that works for one project carries the project in its
  input and resolves with it through `for_step`. Every caller passes the result to the runner (`TurnRequest::model`
  and `effort`), so `--model` and `--effort` override whatever the agents' configuration repository's settings name.
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

Built: promises found in conversations, kept by their projects' Project Managers (`api/src/models/promise.rs`,
`api/src/project_managers/promises.rs`), with the Promises tab on a project's page and the follow-up on its item's page
(`docs/frontend.md`, Projects and Action items).

A promise is the person telling someone they will let them know once some work is done ("I'll ping you here when the
fix is live"). It is recorded on the project whose work keeps it, with who it is for, what was promised, the message it
was made in (which is where they were asked, and where its follow-up goes), and the work it waits on. When that work is
done, the project's Project Manager drafts the follow-up in the person's voice, and it waits in their inbox as an action
item with Send, Edit, and Dismiss. Nothing is sent without Send.

- **Found by segmenting.** A conversation's segmenting turn (`segment_meeting`, `docs/meetings.md`, Segmentation) names
  the promises the person made in it beside its topics. Segmenting is where they are found because it already reads
  every message of the conversation whole, numbered, in one text turn whose answer nothing trusts: a promise must name
  one of the messages shown, once each, someone it is for, what was promised, and a project that was offered, at most
  ten an answer. It reads every conversation whether or not any Project Manager is woken, and a promise needs the exact
  message it was made in, since that is what its follow-up answers. A Project Manager's turn reads records only as
  excerpts and answers through tools, so it ties and drafts but never finds.
- **Where it can go.** A promise is kept only when its message is answered through the connector it came from
  (`Definition.replies`, Slack and Teams today), since its follow-up is a reply to that message. The person's own mail
  is never read, so a promise made by mail is never found there.
- **Once per message.** A message holds at most one promise (`promises.timeline_record_id` is unique), so a
  conversation read again as it grows records nothing twice, and a promise a person dismissed stays dismissed. Each
  new one is recorded as Elysia's, in the transaction that attributes the conversation's messages, with a `promise`
  wake for its Project Manager.
- **Tied to its work.** The Project Manager ties a promise to the work that keeps it (`manager_tie_promise`): a batch of
  its project's, kept when it succeeds, or an action item in its project, kept when it resolves, which is how a linked
  Jira or GitHub issue closing and a pull request merging reach it, through the link watcher. A batch that failed or
  was cancelled, and an item dismissed, deleted, or in another project, are refused. Work already done makes the
  promise due as it is tied.
- **Due when the work is done.** A batch succeeding (`work_batch::finish`) or an item resolving, by anyone
  (`action_item::transition`), makes every promise waiting on it `due` and wakes its Project Manager, in the same
  transaction, so nothing polls for it. A batch that fails leaves its promises waiting, and the Project Manager's next
  turn names them under the failed batch so it ties them to what replaces it. Deployments have no signal of their own
  yet: a batch whose goal includes the deploy, or an item resolved once it is out, is how one counts.
- **Drafted in the person's voice.** For a due promise the Project Manager writes the message as the person who made
  it (`manager_draft_follow_up`). In one transaction it becomes a draft of that person's answering the message the
  promise was made in, through what answers its kind (`drafts::reply_draft`, its kind's first place, the thread), and
  an inbox item of theirs in the project, titled `Follow up with <recipient>: <promise>` and quoting what they said,
  created as `project_manager:<project id>`; the promise is then `drafted`. Raising an inbox item changes nothing in
  the person's name, as `handle_request`'s items do not, so it needs no changeset.
- **The person decides.** Send is `crate::drafts::send`: it posts through the connector the promise came from, as the
  person, and in the transaction that records the send it marks the promise `kept` and resolves its item
  (`docs/connectors.md`, Sending). Edit changes the draft's text. Dismiss dismisses the item, which marks the promise
  `dismissed`. Resolving the item by hand, having told them some other way, keeps the promise too; reopening an item
  changes no promise until it is resolved or its follow-up is sent.
- **Who sees it.** A promise quotes what was ingested, so its routes answer the company's members alone, as the Project
  Manager's do; its follow-up's draft is its person's alone, and anyone else sees that it is theirs to send.

States: `waiting` (recorded, its work not done or not tied yet), `due` (its work done, its follow-up owed), `drafted`,
`kept`, and `dismissed`. A promise leaves `waiting` only when its work finishes.

On the roadmap:

- The person's own messages join a conversation only while it is being worked on (Conversations, Gathering), so a
  promise typed after its conversation's run ended is not read. Reading the person's replies whenever they answer a
  conversation Elysium already holds is the next step.
- Promises made in meetings ("I'll tell them on Slack"): a meeting segment is not answered from Elysium, so its
  follow-up needs a new message to a conversation picked rather than answered (`docs/connectors.md`, Roadmap).
- Dismissing a promise from the project's Promises tab before its work is done.

## Elysia

- Elysia does not orchestrate the engine. Asked, she can see and change anything in it: inspect runs, pause, cancel,
  or reorder work, and talk to a project's Project Manager. Built: talking to one (`elysia_message_project_manager`),
  and the work queue: reading it (`elysia_work_queue`), running a batch next, reprioritizing, pausing, or resuming it
  (`elysia_change_batch`), and cancelling one (`elysia_cancel_batch`), each recorded in the batch's history as hers.
- Questions for the person, from any step, become action items.
