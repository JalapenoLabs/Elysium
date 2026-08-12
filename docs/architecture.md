# Architecture

Arsox is the control plane for a fleet of agent satellites. It holds identity, tenancy, and history;
it holds no agents and runs no models. Everything that actually executes runs on a satellite the
user owns.

## The three planes

```
   browser                 Arsox                        satellite
  ─────────               ───────                      ───────────
  React SPA  ──HTTP/JSON──▶ Axum backend ──HTTP/proto──▶  satellite API
      ▲                        │    ▲                         │
      └───── event stream ─────┘    └───── WebSocket/proto ────┘
                                            (events)
```

- **The browser** talks only to Arsox. It never holds a satellite address and never holds a
  satellite secret.
- **Arsox** is an Anubis application: Postgres for the system of record, cookie sessions,
  organizations and teams, roles compiled from `config/roles.yml`. It is also a satellite SDK
  consumer, and it proxies every satellite call.
- **A satellite** owns the work: the workspace, the git checkouts, the harness processes, the LLM
  proxy, the permission enforcement, and the authoritative thread state.

## Why the browser never talks to a satellite directly

Three reasons, in order of how much they matter:

1. **The secret.** `ARSOX_SECRET` is a bearer token with full command authority over a satellite:
   destroy threads, read every thread's artifacts, rewrite permissions. Anything holding it can do
   all of that. A browser cannot hold it.
2. **Reachability.** Satellites live on home networks and behind corporate firewalls. Most of them
   have no inbound route from a random browser, which is the same reason the satellite polls GitHub
   rather than receiving webhooks.
3. **Tenancy.** Authorization is Arsox's, expressed once in `roles.yml` and enforced by Anubis
   extractors. A direct browser-to-satellite path would put a second, weaker authorization model in
   front of the same data.

The cost is that Arsox is on the hot path for every event, which is a real cost and the right trade.

## Arsox mirrors, the satellite owns

A thread's authoritative state lives on its satellite. Arsox persists a mirror: thread records,
turn records, the event stream, incidents, and usage rollups, all indexed by team.

The mirror exists for three things the satellite cannot provide:

- **Cross-satellite views.** "Everything my team ran this week" cannot be answered by asking one
  satellite, and answering it by fanning out to twelve of them is a query that fails whenever one is
  offline.
- **Survival.** Satellite threads expire on an idle TTL and take their workspace with them.
  Incidents and statistics outlive the thread on the satellite, but the conversation does not. Arsox
  keeps the record.
- **Authorization and audit.** Who started which turn, who answered which question, who approved
  which plan, and who raised a budget. That is Arsox's question and only Arsox can answer it.

**On any disagreement, the satellite wins.** The mirror is derived state and is rebuilt by replaying
from `from_sequence`, never by editing it into agreement.

## Realtime

Events flow satellite to Arsox over the satellite's unidirectional WebSocket, in protobuf, and
Arsox re-emits them to subscribed browsers.

Every event carries a `sequence` that is monotonic per thread and is persisted by the satellite
before it is sent, so a dropped connection is repaired by reconnecting with `?from_sequence=<n>`
rather than by losing events. Arsox records the highest sequence it has mirrored per thread, which
makes reconnection exact.

**This is single-process today.** Fanning an event out to browsers connected to a different replica
needs a pub/sub layer, and Anubis has no realtime channel layer yet (Anubis #32). Running more than
one Arsox replica will drop events for clients that are not on the replica holding the satellite
socket. Do not scale out until that lands.

## Data ownership

Every Arsox resource chains its ownership back to an Anubis `Team`:

```
Organization ─< Team ─< Satellite ─< Thread ─< Turn
                              │         └──< Event, Incident
                              └──< usage rollups
```

Handlers take `anubis::guard::TeamMember`, which resolves the route's `{team_id}` against the
caller's membership before the body runs and answers 404 for non-members, byte-identical to a
nonexistent id. Tenant isolation is enforced by walking that chain, never by trusting an id the
client sent.

## Secrets

Satellite bearer tokens, LLM credentials, and repository credentials are sealed at rest with
`anubis::auth::secret_box` under `ANUBIS_SECRET_KEY`, and are never returned to a client in the
clear. Responses carry a redacted display string, which is the same discipline the satellite
contract applies to its own `common.v1.Secret`: `value` goes up, `display` comes back.

Development boots without `ANUBIS_SECRET_KEY` by falling back to a fixed, public key, and says so at
startup. Production refuses to start without a real one. Rotating the key makes everything already
sealed under it unreadable, so it is generated once per environment and kept.

## Backend layout

`backend/` is one Cargo package. `main.rs` is the composition root and does nothing but wire; the
application lives in `lib.rs` beside it, which is what lets integration tests drive the real
routers.

Router mounts, in the order they are composed:

| Mount | Owner | Serves |
|---|---|---|
| `/healthz` | Arsox | Liveness |
| `/users/{user_id}/avatar` | Anubis | Public profile pictures |
| `/auth` | Anubis | Registration, sessions, MFA, passkeys, email flows |
| `/tenancy` | Anubis | Organizations, teams, memberships, invitations |
| `/developers` | Anubis | Platform applications and bearer tokens |
| `/api/v1` | Anubis | The versioned public API and its OpenAPI document |
| `/account` | Arsox | Arsox's own resources |

Migrations run in two passes at boot, framework first: application tables reference `teams` and rely
on the shared `set_updated_at()` trigger, so the order is a dependency, not a preference.

## Frontend layout

`frontend/` is one npm workspace: a React SPA on Vite and SWC.

- Routing goes through the central `UrlTree` in `src/urls.ts`. Never write a path literal into a
  component.
- Data fetching is ky plus SWR. Route functions live one file per resource under `src/api/routes/`
  and are the source of truth for their request and response types.
- Forms are react-hook-form with zod resolvers.
- Every user-facing string is an i18next key. Logs stay English and untranslated.
- Permission-driven affordances read `roles.generated.ts`, which is generated from `config/roles.yml`
  and never hand-edited.

The vite dev server proxies `/auth`, `/tenancy`, and `/healthz` to the backend, so the SPA and the
API are same-origin in development. In production they are same-origin because the backend serves
the built assets, except that Anubis does not do that yet (Anubis #49), so a production deployment
currently needs its own static host.

## Deployment

The intended shape is one static Rust binary serving the API and the built SPA, plus PostgreSQL.
Versions are pinned everywhere: Rust 1.96.0, Node 24.13.0, Yarn 4.17.0, Postgres 17.6.

Several things a production deployment needs are missing from the framework rather than from Arsox:
SPA serving (#49), an SMTP mailer (#50), graceful shutdown and request timeouts (#52), security
headers (#53), and auth rate limiting (#45). See [anubis.md](anubis.md).
