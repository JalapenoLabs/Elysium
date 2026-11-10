# Remote browser

Elysium signs in to services the way its user does: in a real browser that the host runs and streams to the user.
The user signs in on the real page, through any OAuth, MFA, or passkey prompt it shows, and the browser stays running
on the host afterwards, which keeps the session alive. Connectors then read and act through that same browser, so
Elysium uses each service as its user does and never installs anything into an organization.

Today every Outlook, Gmail, Teams, and Slack connector owns a browser of its own, started with it and removed with it
(`docs/connectors.md`). The engine runs each browser and reads pages in it, the API relays its stream, and the
connector's page streams it to its owner to sign in on. The facts below were verified against the pinned versions.

## Neko

The streamed browser is [Neko](https://github.com/m1k1o/neko), a self-hosted virtual browser: a Chromium on an X
display inside a container, captured and sent over WebRTC, with mouse and keyboard sent back.

| Fact                        | Value                                                                         |
|-----------------------------|-------------------------------------------------------------------------------|
| License                     | Apache-2.0, the same as Elysium, so running and documenting it needs nothing more |
| Image                       | `ghcr.io/m1k1o/neko/chromium:3.1.6`                                           |
| Digest                      | `sha256:2471119c7b4010067f76f3421a78a8bcf35e3c223b2d1f4db084914ec915fb30`     |
| Browser                     | Chromium 154.0.8037.57 on Debian 13                                           |
| Size                        | About 635 MB                                                                  |

Elysium runs the upstream image unchanged and never redistributes it. 3.1.6 is pinned rather than 3.2.0 (released
2026-10-15) so a release has two weeks to settle before it runs here.

## How the stream reaches the user

WebRTC media is not HTTP, so it cannot share the web origin's port. It can still share nginx, which stays the single
ingress: nginx's `stream` module passes raw UDP and TCP the way it passes the mail ports.

- Each browser listens on one port for all its media, UDP and TCP multiplexed (`NEKO_WEBRTC_UDPMUX`,
  `NEKO_WEBRTC_TCPMUX`), and advertises the host's public address as its candidate (`NEKO_WEBRTC_NAT1TO1`). The
  port is the same on the container and the host, because the candidate names it.
- nginx publishes a fixed range, `59000-59007`, for UDP and TCP, and passes each port to the browser that owns it.
  One `server` block covers the range: `listen 59000-59007` and `listen 59000-59007 udp`, with
  `proxy_pass elysium-browser-$server_port:$server_port`, resolved per connection through Docker's DNS. A browser on
  port 59003 is therefore reachable as `elysium-browser-59003`.
- The UDP listener needs `reuseport`, so the kernel hands every datagram from one viewer to the same nginx worker.
  Without it a viewer's datagrams spread over the workers, each relaying from its own socket; the browser then sees
  the DTLS handshake arrive from another port than ICE checked, and ICE connects while no media flows.
- UDP is preferred. TCP is the fallback for networks that block UDP. Both were verified through
  `nginx:1.30.4-alpine`: ICE connected, and a 1280 by 720 stream arrived, over UDP alone and over TCP alone.
- The signaling WebSocket and Neko's HTTP API are never published. The API reaches them on the browser's network
  and relays signaling to the user through an authenticated route of its own (below).

The range is eight browsers. The number is a constant in the API and the range in `compose.yml` and `nginx/`; the
three change together.

nginx publishes the range on every interface, like the mail ports and unlike the web port, because the sign-in
panel is useless to a remote user until media reaches them. What arrives there is worthless on its own: a media
session needs the ICE credentials and DTLS fingerprint exchanged over signaling, and signaling runs only through the
API's authenticated stream route. Reaching the port alone yields nothing, the same argument that lets Stalwart's
ports stay open.

What the host must allow: `59000-59007` over both UDP and TCP, from wherever users reach Elysium, beside the web
port. A cloud VM needs a firewall rule for them. `docs/infrastructure.md` and `docs/security.md` list the range with
the other published ports once it is built.

A provider that refuses sign-ins from a datacenter address is handled when it is met; routing the browser's own
traffic through a VPN is the likely answer. It does not affect the stream, which flows between the user and the
host.

## The client is Elysium's own

Neko ships a web client, but Elysium cannot embed it:

- It is built on Vue 2 aliased to `vue/dist/vue.esm.js`, the build that compiles templates at runtime with
  `new Function`, which the web app's Content Security Policy forbids (no eval).
- Served from Elysium's origin it would be a same-origin frame, able to script the web app itself, and it would
  hold Neko's administrator session in the browser.

So the frontend speaks Neko's protocol directly, which is small and documented in its server's types
(`server/pkg/types/event`, `server/pkg/types/message`, `server/internal/webrtc/payload`):

- **Signaling** is JSON over the WebSocket: `system/init`, then the client sends `signal/request`, the server
  answers `signal/provide` (an SDP offer and ICE servers), the client sends `signal/answer`, and both trade
  `signal/candidate`. Candidates can arrive before the offer and are held until it is set. Verified with a test
  client in headless Chromium.
- **Input** is binary on the data channel the offer opens. Each message is a big-endian header (event `u8`, length
  `u16`) and a body: move (`x u16`, `y u16`), scroll (`dx i16`, `dy i16`, control `bool`), key and button down and
  up (`u32` X11 keysym or button). Only the session holding control moves the pointer, so the client sends
  `control/request` first.
- **Keys** are sent as X11 keysyms. `KeyboardEvent.key` maps to one directly: a printable character is its code
  point (Latin-1 as is, anything above as `0x01000000` plus the code point), and named keys come from one typed
  lookup table (`Enter` is `0xff0d`).
- **Paste** sends `clipboard/set` with the text, then control and V, so a password manager on the user's own machine
  works in the remote page. The clipboard travels over the signaling socket and the keys over the data channel,
  which nothing orders, so the keys wait until Neko's `clipboard/updated` reports the new text, or one second.

## Authentication

Neko's own sign-in stays inside the host. Each browser runs with the `multiuser` member provider and session cookies
off, so a sign-in answers a token in its body. Two passwords are generated:

- The **member** password, sealed in Postgres like any application secret. A member session can watch, hold control,
  and set the clipboard, which is everything a viewer needs, and nothing more: it cannot start a broadcast, manage
  members, change the screen, or read logs. The API signs in with it server to server (`POST /api/login`) and opens
  the upstream WebSocket with the token.
- The **administrator** password, generated for each new container and never kept, because nothing in Elysium needs
  an administrator session and Neko would otherwise accept its default one.

The browser only ever talks to the API's stream route, which:

- requires a Kratos session and Elysium's own `Origin` (a WebSocket upgrade is a `GET`, which the API's
  unsafe-request check does not cover, so the route checks the origin itself),
- answers only the person who created the browser,
- relays only the events a viewer needs (signaling, control, heartbeats, clipboard set, keyboard map) from the user
  to Neko, and drops the rest. Neko already refuses a member the administrator's events; the list is the second
  wall, so an event it misses is a bug, not an escalation.

## Elysia drives the same browser

Elysia and the connectors reach the browser over the Chrome DevTools Protocol, verified against Chromium 154:

- `--remote-debugging-port=9222` opens the port, but Neko's policy `DeveloperToolsAvailability: 2` allows only the
  browser-level endpoint: attaching to a page answers `Not allowed`, and a page's own socket never answers
  `Runtime.evaluate`. Elysium's policy sets it to `1`, which also lets the signed-in person open DevTools in their
  own browser.
- Headful Chromium binds that port to loopback and ignores `--remote-debugging-address`. A small forwarder in the
  container, run by Neko's supervisord, listens on `9223` and passes connections to it.
- Chromium refuses any `Host` header that is not an IP address or `localhost`, so the API's client sends
  `Host: localhost` whatever name it dials.
- The DevTools port has no authentication of its own. Each browser therefore has its own Docker network, which only
  the engine, the API, and nginx join, and nginx forwards nothing to a browser but its media port. No browser can
  reach another.

Per-browser networks are what Stalwart never needed, and they change two things:

- `docker-proxy` must answer the network endpoints (`NETWORKS: 1`), to create a network per browser and connect the
  engine, the API, and nginx to it. That adds no reach they lack: creating a container is already root on the host,
  which `docs/security.md` names as the reason every account is approved by an admin.
- nginx, the API, and the engine are compose services, so `docker compose up` resets their networks to the ones
  `compose.yml` lists and drops the ones the engine added. The engine's reconciler checks every browser's network every
  10 seconds and joins back itself, nginx, and every container of the compose project's `api` service, found by its
  compose labels. Until it does, that browser's media port refuses connections and an open stream reconnects. A shared
  network would avoid it, but would let every browser reach every other's DevTools port.
- Docker routes a container's default gateway, and the ports it publishes, through its network of highest gateway
  priority, ties broken by name. At the default priority a browser's network can win for nginx, so the host's
  requests to the web port arrive through it, and removing that browser cuts them mid-request (seen as nginx `499`s).
  Every container joined to a browser network joins it at priority `-1`, below its compose networks. Gateway
  priority needs Docker Engine 28.0 (API 1.48) or later; an older Docker Engine ignores it, and the engine warns at
  startup (`browser.engine.outdated`).

The forwarder and Chromium's command line are copied into the container before it first starts, through Docker's
archive endpoint, so the upstream image runs as published. Neko's supervisord loads every file in
`/etc/neko/supervisord/`.

## The profile persists

Each browser keeps its profile on its own volume at `/home/neko/.config/chromium`. Two of Neko's defaults would lose
the sign-in on every restart, and both are overridden:

- Neko's preferences make cookies session-only (`profile.default_content_setting_values.cookies: 4`), so Chromium
  deletes them when it exits. A managed policy, `DefaultCookiesSetting: 1`, keeps them; a cookie set over CDP survived
  a stop and start with it, and was gone without it.
- Chromium is started without `--bwsi`.

Replacing a container moves the profile to a container with another hostname, and Chromium then refuses to start:
its profile lock names the old one ("in use by another Chromium process on another computer"). Chromium therefore
starts through `start-chromium.sh`, which clears the lock first; only one container ever runs on a profile, so a lock
found at start is always left over. Elysium also stops a container, giving Chromium 15 seconds to write its cookies
and release the lock, before it removes it.

The policy file (`api/src/browser/neko/policies.json`) is Neko's own with four changes: cookies are kept, pop-ups
are allowed because sign-in flows open them, DevTools is allowed so a page can be read over its protocol, and no
extension is installed, so the browser is a plain Chromium.

## Browser sessions

`api/src/browser/` runs them; `/api/v1/browser-sessions` exposes them (`docs/api.md`); `browser_sessions` stores them
(`docs/database.md`). The engine's reconciler brings every browser up; the API adds, resets, and removes them. Every
change to a browser's Docker resources, in either process, holds one Postgres advisory lock (`browsers:docker`), so a
browser being created, checked, reset, and removed at once never races itself.

- **Adding one** happens only with its connector (`connectors::kinds::browser`), named for the connector's kind and
  started at the kind's start page; nothing adds a browser on its own. It stores it on the lowest free port with a
  generated member password, answers at once as `starting`, and signals the engine. The engine's reconciler pulls
  Neko's image if Docker lacks it, creates the browser's network, volume, and container, joins the network as above,
  copies the setup files in, and starts it. Once Chromium's DevTools port answers, it opens the browser's start page in
  a new tab and records that in `start_page_opened_at`. Until it is recorded, every reconcile pass tries again, so a
  DevTools port that answered late or an engine restart does not lose it. Each change of status is published as
  `browserSession.upserted`.
- **The reconciler** runs in the engine, once at startup, then every 10 seconds and whenever a browser is owed one, over
  every browser. It recreates whatever is missing, starts a stopped container, rejoins each network, and replaces a
  container whose spec
  changed: another image, public address, or setup file. The spec is a hash in the container's
  `dev.elysium.browser-spec` label; replacing a container keeps its profile volume. A container that never started
  is replaced too, and one whose setup files fail to copy in is removed at once, since the label claims they are in
  place. Each pass ends by removing whatever Docker holds for a browser no row owns. A failure marks the browser
  `failed` with the reason until a pass succeeds.
- **Ownership** is a `dev.elysium.deployment` label on every container, network, and volume: the hex SHA-256 of
  `elysium/browser/deployment:` and `ELYSIUM_ENCRYPTION_KEY`, a purpose of its own beside Kratos's and Hydra's. The
  removal at the end of a pass lists only what carries this deployment's label, so a deployment never removes a
  browser, or the profile someone signed in on, that another deployment on the same Docker host created. The label
  is part of the spec hash, so a container without it is replaced to gain it. Two deployments still cannot both run
  browsers on one host: nginx's container name and the media ports are fixed.
- **Starting over** opens the start page in a fresh tab over DevTools, then closes every page that was open before
  it (never a service worker or other background target, and the new tab first, since Chromium exits when its last
  window closes). The profile stays, so whatever the browser is signed in to stays signed in. It is the way out of a
  sign-in that landed somewhere with nowhere to go, such as a provider's error page after its MFA was reset, and is
  announced as `browserSession.startedOver`.
- **Refreshing** keeps a sign-in from going stale. Outlook, Teams, and Slack renew a rolling session whenever their app
  loads, but a browser only Elysium reads never loads anything again after its sign-in: the app keeps running, its
  reader keeps reading what the page holds, and the session behind it lapses unnoticed. So once a day
  (`REFRESH_INTERVAL`, 24 hours, a code constant) the engine loads each browser's service again, then reads its
  connector, which records whether it is still signed in: a session that lapsed reads `signed_out`, and the connector
  shows `error`. It is part of the minute's ingest pass (`api/src/connectors/refresh.rs`), not the reconciler, since
  the read that follows is the pass's own.
  - **Which browser.** At most one per pass, the most overdue: a running browser whose start page is open, once a day
    has passed since the later of its last refresh and its start page opening, so a browser just added or reset waits
    a day before anything reloads under the person signing in. The refresh is recorded on the row
    (`refreshed_at`) before it is attempted, so a restart neither reloads every browser at once nor skips one, one that
    fails waits its day, and one a minute keeps every browser in its own minute. Only active connectors' browsers are
    refreshed, as only they are read.
  - **How.** The tab showing the service, by the addresses its reader goes by (`readers::service_page`), is reloaded
    in place (`Page.reload`) and given 20 seconds after its load event to start before it is read. A reload renews the
    session exactly as a fresh tab would, keeps the person's tab where it is, steals no focus from a window they may be
    watching, and makes the very tab the reader reads show whether the session still holds; a fresh tab would leave
    the old page reading what it held. With no tab on the service, its start page opens in a new tab, as the Teams and
    Slack readers do. A tab already asking to sign in is left alone: there is nothing to renew.
  - **Logs.** `browser.refresh.done` (with its outcome: `reloaded`, `opened`, or `signing_in`),
    `browser.refresh.failed`, and `browser.refresh.checked` with the sign-in the read found, naming the browser and its
    connector's kind, never a page's address or content.
- **Resetting** empties the profile and starts the browser again at its start page. Under Docker's lock the API marks
  the start page as owed, removes the container and the profile volume, and reports the browser `starting`; the
  network stays, so everything joined to it stays joined. Then it signals the engine, which brings the browser up as if
  new. The row, its port, and its connector stay; the connector reads
  `signed_out` until the person signs in again. Removing the volume, rather than clearing data over DevTools, is
  what makes the reset complete: Chromium's own clearing reaches cookies and site storage, but not service workers,
  saved passwords, or whatever else a sign-in left in the profile, and it cannot repair a profile left half written.
  An empty volume holds nothing.
- **Status** is on the browser's row (`state`, `state_error`), as the reconciler last found it, so every process and
  replica shows the same. A new browser is `starting` until the first pass.
- **Removing one** happens only with its connector: removing the connector removes its browser. It removes the
  container, network, and volume, then the row (and with it the connector, by its foreign key), in a task of its own so
  a request that is given up on does not stop it halfway, and holding Docker's lock throughout. A reconcile pass
  waiting for the lock checks the row is still there before it creates anything. The accounts it was signed in to are
  untouched; their sessions simply stop being used.
- **The public address** a browser advertises is what `ELYSIUM_PUBLIC_URL`'s host resolves to (its first IPv4
  address, else its first address), or loopback for `localhost`. A lookup that fails keeps the last address that
  resolved, so a resolver's bad moment does not fail and replace every browser. A host whose public name resolves
  somewhere viewers cannot reach it from, such as a proxy that carries only HTTP, needs an explicit address, which
  is on the roadmap.
- **The desktop** is 1920 by 1080 at 30 frames a second, in UTC.

## The sign-in panel

A browser connector's page (`/settings/connectors/:connectorId`, `BrowserSignIn`) is where its person signs in. Adding
an Outlook, Gmail, Teams, or Slack connector starts its browser and goes straight to that page, which streams the
browser once it is running and offers **Start over** (while it runs) and **Reset browser** (unless it is starting), the
second behind a confirmation that it signs the browser out of everything. A connector waiting for a browser of its
own offers **Start browser** instead (`POST /api/v1/connectors/{id}/sign-in`). A browser's old address,
`/settings/browsers/<id>`, opens the connector that owns it. The client is `frontend/src/browser/`:

- `NekoConnection` opens the stream route, answers Neko's offer, holds the WebRTC peer, and asks for control on
  connecting (Neko hosts implicitly, so the first viewer to ask gets the pointer and keyboard). It tracks the keys
  and buttons held on the remote desktop. Once closed it detaches from its socket and peer, so reconnecting never
  hears from the old connection.
- `BrowserStream` draws the video and turns what happens on it into input: pointer moves through `toDesktopPoint`,
  which undoes the letterboxing of `object-fit: contain`; buttons by X11 number; the wheel as whole steps through
  `toScrollSteps`, which carries travel short of a step to the next event and reads pixel and line deltas; keys as
  keysyms through `toKeysym` while the video has focus; and a paste as `clipboard/set` followed by control and V.
  The pointer is captured while a button is down, so a release outside the video still arrives, and whatever is
  held is released when the video loses focus or the capture. The remote pointer's shape arrives on the data
  channel as a PNG and becomes the element's CSS cursor.
- A stream whose media never connects says so. Wherever the media ports are reachable the peer connects within a
  few seconds; a firewall that drops them leaves ICE checking for half a minute or more. So when the peer fails
  before it ever connected, or has not connected 20 seconds after opening (`MEDIA_CONNECT_TIMEOUT_MS`), the
  connection ends as `unreachable`: the panel says the picture can't reach the person, beside Reconnect, and its `?`
  names the likely cause, the media ports `59000-59007` over UDP and TCP. A peer that connected and later failed is an ordinary disconnect.
- `nekoInput` encodes the data channel's binary messages; its tests pin the byte layouts.

Every client hears every browser's events, so the event stream keeps only the signed-in person's own browsers in
Redux.

## Roadmap

- An explicit public address, for hosts whose public name does not resolve to where media can reach them.
- Elysia's triage turn and drafting, driving the same browser.
