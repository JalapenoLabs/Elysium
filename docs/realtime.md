# Realtime (Server-Sent Events)

Scope for the server-to-client realtime channel. **Both sides are built**: the
per-user broadcast bus, the typed `EventEnvelope`, the authenticated stream, and
the first event kinds (`org.updated`, `presence.changed`, `profile.updated`,
`session.changed`) publish live (`backend/src/{sse,events}.rs`), and the client
fans them out from one SharedWorker-owned `EventSource` per app
(`frontend/src/realtime/`: the worker broadcasts base64 frames to every window's
`MessagePort`; windows decode only the kinds they subscribe to via
`subscribeRealtime(kind, handler)`, with a direct-`EventSource` fallback when a
SharedWorker isn't available). Session-level kinds wire into Redux in
`useRealtimeSession` (mounted by `AppLayout`); feature surfaces subscribe to
their own kinds (the taskbar's org switcher refreshes on `org.updated`).

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

The built envelope (`proto/realtime.proto`, package `elysium.realtime.v1`); the SSE
`event:` name is the dotted lowercase of the kind (`org.updated`, `presence.changed`,
`profile.updated`, `session.changed`, `snapshot`):

```proto
enum EventKind {
  EVENT_KIND_UNSPECIFIED = 0;
  EVENT_KIND_SNAPSHOT = 1;          // full state dump (on connect / resync); reserved
  EVENT_KIND_ORG_UPDATED = 2;
  EVENT_KIND_PROFILE_UPDATED = 3;
  EVENT_KIND_PRESENCE_CHANGED = 4;
  EVENT_KIND_SESSION_CHANGED = 5;
  // ... extended as new push types are added (Claude session status, drive, ...)
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
    OrgUpdated org_updated = 11;
    ProfileUpdated profile_updated = 12;
    PresenceChanged presence_changed = 13;
    SessionChanged session_changed = 14;
  }
}
```

### What is built

- **The bus** (`backend/src/sse.rs`): each user has a `tokio::sync::broadcast`
  channel; every live SSE stream for that user subscribes to it, so an event
  published to a user reaches all their connections. Per-user `sequence` numbers
  the `id:` line; a lagged slow client skips dropped envelopes and resyncs on the
  next one. The registry also powers derived presence and the connection cap.
- **Fan-out** (`backend/src/events.rs`): `publish_org` (to an org's members),
  `publish_teammates` (everyone sharing an org, including the user's own windows),
  and `publish_user`. `announce_presence` fires online on connect and offline when
  the last connection drops.
- **Publishers**: `PATCH /api/v1/orgs/:id` emits `org.updated` to members;
  profile/status edits emit `profile.updated` to teammates; SSE connect/disconnect
  emits `presence.changed`; signing an account out emits `session.changed`.
- **Built**: the client SharedWorker holds one `EventSource` per app and fans
  decoded payloads to every window; `GET /api/v1/context` is the session bootstrap
  and resync, reconciled against the live stream by `as_of_sequence` (below).

The SharedWorker decodes the envelope with the generated schema and posts the
typed payload to windows, so the protobuf contract holds end to end. The `event:`
name (the `EventKind`) lets a window subscribe to only the kinds it cares about
without decoding everything.

## Auth: session cookie, never a token in a URL

`EventSource` cannot set an `Authorization` header, and we will not put a token in
the query string (it leaks into logs). Instead the SSE endpoint authenticates a
short-lived **HttpOnly, Secure session cookie**:

1. The sign-in ceremony (see `auth-model.md`) sets the opaque HttpOnly, Secure
   session cookie; no separate SSE handshake exists and no token is ever involved.
2. The SharedWorker opens `EventSource('/api/v1/events', { withCredentials: true })`;
   the browser attaches the cookie. The backend reads and validates it.
3. On expiry the backend closes the stream with an auth error; the SPA
   re-authenticates and the worker reconnects once a fresh session exists.

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

## Session bootstrap and resync (GET /api/v1/context)

Initial and resync state does not ride the SSE stream; it is a plain
authenticated request, `GET /api/v1/context`, returning the active profile, the
session's accounts, the active account's orgs, and an `as_of_sequence` (the user's
realtime high-water mark at read time). This decouples the large initial read from
the event transport and gives the client one response to authorize its pages on,
built from reads the backend runs together.

- **Order**: the client connects the SSE stream first (or reuses the
  SharedWorker's existing one), then fetches `/context`. Subscribing before
  reading is what guarantees no event slips through the gap between the two.
- **Reconcile by sequence**: the worker applies only SSE events whose `sequence`
  exceeds the context's `as_of_sequence` and discards older ones as already
  reflected. That makes apply order irrelevant and a redundant delivery idempotent,
  which is what actually closes the race, not the ordering alone.
- **Reconnect**: `EventSource` auto-reconnects; the server sets `retry:` and honors
  `Last-Event-ID` to resume missed events. The worker relays connection state
  (connecting / open / error) to windows, which powers the reactive session
  background colors.
- **Resync on focus**: when a window regains focus the worker re-fetches
  `/context` in the background and rebroadcasts it, but only when it is worth it,
  the SSE connection dropped while hidden or the tab was hidden past a short
  threshold. If the stream stayed healthy the snapshot is already current and the
  refetch is skipped. The worker keeps the last `/context` in memory, so a refocus
  restores instantly while any refetch is in flight.

`EVENT_KIND_SNAPSHOT` stays reserved but unused: the snapshot is `/context`, not an
SSE frame.

## Into Redux

The SharedWorker is the single source of truth for pushed state. Each window
receives decoded envelopes over its `MessagePort` and dispatches them into its
Redux store (the `/context` response seeds the relevant slices; deltas patch them).
Every window stays live and consistent because they all drink from one connection.

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
- **Prod**: the Electron main process serves the packaged app from a local reverse
  proxy at `http://127.0.0.1:<port>` (a real, secure-context origin, so SharedWorker
  sharing works) and forwards `/api`, `/auth`, and this stream to the chosen
  instance (see `networking.md`). This is the decided production path.
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
4. Reconnect after session expiry: the stream closes with an auth error, the SPA
   re-authenticates, and the worker reconnects on the fresh session.

Run this once PR #1 (the Electron shell) merges and a second window can be created.

## Roadmap

- **Phase 1**: **(landed)** the session cookie comes from the passkey sign-in
  (see `auth-model.md`), and `GET /api/v1/events` authenticates with it
  and enforces `MAX_SSE_CONNECTIONS_PER_USER`. **Remaining**: the `EventEnvelope`
  proto + per-user broadcast bus, and a plain browser `EventSource` client to prove
  the stream end to end.
- **Phase 2**: SharedWorker fan-out with the `/context` bootstrap + reconnect +
  resync-on-focus; dispatch envelopes into Redux.
- **Phase 3**: Electron verification (the 4-point check); `utilityProcess` fallback
  only if SharedWorker sharing proves flaky.
- **Phase 4**: RabbitMQ-to-SSE bridge so worker/service events fan out to clients.
