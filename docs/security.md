# Security

## Trust boundary

Every API route but `GET /api/v1/auth/status` requires a Kratos session and an approved account; see
`docs/auth.md`. Everyone signed in shares one workspace: its LLM routes write credentials, its coding routes drive
agents in the workspace container, and its mail routes reach the Docker host. An account is therefore trusted with all
of that, which is why admins approve every sign-up and can close sign-up entirely.

nginx publishes the web on `127.0.0.1` by default. `WEB_BIND_ADDRESS` publishes it on another address, for a network
whose users may all hold accounts, together with an https `ELYSIUM_PUBLIC_URL`.

The mail ports (25, 465, 993) are published on every interface. They reach Stalwart alone, which authenticates
mailbox access itself and accepts unauthenticated SMTP only as delivery to its own domains, never as a relay.

The browser media ports (59000-59007, UDP and TCP) are published on every interface too. What reaches them is
useless alone: a media session needs the ICE credentials and DTLS fingerprint exchanged over signaling, and signaling
runs only through the API's stream route, which requires the browser's owner and Elysium's own origin.

The API itself has no published port. Everything reaches it through nginx, which is the
only component that can set `X-Real-IP` and `X-Forwarded-For`. The rate limiter and
the logs trust those headers for that reason alone. Publishing the API port directly
would let a client forge them.

The engine publishes nothing either, and answers nothing but its health check, which nginx never routes to. It sits
behind the same trust boundary as the API: both hold the encryption key and the database's credentials, and the bus
between them carries nothing secret in the clear (`docs/realtime.md`, Signals).

## Agents

- Agents run with every permission, as `agent` in the `elysium-workspace` container, never in the API's or the
  engine's container, which hold `ELYSIUM_ENCRYPTION_KEY` and the database's credentials. See `docs/coding.md`.
- An agent can read everything its turn was given: the model credential, the session's GitHub token, and every
  environment variable. Nothing that must stay hidden from agents is given to a turn.
- The workspace shares `elysium-agents` with the API alone, so it reaches no other service: not Postgres, Redis,
  Kratos, Hydra, or `docker-proxy`. Agents reach Elysium only at `/api/agent-tools`, with a per-turn grant scoped to
  their session, and nginx answers `404` for that path on the public origin.
- Each of the worker pool's workers runs in a container of its own, `elysium-worker-<batch id>`, on a network of its
  own that the API alone joins (`docs/engine.md`, Workers). A worker reaches the internet (GitHub, package registries,
  the model provider, and whatever else its work fetches) and Elysium's tools under its turn's grant, scoped to its
  batch; it reaches no other service, not the shared workspace, and not another worker, and nothing reaches it. Its
  network is a plain bridge, since its setup and work fetch from the internet; an allowlist of the hosts it may reach
  is on the roadmap.
- A worker is given the project's GitHub token as `GH_TOKEN` and every workspace environment variable, as a session
  is, and its events, setup's output, and log are scrubbed of their secret values before anything of them is stored
  or shown. Its project's tools script runs as root in its own container, which is its boundary.
- A worker is told never to merge; it holds the token, so that is an instruction, not a boundary. What Elysium itself
  merges is decided by the engine alone, failing closed: only a pull request the worker recorded, which the project's
  account opened from the repository itself during the batch, in a repository set to auto-merge, with every check
  passed and no review left open, naming the head commit it read and never bypassing the branch's rules
  (`docs/engine.md`, Merging).
- The API reads and writes workspace files through `docker exec` as `agent`, confined to a session's directory by
  `realpath`, so a symlink an agent plants cannot point the API elsewhere. Neither the API nor the engine mounts the
  volume.
- The event stream carries agent output, including tool input and output previews. It is served on the same
  origin as everything else and shares the API's trust boundary.

## Mail

- Mail servers are fixed per account kind in code; no request names a host the API connects to.
- OAuth credentials never reach Elysium as passwords. Refresh tokens arrive through the OAuth broker's handoff,
  redeemed server to server with a PKCE verifier, and each flow is bound to the starting browser by a cookie. See
  `docs/mail.md` for the full set of protections.
- Stalwart publishes no ports of its own; nginx passes the mail ports to it. The API skips certificate verification
  only for Stalwart, on the private mail network.
- Stalwart accepts the PROXY protocol header, which sets the client address its checks use, from nginx's fixed
  address alone. Anything else on the mail network connects as itself.
- Stalwart's administrator password is generated by Stalwart and sealed in Postgres; no person or file ever holds
  it. The temporary password it prints before setup is read from its log by the API and stops working at setup.

## Docker access

The engine and the API run the agent workspace, the mail server, the browsers, and transcriptions through Docker.
Neither mounts the Docker socket: both reach `docker-proxy`, which answers only the container, image, volume, network,
and exec endpoints, on an internal network no other service joins. Build, swarm, secrets, and system endpoints are refused.

- **The engine** creates and replaces everything: the workspace and every turn in it, browsers' containers and
  networks, the mail server brought back up, backups' dumps (`pg_dump` in the Postgres container), a Whisper
  container for each transcription, and each worker's container, volume, and network, which it joins the API to and
  limits to its share of the CPU through the container update endpoint. The proxy's allowlist needs nothing new for
  them: containers, volumes, networks, and exec are already allowed.
- **The API** works in what exists while a person waits: files in the workspace and a new session's clones, Claude
  sign-ins, creating the mail server, and resetting or removing a browser. It never creates or replaces the workspace.

The proxy narrows the surface but does not make it safe: creating a container is enough to control the host. Anyone
signed in can therefore reach the host through the mail and browser routes, which is one more reason every account
is approved by an admin. The network endpoints, which create a network per browser and join the engine, the API, and
nginx to it, add nothing to that.

## Transcriptions

- A meeting's recording is transcribed on the host by the engine's own Whisper, never by a service, so it never leaves
  the host (`docs/meetings.md`, Transcription).
- Each transcription's container has no network at all (`network_mode: none`), mounts nothing, drops every capability,
  and cannot gain privileges. The recording reaches it on its standard input and the transcript leaves on its standard
  output, over the attach stream the engine holds through `docker-proxy`.
- Its log reaches every company member through the run's page, so the transcriber reports progress, durations, and
  counts, never what was said.

## Embeddings

- The knowledge base's passages and searches' queries are embedded on the host by the embedding service, never by an
  outside service, so no text leaves the host (`docs/knowledge.md`, Embeddings).
- Its only network, `embeddings`, is internal, with no route out of the host, and only the engine and the API join
  it. nginx does not, so nothing public reaches it, and the workspace does not, so agents cannot call it.
- It answers anyone on that network without authentication, which is why nothing else joins it. What it holds is a
  public model; what reaches it is text every company member may already read (`docs/knowledge.md`, Who sees what).
- It runs as an unprivileged user on a read-only root with `/tmp` as a tmpfs, every capability dropped, and no new
  privileges, capped at 4 CPUs, 3 GiB, and 256 processes.
- The engine checks that it serves the model the engine expects before recording an embedding, and its log carries
  counts and durations, never a passage's text.

## Browsers

- A browser is signed in to its owner's accounts, so every browser route answers its owner alone, and another
  person's browser is a `404`.
- The stream route checks the origin itself: a WebSocket upgrade is a `GET`, which the unsafe-request check does not
  cover. It closes when the owner's session or account stops being allowed, checked every 30 seconds.
- The API relays a Neko member session, which can watch, hold control, and set the clipboard, and nothing else. The
  relay also passes only the events a viewer needs, so it is the second wall, not the only one. Neko's
  administrator password is generated per container and never kept.
- Each browser's DevTools port has no authentication, so each browser has its own network, joined only by the API and
  nginx, and nginx forwards only its media port. A compromised browser cannot reach another.
- A browser's profile volume holds its sign-ins. Removing the browser removes it. The API removes only what carries
  its own deployment's label, so it never removes another deployment's browser on a shared Docker host. The label is
  derived from the encryption key under its own purpose and is readable by anyone with Docker access; it reveals
  nothing about the key.
- `browserSession.upserted` and `browserSession.deleted` reach every client of the event stream, as every event does:
  the workspace shares one trust boundary. They carry a browser's name, start page, and status, never its password.

See `docs/browser.md`.

## Connectors

- A mail or meeting connector, and every timeline record it ingests, belongs to the person whose browser or key it
  reads through; those connectors and every timeline route answer them alone. Jira and GitHub connectors are the
  workspace's, like their accounts.
- `timelineRecord.created` carries ids and kinds, never a message's content, since every client of the event stream
  hears every event. The inbox item a message raises is an action item like any other, which every workspace member
  can see; per-person items are on the roadmap with owners as users.
- A connector reads a page over the browser's DevTools port, which only the engine and the API reach (above).

See `docs/connectors.md`.

## Problem reports

- A report is posted to `JalapenoLabs/Elysium`, fixed in code, with a GitHub token the reporter added themselves; no
  request names another repository or someone else's token.
- What a report carries is redacted on the server, deterministically, before the person sees it and again before it is
  posted: the workspace's known names, accounts, and sites, then patterns for addresses, secrets, URLs, ids, and IPs.
  Text from outside Elysium is posted inside fenced code blocks, so it cannot mention anyone or inject Markdown or HTML.
- A subject that names a connector or a check run is read with the reporter's own visibility, so nobody reports what
  they could not see. Each person may file ten reports a day.

See `docs/issue-reporting.md`.

## Auto-update

The updater (`updater/updater.sh`) is the one container that mounts the Docker socket, because it runs compose. It
is off unless the operator turns on the `auto-update` profile. It publishes nothing and listens on nothing: it only
calls the API's `/internal/updater` routes, which nginx never forwards, with a key derived from the encryption key.

What the API can make it do is narrow. An instruction names two versions, each checked as `X.Y.Z` before use; the
image repositories, where the configuration comes from, and every command are fixed in the script or in the release
itself. A compromised API could make it apply or roll back a published release of Elysium, and nothing else.

## Sign-in

Kratos holds passwords (Argon2id), passkeys, authenticator apps, and sessions; the API never sees a password.

- **Cookie.** The session cookie is `HttpOnly`, `SameSite=Lax`, and host-only, and it is `Secure` over https.
- **Origin.** Unsafe API requests must carry Elysium's own `Origin`.
- **Brute force.** nginx throttles sign-in, sign-up, and recovery submissions to 5 a minute per client address.
  There is no per-account lockout, which would let anyone lock a known person out. Behind a proxy, the address is
  right only when `TRUSTED_PROXY_ADDRESSES` names that proxy (`docs/auth.md`).
- **Kratos's internal routes.** Kratos calls the API's `/internal` routes. nginx never forwards `/internal`
  (production answers it with a `404`), and the routes also check a key derived from the encryption key.
- **Ending a session.** A session that is signed out, ended from the Sessions tab, or ended by an admin is refused on
  its next request, though the API caches Kratos's answers: the API ends sessions itself and forgets them in the same
  step, and nginx keeps browsers from ending one through Kratos directly (`docs/auth.md`, Ending a session).

- **MCP clients.** External MCP clients reach `/api/mcp` with OAuth access tokens Hydra issues, only after an
  approved person consents. Tokens are opaque, bound to `/api/mcp` alone, and checked against Hydra and the person's
  account on every call; a revoked one is refused on its next call. Client registration is open and throttled per
  address. See `docs/mcp.md`.

`docs/auth.md` has the whole design.

## Rate limiting

Token bucket per client IP, on `governor`: 30 requests may be spent at once, refilling at 10 per second. Both are
constants in `api/src/middleware/rate_limit.rs`, not configuration, so every deployment enforces the same limit.
Only `/api` is limited: the web app's files cost a file read each, and a cold page load fetches several at
once. The event stream counts as one request per connection. Exceeding the limit answers `429` with a JSON body,
`Retry-After`, and `x-ratelimit-*` headers. Buckets for idle clients are swept every minute so the key map stays
bounded.

A route can hold a budget of its own on the same limiter, keyed by something other than an address. The global search
(`GET /api/v1/search`) is limited per person, keyed by their user id: 20 at once, refilling 5 a second
(`PER_PERSON` in `api/src/search/mod.rs`), and answers `429` with the code `search_rate_limited` past it. Its keys
are signed-in people, whom the `users` table bounds, so nothing sweeps them.

The knowledge search (`GET /api/v1/knowledge/search`) has a budget of its own per person, lower since each search
embeds its query on the embedding service: 20 at once, refilling 2 a second (`PER_PERSON` in
`api/src/knowledge/search.rs`), answered `429` with the code `knowledge_search_rate_limited`. The same budget is spent
by the person's MCP clients and by the agents of the sessions they started (`knowledge_search`), so no way of
searching gets around it; Elysia's turns spend one of their own.

The client is the first parseable `X-Forwarded-For` address, then `X-Real-IP`, then the peer address. nginx sets
`X-Forwarded-For` to the one client address it resolved, replacing whatever the client sent, so a client cannot pick
its own bucket. Behind a proxy, that address is the client's only when `TRUSTED_PROXY_ADDRESSES` names the proxy.

**Buckets are timed with `std::time::Instant`**, which pauses while the host is suspended. governor's default
clock reads the CPU's time stamp counter, which does not survive a suspend: after one, every stored bucket sat
hours ahead of "now" and every client was refused until the API restarted. `tower_governor` fixes that clock in
its types, so the middleware drives governor directly.

The clock is the limiter's own (`LimiterClock`), so tests time a limiter by one they hold still and move by hand
(`HeldClock`). A test that counts a budget out through the router never races the refill, which on a busy runner
can keep up with the requests and leave the refusal it expects never coming. `test_support::hold_budgets_still`
holds the per-person budgets of a test's `AppState`.

A refusal asking for longer than an empty bucket takes to refill is one no working clock produces. The limiter
logs `rate_limit.clock.moved`, starts every bucket afresh, and admits the request rather than locking every
client out.

## Response headers

The API sets the helmet-equivalent set, plus `Permissions-Policy`, on every response, including `429`s and the web
app's files:

| Header                              | Value                                        |
|-------------------------------------|----------------------------------------------|
| `Content-Security-Policy`           | By path, below                               |
| `Cross-Origin-Opener-Policy`        | `same-origin`                                |
| `Cross-Origin-Resource-Policy`      | `same-origin`                                |
| `Origin-Agent-Cluster`              | `?1`                                         |
| `Permissions-Policy`                | `camera=(), geolocation=(), microphone=()`   |
| `Referrer-Policy`                   | `no-referrer`                                |
| `Strict-Transport-Security`         | `max-age=63072000; includeSubDomains`        |
| `X-Content-Type-Options`            | `nosniff`                                    |
| `X-DNS-Prefetch-Control`            | `off`                                        |
| `X-Download-Options`                | `noopen`                                     |
| `X-Frame-Options`                   | `DENY`                                       |
| `X-Permitted-Cross-Domain-Policies` | `none`                                       |

The Content Security Policy depends on what answers, in `api/src/middleware/security_headers.rs`:

- `/api` answers JSON, plain text, and the files Studio's agents wrote, so its policy is `default-src 'none';
  frame-ancestors 'none'; sandbox allow-downloads`. A browser that renders an API response directly, such as an
  agent's SVG opened in a tab, gets a unique origin and runs, loads, and embeds nothing from it; `nosniff` keeps the
  stored type. Downloads stay allowed, because Studio's file downloads are `/api` responses.
- Every other path is the web app the published image serves (see `docs/infrastructure.md`). Its policy allows only
  its own origin: `script-src 'self'` with no inline scripts and no eval, `connect-src 'self'`, `object-src 'none'`,
  `base-uri` and `form-action 'self'`, and `frame-ancestors 'none'`. Three allowances are deliberate:
  `style-src 'unsafe-inline'`, because React Aria and HeroUI write styles at runtime; `img-src data: blob:`, for
  the cover preview and inlined icons; and `font-src data:`, for fonts bundled CSS inlines.

Keeping the app inside that policy takes two things on the frontend: the pre-paint theme script is the file
`public/theme.js`, not inline, and Zod runs `jitless`, set by the first module `main.tsx` imports, so it never probes
for `new Function`. See `docs/frontend.md`.

Knowledge base pages are Markdown that agents write, so the app treats them as hostile, inside the same policy
(`docs/knowledge.md`, Pages, In the web app):

- **Rendering.** `Markdown` builds React elements and never sets HTML. Raw HTML in a page stays text, so `<script>`,
  an event handler, or an `<iframe>` creates no element.
- **Addresses.** Only `http`, `https`, `mailto`, and relative addresses survive, so `javascript:` and `data:` links
  and images are blanked whatever their encoding. A link's text stays; an image says it is unavailable.
- **Images.** Only Elysium's own origin loads. `img-src` stays as it is, since allowing other hosts would let a page
  tell a third party who read it and when. An image elsewhere is a link that opens in a new tab, with no referrer.
- **Highlighting.** highlight.js marks tokens with class names, and evaluates nothing and needs no WebAssembly.
  Highlighters that do (Shiki) would need `'wasm-unsafe-eval'`, and write inline styles.

Studio's 3D viewer (`@google/model-viewer`, see `docs/studio.md`) runs inside the same policy:

- **Decoders.** Its Draco, KTX2, and Meshopt decoders are off. They load from Google's CDN and run as blob workers
  with WebAssembly, which would need another origin in `connect-src`, `blob:` in `worker-src`, and
  `'wasm-unsafe-eval'`. The viewer refuses every URL off the app's own origin, so they never load, in any
  environment. Elysium's own glTF export never compresses.
- **Textures.** three.js reads a model's embedded textures by fetching `blob:` URLs, which `connect-src 'self'`
  refuses. Textured models render without their textures in production until the policy allows `blob:` in
  `connect-src`.
- **Studio's files.** They are served under the `/api` policy above (`docs/studio.md`). The app shows them only as
  `<img>` sources and viewer models, all from `/api`.

In the development stack nginx sends the app's paths to the Vite dev server instead. The dev server needs inline
scripts and websockets, so those pages carry no CSP, only the headers nginx adds there (`nosniff`, `DENY`,
`Referrer-Policy`, `Permissions-Policy`). That stack is for local work, never a deployment. nginx hides its version
in both stacks.

## CORS

Explicit allow-list from `CORS_ALLOWED_ORIGINS`, set inline in `compose.yml`. It is empty because the frontend and
API share one origin through nginx, so browsers never need a cross-origin grant.

## Request hygiene

- 30 second handler timeout (`408`). It bounds producing the response, so the event stream, once open, is not
  cut off.
- 1 MiB body cap in both nginx and the API (`413`).
- Panics in a handler become `500` instead of dropping the connection.
- The API container runs as an unprivileged user.

## Secrets

`.env` is ignored by git, and `.env.example` documents its keys. It holds the infrastructure credentials needed to
bootstrap the stack: Postgres and Redis credentials, `ELYSIUM_ENCRYPTION_KEY`, and optionally `RUST_LOG`.
Application secrets never go there. They are stored in Postgres, encrypted with that key; see `docs/secrets.md`.

## Roadmap

- Narrowing the API's Docker access to the containers and volumes it manages (labels, no privileged mode, no
  host mounts), then publishing nginx beyond loopback by default.
- Deciding on `blob:` in the web app's `connect-src`, which Studio's viewer needs to show textured models. It would let
  the app fetch blob URLs it created itself, and nothing from another origin.
- TLS termination at nginx.
- An egress allowlist for workers' networks: GitHub, the package registries, and the model provider alone, through a
  proxy their networks route through.
