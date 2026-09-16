# Realtime

Every open page holds one server-sent event stream, `GET /api/v1/events`. It is how the frontend stays current: a
write in one tab, a satellite coming online, and an agent's output all arrive through it. Components never poll.

## Protocol

Each SSE message is unnamed (`message`), and its `data` is one JSON envelope:

```json
{ "type": "session.event", "data": { "...": "..." } }
```

| Type                 | `data`                   | Sent when                                                  |
|----------------------|--------------------------|------------------------------------------------------------|
| `hello`              | none                     | First message on every connection                          |
| `resync`             | none                     | This client fell behind and missed events                  |
| `llm.upserted`       | `Llm`                    | An LLM credential was created or changed                   |
| `llm.deleted`        | `{ id }`                 | An LLM credential was deleted                              |
| `mailbox.upserted`   | `MailAccount`            | A mailbox was connected, changed, or checked               |
| `mailbox.deleted`    | `{ id }`                 | A mailbox was disconnected                                 |
| `mailServer.updated` | `MailServer`             | The mail server's state changed, including each creation step |
| `mailDomain.upserted`| `MailDomain`             | A domain was added                                         |
| `mailDomain.deleted` | `{ id }`                 | A domain was removed                                       |
| `project.upserted`   | `Project`                | A project was created or changed                           |
| `project.deleted`    | `{ id }`                 | A project was deleted                                      |
| `satellite.upserted` | `Satellite`              | A satellite was created or changed (status arrives later)  |
| `satellite.deleted`  | `{ id }`                 | A satellite and its sessions were deleted                  |
| `satellite.status`   | `SatelliteStatus`        | A poll found different reachability, version, or load      |
| `environmentVariable.upserted` | `EnvironmentVariable` | An environment variable was added or changed      |
| `environmentVariable.deleted`  | `{ id }`              | An environment variable was deleted               |
| `githubCredential.upserted` | `GithubCredential` | A GitHub token was added, changed, or checked             |
| `githubCredential.deleted`  | `{ id }`          | A GitHub token was deleted                                 |
| `storageLocation.upserted` | `StorageLocation`  | A storage location was added or changed                    |
| `storageLocation.deleted`  | `{ id }`           | A storage location was deleted                             |
| `session.upserted`   | `CodingSession`          | A session was created or renamed, or its thread changed    |
| `session.deleted`    | `{ id }`                 | A session was deleted; `id` is the session's number        |
| `session.event`      | `SessionEvent`           | A thread emitted an event                                  |
| `session.resync`     | `{ id }`                 | Live events for that session may have been missed          |

Payload shapes are the same JSON the REST routes return; see `docs/api.md` and `docs/coding.md`.

The stream sends a comment every 15 seconds so proxies never see it idle. nginx serves `/api/v1/events` from its
own location with buffering off and a one-hour read timeout.

## Delivery

Delivery is at most once. Nothing is stored or replayed, and the stream carries no event ids.

- A client that connects or reconnects receives `hello` and refetches everything it shows.
- A client more than 4096 events behind receives `resync` in place of what it missed, and refetches.
- `session.resync` covers the gap when the API itself lost a satellite's stream and could not resume it.

The bus is an in-process `tokio::sync::broadcast` channel (`api/src/realtime.rs`). An event published on one API
process never reaches clients of another, so the API runs as a single replica.

## Shutdown

Streams end when shutdown begins. The shutdown signal cancels a token that every stream and fleet watcher
observes, so open streams do not hold up the graceful drain. Browsers reconnect on their own and receive `hello`
from the next process.

## Frontend

`frontend/src/realtime/eventStream.ts` opens the one `EventSource` at startup, outside React, and dispatches each
envelope into Redux through a lookup table typed against `ServerEvent` (`src/realtime/serverEvents.ts`).

- `hello` and `resync` revalidate every SWR key a mounted component holds; the loaders put the responses in
  Redux. Data no component shows is fetched when one mounts. `session.resync` revalidates that session's history.
- Browsers retry a dropped stream by themselves, but give up after an HTTP error response such as a 502 while the
  API restarts. The module then reopens the stream, backing off from 1 to 30 seconds.
- `realtimeSlice` tracks the connection. The topbar dot is green when connected and amber while reconnecting.

## Adding an event

1. Add a variant to `ServerEvent` in `api/src/realtime.rs` with its `serde(rename)`.
2. Publish it where the change happens, after the write succeeds.
3. Add it to `ServerEvent` in `frontend/src/realtime/serverEvents.ts`. The handler table then fails typecheck
   until the new type is handled.
4. Add a row to the table above.

## Roadmap

- Redis pub/sub behind the bus, so more than one API replica can serve streams.
