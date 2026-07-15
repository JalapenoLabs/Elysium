# Networking: connecting a client to a server

Elysium is client-server: one deployment (server C) hosts everything (API,
database, realtime), and clients connect to it. The desktop (Electron) client can
connect to **any** instance the user chooses, by IP or domain, configured up front.
The browser build is always same-origin with its own API (the self-hosting
operator's concern; hosting and TLS are their responsibility, not Elysium's).

## The desktop client points at any instance

A client on laptop A and laptop B both connect to server C. The connection is a
GUI feature: a list of saved instances (name + URL), one active, shown
front-and-center before sign-in.

### Staying same-origin: the reverse proxy

The whole auth + realtime design depends on the renderer being same-origin with
its API (a first-party HttpOnly session cookie, no CORS, no `SameSite=None`, SSE on
the same origin; see `auth-model.md`, `realtime.md`). To keep that while talking to
a remote server, the Electron main process runs a local HTTP reverse proxy
(`desktop/src/proxy.ts`):

- It serves the packaged renderer at `http://127.0.0.1:<port>`, a real,
  secure-context origin, so first-party cookies, SharedWorker sharing, and SSE all
  work.
- It forwards `/api`, `/auth`, and the realtime stream to the **active** instance,
  rebinding the server's `Set-Cookie` to the local origin so the session stays
  first-party.
- The target is read per request, so switching instances takes effect on the next
  window load.

The renderer never knows which server it is talking to; it just uses relative
`/api/...`. In development the renderer is the Vite dev server (which proxies to the
local stack), so the proxy is a packaged-build concern.

### Configuring instances

Saved instances persist per user (`desktop/src/config.ts`). The renderer manages
them over `window.elysium.connection` (`list` / `active` / `add` / `remove` /
`select` / `test`). Adding or testing an address probes `GET /api/v1/instance`
(public) to confirm it is a reachable Elysium instance and show whose it is before
sign-in. Selecting a different instance clears cookies and reloads, so the client
re-authenticates against the new server.

## Hosting

Running the server and exposing it (a domain, TLS/HSTS, a reverse proxy) is the
operator's responsibility. Elysium does not provide hosting; the client simply
points at wherever the operator put their instance. The UI + API can be hosted on
any domain the operator chooses.
