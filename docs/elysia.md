# Elysia

Elysia is Elysium's AI assistant. She is meant to start work on her own: events create work, she decides what is
hers, and she tells people what she is doing and when it will be ready. `docs/vision.md` is the whole picture; this
doc is what is built and what comes next.

## Today

- **Her panel** opens beside every page from the **Elysia** button in the topbar, and stays open across pages and
  reloads. It shows her status, what holds her back, her open questions, the work she has taken on, and either the
  conversation or her activity.
- **The conversation** takes a person's message and queues a turn; her reply joins the conversation when the turn
  ends.
- **Her questions** can be answered with one of her suggestions or in a person's own words, or dismissed. An answer
  queues a turn so she continues the work it was about.
- **The intake** listens to the event stream and queues `event` turns for what can mean work for her (see
  [Triggers](#triggers)).
- **Settings**: admins pause and resume her from the panel, set her hours, and lift a usage-limit hold early.
- **Her turns** run on the host runner (`crate::runner`), one at a time ([Turns](#turns)).
- **The idle loop** brings her back to her own work when nothing is queued ([Idle loop](#idle-loop)).

- **Her tools** let her read the workspace and keep her own record ([Tools](#tools)).

The code is `api/src/elysia/` (status, availability, intake, dispatcher, prompt, runner seam, trigger kinds),
`api/src/models/elysia.rs`, and `api/src/routes/v1/elysia/`. The frontend is `frontend/src/elysia/`.

## Turns

The dispatcher (`api/src/elysia/dispatcher.rs`) looks at the queue whenever something is queued or a hold changes,
and every minute. A turn starts when none is running and nothing holds her back, people first: a queued message or
answer starts before any queued event. Every turn:

- runs as one runner conversation, so the runner never starts two of hers at once, in the workspace's `elysia/`
  directory, where her files persist;
- resumes the Claude Code session the previous turn left (`elysia_state.runner_session_id`), so she carries context;
- appends her standing instructions, `api/src/elysia/instructions.md`, versioned with the code, to Claude Code's
  system prompt;
- gets a prompt naming its trigger and carrying what it needs: every message since her last reply, the question and
  its answer, the batched changes, or the work she is revisiting (`api/src/elysia/prompt.rs`).

Her final text ends with a `Summary:` line. For a message or an answer, the text before it is posted to the
conversation as her reply; for every turn, the summary is stored for her activity log.

When every Claude credential has reached its usage limit, the turn fails and an identical one is queued, and nothing
starts before the limit resets: the time Claude's rate-limit report names, or an hour when it names none. A turn
left running when the API stops is failed at the next start; its trigger is not lost, because the messages or work it
was about are still there for the next turn.

## Tools

Each turn is granted one tool server, `elysia` (`api/src/tools/elysia.rs`), at `/api/agent-tools/elysia` with a
token minted for that turn and revoked when it ends (`docs/coding.md`, Elysium's tools). Coding sessions' servers
refuse her, and hers refuse them.

| Tool                 | Does                                                                          |
|----------------------|-------------------------------------------------------------------------------|
| `elysia_overview`    | Every item in the inbox or open, the active initiatives, the projects, and her own questions and work |
| `elysia_item`        | One item's notes and comments                                                 |
| `elysia_ask`         | Asks a person a question with up to four suggested answers                    |
| `elysia_work_start`  | Records work she takes on, with when she expects it ready                     |
| `elysia_work_update` | Updates that record: where it stands, its status, its estimate                |
| `elysia_propose_changes` | Proposes a changeset of changes to items and initiatives, for a person to approve |

What she writes shows in her panel at once. She reads every live item, initiative, and project, since she acts for
the whole workspace. She changes items and initiatives only by proposing a changeset (`docs/action-items.md`),
proposed as `elysia`, which may name any live item, initiative, or project; what she creates joins the projects she
names. Linking provider issues and pull requests stays a person's to do.

## Idle loop

When nothing has been queued or run for 30 minutes and she is within her hours, she revisits her own work: what
she gave up on or was blocked on and nobody touched for two hours, and what is past her estimate of when it would be
ready and was not revisited in the last two hours. Each piece is recorded as revisited, so it comes back at most
once per cooldown.

## Status

Her color, derived on every read from her turns, questions, and settings (`api/src/elysia/status.rs`):

| Light       | Color  | When                                               |
|-------------|--------|----------------------------------------------------|
| `needs-you` | Pink   | She asked a question nobody has answered           |
| `running`   | Orange | A turn is running                                  |
| `held`      | Grey   | Work is queued and a hold keeps it from starting   |
| `errored`   | Red    | Her last finished turn failed                      |
| `done`      | Green  | None of the above                                  |

When more than one applies, needing a person wins, then running, then held, then a failure. The colors are the `--elysia-*`
tokens in `frontend/src/theme/matter.css`.

Separately, a **hold** says why queued work is not starting, the first of: `paused` (a person paused her),
`usage-limit` (every Claude credential reached its limit; it lifts at the time shown), and `unavailable` (outside
her hours).

Each light change worth hearing plays a short tone, synthesized in the browser (`frontend/src/elysia/sounds.ts`): a
rising one when she needs a person, another when running work finishes, and a falling one when a turn fails. The
first light after a page loads plays nothing.

## Triggers

A turn records what started it in `trigger_kind`, text so later sources add kinds without a migration:

| Kind      | Queued by                         | Waits for her hours |
|-----------|-----------------------------------|---------------------|
| `message` | A person writing to her           | No                  |
| `answer`  | A person answering her question   | No                  |
| `event`   | The intake                        | Yes                 |
| `idle`    | The idle loop                     | Yes                 |

The intake (`api/src/elysia/intake.rs`) keeps, from the event stream:

- every action item and initiative history entry the watcher records (`watcher:<provider>`): what changed in Jira or
  GitHub;
- a person creating an item or commenting on one;
- a decision on a changeset she proposed;
- `timelineRecord.created`: mail, chat, and meetings, once connectors ingest them.

It ignores her own changes, coding agents' changes, and a person's other edits (snoozing, renaming). The first kept
change opens a batch, and 30 seconds later the batch becomes one `event` turn, so a burst costs one turn. A new batch
joins an `event` turn still queued instead of queueing another. A turn carries at most 50 changes and counts the rest.

## Hours

Her hours are weekly windows of UTC minutes (Monday 00:00 UTC is minute 0), because the server holds no time zones.
No windows means any time. Only turns she would start herself wait for them. The panel's **Hours** dialog edits one
daily range on chosen days in the viewer's zone and converts it with the zone's current offset
(`frontend/src/elysia/hours.ts`); a range crossing the end of the UTC week is saved as two windows. A schedule saved
in a local zone therefore shifts by an hour against local time when daylight saving starts or ends, until it is
saved again, which the dialog says.

## Autonomy

What she may do without asking follows the tiers in `docs/vision.md`. Everything starts conservative:

| Kind of action                                                     | Starting tier    |
|--------------------------------------------------------------------|------------------|
| Reading anything in the workspace                                  | Done             |
| Recording her own work and asking questions                        | Done             |
| Drafting (replies, documents), labelling, filing                   | Done and shown   |
| Any change to action items or initiatives                          | Waits for you, as a changeset |
| Anything sent in a person's name (a comment, a closed issue, mail) | Waits for you, as a changeset |
| Deleting anything, payments                                        | Waits for you    |

Which kinds move up, and when, is not decided.

## Tables and routes

`elysia_state`, `elysia_messages`, `elysia_turns`, `elysia_questions`, and `elysia_work` are described in
`docs/database.md`, the routes under `/api/v1/elysia` in `docs/api.md`, and the `elysia.updated`,
`elysiaMessage.created`, and `elysiaTurn.upserted` events in `docs/realtime.md`.

## Roadmap

- Reading changesets and coding sessions, and starting a coding session for work that needs one.
- Her turns' full logs beside their summaries, expanding like a GitHub Actions step.
