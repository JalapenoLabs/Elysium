# Realtime

Every open page holds one server-sent event stream, `GET /api/v1/events`. It is how the frontend stays current: a
write in one tab, a mailbox being checked, and an agent's output all arrive through it. Components never poll.

## Protocol

Each SSE message is unnamed (`message`), and its `data` is one JSON envelope:

```json
{ "type": "session.event", "data": { "...": "..." } }
```

| Type                 | `data`                   | Sent when                                                  |
|----------------------|--------------------------|------------------------------------------------------------|
| `hello`              | none                     | First message on every connection                          |
| `resync`             | none                     | This client may have missed events: it fell behind, or Redis lost some on the way |
| `actionItem.upserted` | `ActionItem`            | An item was created, changed, restored, or joined or left a project or initiative |
| `actionItem.deleted` | `{ id }`                 | An item was deleted (softly; it may be restored)           |
| `actionItemLink.upserted` | `ActionItemLink`    | A link was added or promoted, the watcher read a change, or a write it owes was owed, tried, landed, or cancelled |
| `actionItemLink.deleted` | `{ id, actionItemId }` | A link was removed                                     |
| `initiativeLink.upserted` | `InitiativeLink`    | A container was linked, or the watcher read it             |
| `initiativeLink.deleted` | `{ id, initiativeId }` | A container was unlinked                               |
| `actionItemComment.upserted` | `Comment`        | A comment was written or edited                            |
| `actionItemComment.deleted`  | `{ id, actionItemId }` | A comment was deleted                                |
| `initiative.upserted` | `Initiative`            | An initiative was created, changed, or restored, or its progress moved |
| `initiative.deleted` | `{ id }`                 | An initiative was deleted (softly; it may be restored)     |
| `history.appended`   | `HistoryEntry`           | A write recorded an entry in an item's or initiative's history |
| `changeset.upserted` | `Changeset`              | A changeset was proposed, decided on, applied, or undone   |
| `llm.upserted`       | `Llm`                    | An LLM credential was created or changed                   |
| `llm.deleted`        | `{ id }`                 | An LLM credential was deleted                              |
| `llm.usage`          | `{ id, usage }`          | A Claude account's usage changed; numbers and instants only |
| `mailbox.upserted`   | `MailAccount`            | A mailbox was connected, changed, or checked               |
| `mailbox.deleted`    | `{ id }`                 | A mailbox was disconnected                                 |
| `mailServer.updated` | `MailServer`             | The mail server's state changed, including each creation step |
| `mailDomain.upserted`| `MailDomain`             | A domain was added                                         |
| `mailDomain.deleted` | `{ id }`                 | A domain was removed                                       |
| `browserSession.upserted` | `BrowserSession` | A browser was added, or began starting, running, or failing |
| `browserSession.deleted`  | `{ id }`         | A browser and its profile were removed                     |
| `browserSession.startedOver` | `{ id, createdBy }` | A browser went back to its start page, other tabs closed, profile kept |
| `connector.changed`      | `{ id }`         | A connector was added or changed: paused, content, a sync, a read, its checks; load it by id or error |
| `connector.deleted`      | `{ id }`         | A connector, or the account it read through, was removed   |
| `learning.changed`       | `{ id }`         | A learning was saved, corrected, deleted, restored, or had a change undone; never its text, since guests hear it too; members load it by id, see `docs/knowledge.md` |
| `knowledgeAsk.changed`   | `{ id, askedBy, state }` | A question asked of the knowledge base was made, taken, answered, or failed; never the question or its answer; the asker loads it by id, see `docs/knowledge.md`, Asking |
| `learningCategories.changed` | none         | A workspace learning category was added, edited, reordered, or removed; members load the list again |
| `companyDoc.changed`     | none             | The company doc changed, or a change to it was proposed or decided; members load it again, see `docs/knowledge.md` |
| `agentGuidance.changed`  | none             | An admin saved or restored a version of the agent guidance; members load it again, see `docs/knowledge.md`, Agent guidance |
| `modelSetting.changed`   | `{ agent }`      | An admin chose what a step or agent runs on, or let it follow its kind again; admins load the list again, see `docs/engine.md`, Models |
| `kindModelSetting.changed` | `{ kind, projectId }` | Someone chose what a kind of agent runs on, for the workspace (`projectId` null) or one project, or put it back; admins load the workspace's list again, and a page showing a project's loads it again |
| `connectorCheckRun.updated` | `{ runId, connectorId, checkId, state }` | A check in a run moved on, or the run ended (`checkId` null, `state` `finished`); load the run for its messages |
| `timelineRecord.created` | `{ id, sourceKind, createdBy, actionItemId }` | Something was ingested; never its content, see `docs/connectors.md` |
| `timelineRecord.changed` | `{ id, createdBy }`      | What a record is attributed to changed: its owner's change or undo, or segmenting read its conversation again; its owner loads it by id |
| `draft.changed`          | `{ id, createdBy }`      | A draft was written, edited, discarded, or a send of it began, landed, or failed; never its text, see `docs/connectors.md` |
| `meeting.changed`        | `{ id, state, createdBy }` | A meeting was read or its processing moved on; never its title or transcript, see `docs/meetings.md` |
| `project.upserted`   | `Project`                | A project was created or changed                           |
| `project.deleted`    | `{ id }`                 | A project was deleted                                      |
| `elysia.updated`     | `Elysia`                 | Her status, settings, open questions, or work changed     |
| `elysiaMessage.created` | `ElysiaMessage`      | A person or Elysia added to the conversation              |
| `elysiaTurn.upserted` | `ElysiaTurn`            | One of her turns was queued, started, ended, or cancelled |
| `environmentVariable.upserted` | `EnvironmentVariable` | An environment variable was added or changed      |
| `environmentVariable.deleted`  | `{ id }`              | An environment variable was deleted               |
| `githubCredential.upserted` | `GithubCredential` | A GitHub token was added, changed, or checked             |
| `githubCredential.deleted`  | `{ id }`          | A GitHub token was deleted                                 |
| `jiraCredential.upserted` | `JiraCredential` | A Jira credential was added, changed, or checked           |
| `jiraCredential.deleted`  | `{ id }`         | A Jira credential was deleted                              |
| `storageLocation.upserted` | `StorageLocation`  | A storage location was added or changed                    |
| `storageLocation.deleted`  | `{ id }`           | A storage location was deleted                             |
| `session.upserted`   | `CodingSession`          | A session was created or renamed, or what it is doing changed |
| `session.deleted`    | `{ id }`                 | A session was deleted; `id` is the session's number        |
| `session.event`      | `SessionEvent`           | A session's turn reported an event, already recorded       |
| `session.summary`    | `TurnSummary`            | A finished turn's summary was written                      |
| `studioItem.upserted` | `StudioItem`            | A Studio item was created, renamed, pinned, or restored    |
| `studioItem.deleted` | `{ id }`                 | A Studio item was deleted, softly or for good              |
| `studioAsset.created` | `StudioAsset`           | A file was pulled from a workspace into a Studio item      |
| `studioFeedback.created` | `StudioFeedback`     | A prompt with a drawing was sent to a Studio item          |
| `backups.status`     | `BackupsStatus`          | A database backup began or ended, or an admin chose where backups go |
| `updates.status`     | `UpdatesStatus`          | Auto-update checked, began or finished an update, or what it waits for changed |
| `user.upserted`      | `User`                   | Someone signed up, was approved, changed role, or was disabled or enabled |
| `user.deleted`       | `{ id }`                 | A pending sign-up was rejected                             |
| `workspaceSettings.updated` | `WorkspaceSettings` | Sign-up was opened or closed, or the authenticator rule changed |
| `passwordVault.updated` | `PasswordVault`  | The vault was connected, its credentials replaced, or its collection chosen |
| `passwordVault.deleted` | none             | The vault was removed                                      |
| `passwordVault.status`  | `VaultStatus`    | The vault's CLI began starting, became ready, failed, or synced |
| `workflowRun.changed`   | `{ id, state }`  | A run started, it or one of its steps moved on, or a person acted on a step; never its input, output, or logs, see `docs/engine.md`, Runs |
| `stepJob.changed`       | `{ id, runId, state }` | A run's step moved on, or wrote to its log; members load what they show by id |

Payload shapes are the same JSON the REST routes return; see `docs/api.md` and `docs/coding.md`.

A write to an item also sends `initiative.upserted` for every initiative it is in, and for one it just left, since
their progress may have moved. Anything that changes a link or what it owes sends `actionItemLink.upserted` for every
link of its item, so a client replaces an item's links whole. The link watcher publishes what it applies the same way
the routes do, with the watcher as the history's actor. Deleting or restoring an initiative sends `actionItem.upserted` for each of its items,
whose `initiativeIds` leave deleted initiatives out. A write that changed nothing sends nothing. The frontend applies
every action item event to Redux; `actionItem.deleted` and `initiative.deleted` also revalidate the deleted lists,
`initiative.upserted` the initiative's burnup, and `actionItemLink.upserted` the item's live provider reads, each only
while a view shows it (`docs/frontend.md`). The link and container events land in the `actionItemLinks` and
`initiativeLinks` slices, `changeset.upserted` in the `changesets` slice, and the Elysia events in the `elysia`
slice. Elysia's intake also listens to the stream, on the server, to queue her `event` turns (`docs/elysia.md`). The Studio events land in the
`studioItems`, `studioAssets`, and `studioFeedback` slices; `studioItem.deleted` also revalidates the deleted items and
the item's own page, which then shows a soft-deleted item as deleted and a permanently deleted one as missing.

Only an active, approved person may open the stream, and every one of them receives every event. Every 30 seconds
the stream checks its session and person again and closes when either lost access (`docs/auth.md`). The stream never
extends a session, so an open tab alone does not keep anyone signed in.

The stream sends a comment every 15 seconds so proxies never see it idle. nginx serves `/api/v1/events` from its
own location with buffering off and a one-hour read timeout.

## Delivery

Delivery is at most once. Nothing is stored or replayed, and the stream carries no event ids.

- Every connection begins with `hello`, sent once the stream has subscribed to the bus. The server subscribes before
  it answers, so anything a client reads after the stream opened is covered by it.
- A client subscribes before it reads: on a page load the workspace reads nothing until its stream has opened, so the
  first `hello` has nothing to refetch. A client that reconnects, or whose stream failed before it first opened,
  refetches everything it shows on `hello`.
- A client more than 4096 events behind receives `resync` in place of what it missed, and refetches.
- A `session.event` is recorded before it is published, so a client that refetches a session's history after one
  always finds it.

## The bus

Events travel through one Redis pub/sub channel, `elysium:events`, so any process built from the API's crate publishes
to every browser: the API's own writes, and a process that serves no HTTP, such as the engine (`docs/engine.md`),
reach clients the same way (`api/src/realtime/`).

- Every process holds one subscription to the channel and relays what arrives into its own in-process
  `tokio::sync::broadcast` channel, which its event streams and in-process listeners (Elysia's intake) read.
- Publishing sends to Redis alone, never straight to the publishing process's own listeners, so every process hears
  every event in the one order Redis delivers them. A process's own events reach the channel in the order it
  published them: `publish` queues without waiting, and one task sends the queue in order, a batch per pipeline.
- A process starts serving only once Redis has confirmed its subscription, and the stream opens only while the
  subscription holds: while it is down, `GET /api/v1/events` answers `503`, and the browser retries with backoff.
  So `hello` always means "subscribed".
- A lost subscription (the connection closed, or a ping every 15 seconds went unanswered) is logged, reconnected with
  backoff up to 10 seconds, and confirmed. Only then do the process's listeners receive `resync`, since one that
  reloaded any earlier could still miss what is published before the subscription returned.
- A publish that fails loses its batch: it is logged, and `resync` is published on the channel as soon as Redis
  answers again, so every process's clients refetch. Events that do not fit in the queue (4096) while Redis is slow
  or away are dropped, logged once, and followed by `resync` after the events still queued.
- A `resync` always goes out after the events it stands in for would have, so a client never applies an event older
  than what its refetch read.
- On shutdown the bus closes last, after every loop that publishes has stopped, and flushes its queue for up to 5
  seconds, so a stopping process's last changes still reach every other process's clients.

Pub/sub channels span the whole Redis server, so the store-backed tests each use a channel of their own. A second bus
on its own connections stands in for a second process in `api/src/realtime/tests.rs`, and a TCP proxy the test cuts
proves that a dropped subscription and a failed publish each end in `resync`, and that no stream opens meanwhile.

The bus serves any number of processes: today the API and the engine (`docs/engine.md`). What else holds the API to
one replica is in `docs/engine.md`, One replica of the API.

## Signals

Beside the events, the bus carries signals between processes on a channel of their own, `elysium:signals`, which no
event stream relays to a browser (`api/src/realtime/signals.rs`). A signal tells a loop to look at work that is
already recorded: the API queued a session's turn (`sessionTurns`), Elysia's queue changed (`elysia`), a link owes its
provider a write (`linkWatcher`), a backup or a sync was asked for (`backups`, `connectorSync`), a browser is owed a
container (`browsers`), a Claude account was signed in (`claudeAccounts`), the agents' configuration repository
changed (`agentConfiguration`), a workflow run was started or a person retried, skipped, or resumed one of its
steps (`workflows`), or someone asked the knowledge base a question (`knowledgeAsks`).

- A signal carries no state, so a lost one costs latency and never work: every loop that listens also looks on a
  timer. `EventBus::signal` sends like `publish`, without waiting, and the same link to Redis carries it, so a lost
  subscription ends in `resync` here too, which every listener treats as the signal it waits for.
- A loop takes its `Wake` (`EventBus::wake_on`) before its first pass, so a signal sent during a pass wakes the next
  one; any number arriving meanwhile wake it once, since one pass serves them all.
- The one exception to carrying no state is the password vault: the API's requests and the engine's answers travel as
  `vaultRequest` and `vaultReply`, each sealed with the encryption key and bound to its id and direction, so Redis
  never holds a secret (`docs/passwords.md`).

## Shutdown

Streams end when shutdown begins. The shutdown signal cancels a token that every stream observes in the API, and every
loop and agent turn in the engine, so open streams do not hold up the graceful drain. Browsers reconnect on their own and receive `hello`
from the next process. The bus itself closes last, as above.

## Frontend

`frontend/src/realtime/eventStream.ts` opens the one `EventSource` at startup, outside React, and dispatches each
envelope into Redux through a lookup table typed against `ServerEvent` (`src/realtime/serverEvents.ts`).

- `AuthGate` opens the stream once `/api/v1/me` says the person may use the workspace, and holds the workspace back
  while the stream is `connecting`: until it opens, or fails, in which case the workspace loads without it. `/me`
  itself is read before the stream exists; a change to the person's access in between is caught by the stream's own
  access check, which closes it.
- `resync`, and `hello` on any connection but a page load's first, revalidate every SWR key a mounted component
  holds; the loaders put the responses in Redux. Data no component shows is fetched when one mounts.
  `session.resync` revalidates that session's history.
- **The running build.** As the stream starts, the module reads `/api/version` in the background: its `builtAt` names
  the build this page loaded. `hello` on any connection but a page load's first reads it again. A new build only ever
  runs in a new API process, which ends every stream, so a reconnection is the moment a new one can appear, and
  nothing asks on a clock. A build other than the loaded one offers a refresh (`docs/frontend.md`, Layout).
  `updates.status`, which every person receives, names the build of the process that sent it and feeds the same
  comparison, but it is published while streams are down for the restart, so it never stands alone.
- Browsers retry a dropped stream by themselves, but give up after an HTTP error response, such as a 502 while the
  API restarts or the 503 while its subscription is down. The module then reopens the stream, backing off from 1 to
  30 seconds.
- `realtimeSlice` tracks the connection: `connecting` until a stream first opens or fails, and again after it is
  stopped. The topbar dot is green when connected and amber while reconnecting.
- `src/bugs/67.test.tsx` holds a page load to one read of each thing, and proves a reconnection still refetches.

## Adding an event

1. Add a variant to `ServerEvent` in `api/src/realtime/mod.rs` with its `serde(rename)`.
2. Publish it where the change happens, after the write succeeds.
3. Add it to `ServerEvent` in `frontend/src/realtime/serverEvents.ts`. The handler table then fails typecheck
   until the new type is handled.
4. Add a row to the table above.
