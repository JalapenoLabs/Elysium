# Elysia

Elysia is Elysium's AI assistant. She is meant to start work on her own: events create work, she decides what is
hers, and she tells people what she is doing and when it will be ready. `docs/vision.md` is the whole picture; this
doc is what is built and what comes next.

## Today

- **Her panel** opens beside every page from the **Elysia** button in the topbar, and stays open across pages and
  reloads. It shows her status, what holds her back, her open questions, the work she has taken on, and either the
  conversation or her activity.
- **The conversation** takes a person's message and queues a turn for her to answer it.
- **Her questions** can be answered with one of her suggestions or in a person's own words, or dismissed. An answer
  queues a turn so she continues the work it was about.
- **The intake** listens to the event stream and queues `event` turns for what can mean work for her (see
  [Triggers](#triggers)).
- **Settings**: admins pause and resume her from the panel, and lift a usage-limit hold early.

Nothing runs her turns yet: they wait in the queue, and her panel says so. The host runner (`crate::runner`, Claude
Code on Elysium's host) is what will run them; wiring her to it is next on the roadmap.

The code is `api/src/elysia/` (status, availability, intake, trigger kinds), `api/src/models/elysia.rs`, and
`api/src/routes/v1/elysia/`. The frontend is `frontend/src/elysia/`.

## Status

Her color, derived on every read from her turns, questions, and settings (`api/src/elysia/status.rs`):

| Light       | Color  | When                                               |
|-------------|--------|----------------------------------------------------|
| `needs-you` | Pink   | She asked a question nobody has answered           |
| `running`   | Orange | A turn is running                                  |
| `errored`   | Red    | Her last finished turn failed                      |
| `done`      | Green  | None of the above                                  |

When more than one applies, needing a person wins, then running, then a failure. The colors are the `--elysia-*`
tokens in `frontend/src/theme/matter.css`.

Separately, a **hold** says why queued work is not starting, the first of: `paused` (a person paused her),
`usage-limit` (every Claude credential reached its limit; it lifts at the time shown), `no-runner` (nothing on this
deployment can run her turns), and `unavailable` (outside her hours).

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
No windows means any time. Only turns she would start herself wait for them. A schedule saved in a local zone
therefore shifts by an hour against local time when daylight saving starts or ends, until it is saved again.

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

- Running her turns on the host runner, one at a time on one Claude Code session that each turn resumes, with her
  standing instructions versioned in the repository.
- Her MCP tools: action items, initiatives, projects, changesets, and coding sessions to read; asking questions,
  recording and updating her work, and proposing changesets to write. Served by the API to her turns alone, with a
  token minted per turn.
- Usage limits: a turn refused because every credential is spent queues again for after the limit resets.
- The idle loop: with nothing queued, she revisits work she gave up on or was blocked on after a cooldown, and work
  past her estimate.
- Editing her hours in the panel, in the viewer's own zone.
- Her turns' full logs beside their summaries, expanding like a GitHub Actions step.
