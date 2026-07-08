# Realtime (Server-Sent Events)

Scope for the server-to-client realtime channel. Nothing here is implemented yet;
this is the agreed design and roadmap.

## Goal

Push updates from the server to the client for the "bring things TO you" model:
action-center items, notifications, live Claude session status (the orange /
red / pink / green background states), drive and file updates, and presence. The
client-to-server direction stays ordinary protobuf API calls; realtime is a
one-directional fan-out from the server.

## Why SSE (not WebSocket)

- The need is one-directional (server to client); we do not need a bidirectional
  channel for notifications and status.
- Native auto-reconnect with `Last-Event-ID` resumption is built into
  `EventSource`.
- Plain HTTP: it rides the same Axum server and the Electron/browser origin with
  no upgrade handshake, and survives proxies.
- Simpler auth and observability than a WebSocket.

WebSocket stays available for a later, genuinely bidirectional surface (e.g.
collaborative editing). SSE is the default for notifications and status.

## Distribution: one connection per app, not per window

A single **SharedWorker** owns one `EventSource` to the API and fans events out
to every window/tab through `MessagePort`:

- On `onconnect`, register the new port, send a **state snapshot**, then stream
  deltas.
- N windows collapse to **one** server connection.
- `EventSource` is `[Exposed=(Window,Worker)]`, so opening it inside the
  SharedWorker is standard.
- The same worker code runs in the browser build and the Electron build (see the
  Electron section).
- Degraded fallback: a window that cannot reach the SharedWorker may open its own
  `EventSource` (back to N connections), so realtime never hard-fails.

This is also the vehicle for the `CLAUDE.md` plan to sync Redux between windows:
the worker holds shared state and broadcasts deltas.

## The protobuf tension

Our wire contract is binary protobuf, but SSE frames are UTF-8 text. Resolution:
each event is a proto message (an `elysium.realtime.v1` `Event` envelope)
**base64-encoded** into the SSE `data:` field. The `event:` name carries the
routing key and `id:` carries the resume cursor. The SharedWorker decodes with the
generated schema and posts the typed message to windows, so the typed contract
holds end to end.

## Auth

`EventSource` cannot set request headers, so the Keycloak bearer cannot ride an
`Authorization` header. Proposed approach: a short-lived access token as a query
param on the SSE URL (`/api/v1/events?access_token=...`), minted just-in-time by
the SharedWorker via `keycloak.updateToken()` before each (re)connect, with a
short TTL. The backend validates it exactly like the header path (same JWKS and
issuer as `backend/src/auth.rs`). On 401 or expiry, the worker refreshes and
reconnects. (Cookie-scoped auth for the SSE endpoint is the alternative if we want
to keep tokens out of URLs.)

## Reconnection and snapshots

- `EventSource` auto-reconnects; the server sets `retry:` and honors
  `Last-Event-ID` to resume missed events.
- The worker relays connection state (connecting / open / error) to windows so the
  UI can render it. This is what powers the reactive session background colors.
- On every (re)connect the server sends a snapshot first, then deltas, so a
  newly-opened window is immediately consistent.

## Backend (Axum)

- `GET /api/v1/events` returns `text/event-stream` via
  `axum::response::sse::Sse`.
- Per connection: authenticate the token, then subscribe to a per-user stream.
- Fan-in: a `tokio::sync::broadcast` (or per-user channel) receives domain events.
  The architecture's RabbitMQ / amqp bus feeds these (workers and services publish
  events; the API bridges them onto SSE).
- Filter by user/team permissions before emitting.
- Emit heartbeat comments (`:keep-alive`) to hold the connection through idle
  periods and proxies.

## Electron alignment (with PR #1)

SharedWorker sharing across windows needs the **same real origin**, the **same
session/partition**, and the **same worker URL**:

- **Dev**: PR #1 loads the app from `http://localhost:5173`, a real origin, so a
  SharedWorker is shared across windows. Good.
- **Prod**: `electron-serve` serves the built app from a `standard: true,
  secure: true` `app://-` origin, which also shares. Good. This is the decided
  production path.
- **Never load the app from `file://`.** Chromium treats it as an opaque origin
  and SharedWorker sharing breaks. PR #1's `file://` `launcher.html` is only an
  error page (not the app), so it is fine.
- Windows must stay on the **default session** (no per-window `partition`). PR #1
  uses defaults. When multi-window / rc-dock lands, every window must keep the
  same origin and default session.
- `sandbox: true` + `contextIsolation` (PR #1's defaults) do not block
  SharedWorker: it is a web feature and needs no Node in the worker.
- **Risk**: Chromium site-isolation and version quirks have occasionally broken
  SharedWorker cross-window sharing. Verify empirically on the target Electron
  version (below). If it proves flaky, fall back to an Electron `utilityProcess`
  that owns the single SSE connection and broadcasts via `MessagePortMain`
  (Electron-only; the browser build keeps the SharedWorker).

## Verification plan

Confirm on the target Electron version before building for real:

1. Two `BrowserWindow`s on the app origin share **one** SharedWorker instance
   (log a connection counter from the worker; the second connect must reuse it).
2. One `EventSource` in the worker delivers to **both** windows.
3. Reconnect + `Last-Event-ID` resume works after a dropped connection.
4. Token refresh on reconnect via `keycloak.updateToken()`.

Run this once PR #1 (the Electron shell) merges and a second window can be created.

## Roadmap

- **Phase 1**: Axum SSE endpoint + per-user broadcast; a plain browser
  `EventSource` client (no SharedWorker yet) to prove the stream and auth.
- **Phase 2**: SharedWorker fan-out with snapshot + reconnect; dispatch events into
  Redux.
- **Phase 3**: Electron verification; `utilityProcess` fallback only if needed.
- **Phase 4**: RabbitMQ-to-SSE bridge for cross-service events.
