# Remote browser

Elysium signs in to services the way its user does: in a real browser that the host runs and streams to the user.
The user signs in on the real page, through any OAuth, MFA, or passkey prompt it shows, and the browser stays running
on the host afterwards, which keeps the session alive. Connectors then read and act through that same browser, so
Elysium uses each service as its user does and never installs anything into an organization.

None of this is built yet. This doc records what was verified against the pinned versions below, and the design the
roadmap builds.

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
  works in the remote page.

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

- `--remote-debugging-port=9222` works under Neko's policy, including `DeveloperToolsAvailability: 2`, which only
  removes the DevTools window.
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

The policy file, Neko's own with that one setting added, is copied in with the rest.

## Design

| Piece             | Where                                   | What it does                                            |
|-------------------|-----------------------------------------|---------------------------------------------------------|
| Browser sessions  | `api/src/browser/`, `browser_sessions`  | One container, volume, network, and port per signed-in account, created through `docker-proxy`, and kept in place by a reconciler |
| Stream route      | `/api/v1/browser-sessions/{id}/stream`  | Authenticated WebSocket relay of Neko's signaling       |
| Sign-in panel     | `frontend/src/`                         | Video, input, and paste over Neko's protocol            |
| Connectors        | `api/src/connectors/`                   | One trait: sign in, stay signed in, ingest, act         |
| Timeline          | `timeline_records`                      | Every ingested record, attributed to projects           |
| Outlook and Gmail | `api/src/connectors/`                   | A cheap check for new mail in the signed-in page, then records, an event, and an inbox item |

## Roadmap

- Browser sessions, the stream route, and the sign-in panel.
- The connector trait and the timeline record.
- Outlook and Gmail ingest through the signed-in browser.
- Elysia's triage turn and drafting, driving the same browser.
