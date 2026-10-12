# Architecture

Elysium is a control plane. It holds identity, tenancy, policy, and history; it runs almost
nothing itself. Files live at their provider, services run in containers Elysium provisions,
and agents run on satellites the user owns.

That is the through-line of every decision here: **own the unification, orchestrate the
wheels.**

## The planes

```
   browser                Elysium                     what actually runs
  ─────────              ─────────                   ────────────────────
  React SPA ──REST/JSON──▶ Axum backend ──Docker API──▶ managed containers
      ▲                       │    ▲                     (Forgejo, mail, registry)
      │                       │    │
      │                       ├────┼──────OpenDAL──────▶ Drives
      │                       │    │                     (local FS, S3, SFTP)
      │                       │    │
      │                       └────┼───HTTP + proto────▶ Arsox satellites
      │                            │
      └────── realtime ────────────┘◀──WebSocket/proto──┘
```

- **The browser** talks only to Elysium. It never holds a satellite address, a satellite
  secret, a Drive credential, or a Docker socket.
- **Elysium** is an Anubis application: Postgres as the system of record, cookie sessions,
  organizations and teams, roles compiled from `config/roles.yml`, realtime channels, a
  durable job queue. On top of that it is a storage gateway, a service provisioner, an OIDC
  provider, and a satellite SDK consumer.
- **Everything else** owns its own work: a Drive's provider owns the bytes, a managed
  container owns its service, a satellite owns its threads.

## Elysium mirrors, the source owns

The same rule applies to all three of Elysium's downstreams, and it is the most important
architectural decision in the repository.

- **Files are never mirrored.** A Drive is a live window onto its provider. Browsing calls
  the provider in real time. Deleting in Elysium deletes at the provider. The provider is
  always the source of truth. Search is live and federated rather than index-backed, because
  an index is a mirror and a mirror goes stale. See [storage.md](storage.md).
- **Managed services are desired state, reconciled.** Postgres holds what should be running;
  the reconciler converges containers onto it on boot and after every change through the
  management API. Idempotent throughout: a restart re-converges the same containers and never
  duplicates. See [integrations.md](integrations.md).
- **Agent threads are mirrored, and the satellite wins.** A thread's authoritative state
  lives on its satellite. Elysium persists a mirror so it can answer cross-satellite
  questions, survive a thread's idle expiry, and audit who did what. On any disagreement the
  mirror is rebuilt by replaying from `from_sequence`, never edited into agreement. See
  [satellites.md](satellites.md).

The pattern is the same each time: **derived state is always rebuildable, and never
authoritative.**

## Why the browser never reaches past Elysium

Three reasons, in order of how much they matter:

1. **Credentials.** A satellite bearer token has full command authority over that satellite.
   A Drive credential can read every file in an organization. The Docker socket is root on the
   host. None of these can be in a browser, and there is no version of "just for admins" that
   makes it acceptable.
2. **Reachability.** Satellites and managed containers live on private networks. Most have no
   inbound route from a random browser.
3. **Authorization.** It is Elysium's, expressed once in `config/roles.yml` and in the project
   resolver, enforced by the framework's guards. A direct browser-to-anything path would put a
   second, weaker authorization model in front of the same data.

The cost is that Elysium is on the hot path for every byte and every event. That is a real
cost and the right trade. Presigned URLs relieve it for large transfers where the provider
supports them, which is an optimization inside the boundary rather than a hole in it.

## The trust boundary is infrastructure, never instructions

Elysium stores and forwards agent permissions. It does not enforce them, and the interface
must never imply it does. Enforcement lives on the satellite, in an egress proxy, an exec
broker, a root-owned pre-push hook, and filesystem scoping the agent cannot reach.

The same principle shapes the storage capability model. A consumer requires a capability, a
provider advertises one, and binding git to an SFTP Drive is an enforced impossibility rather
than a documented warning. A rule the system can check beats a rule written in a document.

## Composition

- **`backend/`** is one Cargo package: an Axum server composing the framework's routers with
  Elysium's own. Application migrations are Elysium's, applied after the framework's, because
  application tables reference framework tables.
- **`frontend/`** is one npm workspace: a React SPA on `@jalapenolabs/anubis` and HeroUI, with
  rc-dock for the workspace. Two frames share one router, per
  [workspace.md](workspace.md).
- **`config/`** holds `roles.yml` and `billing.yml`, each compiled into both ends and drift-
  gated in CI.
- **Deployment** is one binary serving the API and the built SPA, from the framework's
  three-stage image.

## Running it

Two ways, and they answer different questions.

**`yarn dev` is the loop.** Postgres runs in Docker, the backend and Vite run on the host,
and both hot-reload. This is what a working day looks like, and `compose.yaml` exists to
serve it: datastores only, no application container.

**`compose.app.yaml` runs the whole thing in Docker**, the release image and its database:

```sh
docker compose --env-file .env.app -f compose.app.yaml up -d --wait
```

It builds the production image, so a source edit means a rebuild. Reach for it to see the
deployment shape, to hand somebody a URL, or to prove the image actually boots. It takes its
own compose project name and network, and every port comes from `.env.app`, so it runs beside
whatever else the host is already serving instead of fighting it for a port.

Three environment values have no defaults and the server refuses to start without them:
`DATABASE_URL`, `APP_URL`, and `ANUBIS_SECRET_KEY`. `APP_URL` is the origin a browser sees,
and email links, OAuth redirect URIs, and the websocket origin are all built from it, so it
has to match the address the container is actually reached on rather than the container's own
view of itself.

The application reaches Postgres by service name over the compose network, never through the
published host port. The published port is for `psql` and for reading the database while
something is wrong.

## Where the tenancy sits

Every resource chains its ownership to a tenant, and the framework's guards walk that chain
rather than trusting a client-supplied id. Elysium's chain runs organization to project to
team, and **the project tier does not exist in the framework yet**, which blocks Elysium's
domain model. See [tenancy.md](tenancy.md) and
[Anubis #93](https://github.com/JalapenoLabs/Anubis/issues/93).
