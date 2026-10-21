# Remote browser

Elysium signs in to services the way its user does: in a real browser that the host runs and streams to the user.
The user signs in on the real page, through any OAuth, MFA, or passkey prompt it shows, and the browser stays running
on the host afterwards, which keeps the session alive. Connectors then read and act through that same browser, so
Elysium uses each service as its user does and never installs anything into an organization.

Today the API runs a browser per signed-in account and relays its stream, and the web app streams it to its owner
under Settings, Browser sign-ins; Outlook is read through it (`docs/connectors.md`). The facts below were
verified against the pinned versions.

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
  the API and nginx join, and nginx forwards nothing to a browser but its media port. No browser can reach another.

Per-browser networks are what Stalwart never needed, and they change two things:

- `docker-proxy` must answer the network endpoints (`NETWORKS: 1`), to create a network per browser and connect the
  API and nginx to it. That adds no reach the API lacks today: creating a container is already root on the host,
  which `docs/security.md` names as the reason every account is approved by an admin.
- nginx is a compose service, so `docker compose up` resets its networks to the ones `compose.yml` lists and drops
  the ones the API added. The API's reconciler checks every browser's network every 10 seconds and joins nginx and
  itself back. Until it does, that browser's media port refuses connections and an open stream reconnects. A shared
  network would avoid it, but would let every browser reach every other's DevTools port.
- Docker routes a container's default gateway, and the ports it publishes, through its network of highest gateway
  priority, ties broken by name. At the default priority a browser's network can win for nginx, so the host's
  requests to the web port arrive through it, and removing that browser cuts them mid-request (seen as nginx `499`s).
  nginx and the API therefore join every browser network at priority `-1`, below their compose networks. Gateway
  priority needs Docker Engine 28.0 (API 1.48) or later; an older engine ignores it, and the API warns at startup
  (`browser.engine.outdated`).

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
found at start is always left over. The API also stops a container, giving Chromium 15 seconds to write its cookies
and release the lock, before it removes it.

The policy file (`api/src/browser/neko/policies.json`) is Neko's own with four changes: cookies are kept, pop-ups
are allowed because sign-in flows open them, DevTools is allowed so a page can be read over its protocol, and no
extension is installed, so the browser is a plain Chromium.

## Browser sessions

`api/src/browser/` runs them; `/api/v1/browser-sessions` exposes them (`docs/api.md`); `browser_sessions` stores them
(`docs/database.md`).

- **Adding one** stores it on the lowest free port with a generated member password and answers at once as
  `starting`. In the background the API pulls Neko's image if Docker lacks it, creates the browser's network, volume,
  and container, joins itself and nginx to the network, copies the setup files in, and starts it. Once Chromium's
  DevTools port answers, it opens the browser's start page in a new tab and records that in
  `start_page_opened_at`. Until it is recorded, every reconcile pass tries again, so a DevTools port that answered
  late or an API restart does not lose it. Each change of status is published as `browserSession.upserted`.
- **The reconciler** runs once at startup and then every 10 seconds over every browser. It recreates whatever is
  missing, starts a stopped container, rejoins nginx and the API to each network, and replaces a container whose spec
  changed: another image, public address, or setup file. The spec is a hash in the container's
  `dev.elysium.browser-spec` label; replacing a container keeps its profile volume. A container that never started
  is replaced too, and one whose setup files fail to copy in is removed at once, since the label claims they are in
  place. Each pass ends by removing whatever Docker holds for a browser no row owns. A failure marks the browser
  `failed` with the reason until a pass succeeds.
- **Status** is kept in memory and starts as `starting` when the API starts; the first pass sets it.
- **Removing one** removes its container, network, and volume, then its row, in a task of its own so a request that
  is given up on does not stop it halfway, and holding Docker's lock throughout. A reconcile pass waiting for the
  lock checks the row is still there before it creates anything. The accounts it was signed in to are untouched;
  their sessions simply stop being used.
- **The public address** a browser advertises is what `ELYSIUM_PUBLIC_URL`'s host resolves to (its first IPv4
  address, else its first address), or loopback for `localhost`. A lookup that fails keeps the last address that
  resolved, so a resolver's bad moment does not fail and replace every browser. A host whose public name resolves
  somewhere viewers cannot reach it from, such as a proxy that carries only HTTP, needs an explicit address, which
  is on the roadmap.
- **The desktop** is 1920 by 1080 at 30 frames a second, in UTC.

## The sign-in panel

Settings, Browser sign-ins (`/settings/browsers`) lists the signed-in person's browsers with their status, and adds
one from a preset (Outlook, Gmail) or any https page. Adding one goes straight to its page, which streams it once it
is running. The client is `frontend/src/browser/`:

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
- `nekoInput` encodes the data channel's binary messages; its tests pin the byte layouts.

Every client hears every browser's events, so the event stream keeps only the signed-in person's own browsers in
Redux.

## Roadmap

- Gmail ingest through the signed-in browser; Outlook is read today (`docs/connectors.md`).
- An explicit public address, for hosts whose public name does not resolve to where media can reach them.
- Elysia's triage turn and drafting, driving the same browser.
