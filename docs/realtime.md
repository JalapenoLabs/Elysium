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

We standardize on SSE for all server-to-client push and are not introducing
WebSocket for now. Any client-to-server action stays an ordinary protobuf API
call.

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

## The event envelope (protobuf over a text stream)

SSE frames are UTF-8, so each event is a single wrapping proto message
**base64-encoded** into the `data:` field. The wrapper is
`elysium.realtime.v1.EventEnvelope`, a discriminated union over every server-push
type so one channel carries them all:

```proto
// Discriminator, mirrored as the SSE `event:` name so a client can route or
// filter before it decodes the payload.
enum EventKind {
  EVENT_KIND_UNSPECIFIED = 0;
  EVENT_KIND_SNAPSHOT = 1;          // full state dump (on connect and on resync)
  EVENT_KIND_PROFILE_UPDATED = 2;
  EVENT_KIND_SETTINGS_UPDATED = 3;
  EVENT_KIND_TEAM_UPDATED = 4;
  EVENT_KIND_CLAUDE_STREAM = 5;
  // ... extended as new push types are added
}

message EventEnvelope {
  // Monotonic per-user id; echoed as the SSE `id:` line for Last-Event-ID resume.
  uint64 sequence = 1;
  elysium.common.v1.Timestamp emitted_at = 2;
  EventKind kind = 3;

  // Exactly one payload; the oneof is the typed discriminated union. Field
  // numbers stay stable as kinds are added.
  oneof payload {
    Snapshot snapshot = 10;
    ProfileUpdated profile_updated = 11;
    SettingsUpdated settings_updated = 12;
    TeamUpdated team_updated = 13;
    ClaudeStream claude_stream = 14;
  }
}
```

The SharedWorker decodes the envelope with the generated schema and posts the
typed payload to windows, so the protobuf contract holds end to end. The `event:`
name (the `EventKind`) lets a window subscribe to only the kinds it cares about
without decoding everything.

## Auth: session cookie, never a token in a URL

`EventSource` cannot set an `Authorization` header, and we will not put a token in
the query string (it leaks into logs). Instead the SSE endpoint authenticates a
short-lived **HttpOnly, Secure session cookie**:

1. After the Keycloak login, the SPA calls `POST /api/v1/session` with its bearer
   token (validated by the existing `backend/src/auth.rs` path). The backend
   verifies it and responds with `Set-Cookie` for an HttpOnly, Secure,
   SameSite-appropriate session cookie bound to the user. The token itself never
   touches JavaScript-readable storage for this channel.
2. The SharedWorker opens `EventSource('/api/v1/events', { withCredentials: true })`;
   the browser attaches the cookie. The backend reads and validates it.
3. On expiry the backend closes the stream with an auth error; the worker refreshes
   the Keycloak token, re-establishes the session cookie, and reconnects.

Cross-origin cookies are the wrinkle: the app origin (Vite dev server, or
`app://-` in Electron) differs from the API origin. To keep the cookie
first-party and avoid `SameSite=None`, **serve the SSE endpoint same-origin with
the app** by proxying `/api` under the app origin (Vite dev proxy today, a reverse
proxy in production). Same-origin is the clean path; a cross-origin setup would
require `SameSite=None; Secure` plus CORS credentials, which is fragile over dev
HTTP and the Electron `app://` origin.

## Connection cap

One connection per client (the SharedWorker holds it), but a user may have several
clients at once: Electron desktop, a browser, another device, plus the degraded
per-window fallback. The backend caps **concurrent SSE connections per user** and
rejects the overflow (HTTP 429). The limit is a single adjustable constant:

```rust
/// Maximum concurrent SSE connections a single user may hold across all clients.
const MAX_SSE_CONNECTIONS_PER_USER: usize = 30;
```

## What flows through the channel

Everything relevant to keeping a user's client current without a refresh: profile
changes, settings changes, team/membership changes, incoming data, other relevant
information, and Claude session streams. Each is a `kind` on the envelope. The
client should never need to reload to see the latest state.

## Reconnection, snapshots, and resync-on-focus

- `EventSource` auto-reconnects; the server sets `retry:` and honors
  `Last-Event-ID` to resume missed events.
- The worker relays connection state (connecting / open / error) to windows so the
  UI can render it. This is what powers the reactive session background colors.
- On every (re)connect the server sends a `SNAPSHOT` first, then deltas, so a
  newly-opened window is immediately consistent.
- **Resync on focus**: when a window regains focus it tells the SharedWorker,
  which requests a fresh dump so nothing is stale. Mechanism: a tiny control call
  `POST /api/v1/events/resync` (same session cookie) makes the server emit a
  `SNAPSHOT` down the existing SSE stream; the worker applies it and rebroadcasts.
  Debounced, and skippable when the connection has been continuously healthy, so
  focus churn does not hammer the server.

## Into Redux

The SharedWorker is the single source of truth for pushed state. Each window
receives decoded envelopes over its `MessagePort` and dispatches them into its
Redux store (a `SNAPSHOT` replaces the relevant slices; deltas patch them). Every
window stays live and consistent because they all drink from the one connection.

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

- **Phase 1**: `POST /api/v1/session` (bearer to HttpOnly session cookie) and the
  same-origin `/api` proxy; the Axum `GET /api/v1/events` SSE endpoint with the
  per-user connection cap and a per-user broadcast; the `EventEnvelope` proto; a
  plain browser `EventSource` client (no SharedWorker yet) to prove the stream and
  cookie auth end to end.
- **Phase 2**: SharedWorker fan-out with `SNAPSHOT` + reconnect + resync-on-focus;
  dispatch envelopes into Redux.
- **Phase 3**: Electron verification (the 4-point check); `utilityProcess` fallback
  only if SharedWorker sharing proves flaky.
- **Phase 4**: RabbitMQ-to-SSE bridge so worker/service events fan out to clients.
