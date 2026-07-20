# Integrations & the service control plane

Elysium does not re-implement git, SMTP, a container registry, or Perforce. It is a
**control plane** that provisions and manages best-of-breed open-source services, wires each
into the stack, and unifies them behind one identity, one storage fabric, one reverse proxy,
and one UI. "Don't reinvent the wheel" is the strategy: orchestrate proven wheels, own the
unification. Storage (`docs/storage.md`) is the foundation these services persist onto.

## What an integration is

An **integration** is to services what a **Drive** is to files: an org has a collection of
them, there can be many of each kind, and the unified surfaces aggregate *across* instances.
"All my repos" spans a managed Forgejo plus a linked GitHub plus a linked GitLab at once;
"all my mail" spans two Outlook accounts.

Each integration has:

- a **kind** — `git`, `mail`, `registry`, `perforce` (Astraval), `runner`, ...
- a **mode** — `managed` or `linked`
- an **owner scope** — `org`, `project`, or `personal`. Work resources default to
  **project** scope (`docs/projects.md`); the mail server in particular is a **per-project**
  managed integration bound to the project's domain(s). Each project's services run in their
  own isolated containers on a shared control plane.
- config, sealed secrets, and (for managed) storage-role bindings, routing, and SSO wiring.

### Managed vs linked

- **Managed** — Elysium provisions and owns the lifecycle of a container (Forgejo, a mail
  server, a registry, Astraval): storage-role volumes, routing, SSO config, upgrades. This
  is the self-hosted flagship.
- **Linked** — Elysium holds credentials to an external endpoint (github.com, an existing
  Gitea, an SMTP relay, Docker Hub) and surfaces its content into Elysium's UI. No
  container, no lifecycle; an OAuth/API integration. Cheap and immediately useful.

Both are first-class modes of one integration concept; the data model carries both from day
one. v1 prioritizes **managed** (the harder architecture and the differentiator); linked is
a per-service fast-follow.

## Ownership scope and permission gates

Integrations reuse the authz spine that governs Drives, extended one level:

| Integration type | Owner scope | Who can create | Gate |
|------------------|-------------|----------------|------|
| **Managed** (runs a container) | org | **Admins only** (`manage_integrations`) | Never members — provisioning infrastructure is admin-only |
| **Linked, org-level** (shared org Google Workspace, the org's GitHub org) | org | Admins | — |
| **Linked, personal** (my own Gmail, Outlook, GitHub) | personal | Members | **Only if an admin policy permits that kind** |

The default rule: **provisioning is always admin infrastructure; *linking* is org-level
(admin) or personal (member), and personal linking is off until an admin enables it, per
kind.** Admins flip per-kind toggles ("members may connect personal email accounts: on /
personal git accounts: off"), exactly like the existing org auth policies. This adds two
things to the data model: `owner_scope` + `owner_id` on each integration, and a per-kind
`integration_policies` toggle set on the org.

Federation composes with this: a member's unified view is **their personal integrations ∪
the org integrations they are granted**. Org integrations carry the same org-wide-vs-
restricted visibility as Drives.

## The Provisioner abstraction

The API never hardcodes Docker and never holds the raw Docker socket.

- **`Provisioner` trait.** The control plane talks to a provisioner interface
  (`backend/src/orchestration/provisioner.rs`): create / start / stop / remove / inspect /
  logs a managed service, attach volumes (bound to capability-eligible Drives), wire network,
  inject secrets. The `Provisioner` is deliberately backend-agnostic; no engine type ever
  appears in its signatures. A `ProvisionerHandle` enum dispatches to the active backend so
  `AppState` holds one cloneable handle and consumers stay generic.
- **The Docker backend** (`backend/src/orchestration/docker.rs`, `DockerProvisioner`)
  implements the trait against the Docker Engine API using `bollard` over plain HTTP. A
  **Kubernetes backend** (`kube-rs`, ServiceAccount RBAC) can slot in behind the same seam
  without touching integrations. Kubernetes is an opt-in backend, never the required floor —
  requiring a cluster would gut the "runs on a home machine" premise.
- **The raw socket stays out of the main API.** The backend never mounts
  `/var/run/docker.sock`. It talks over an internal Docker network to a **scoped
  socket-proxy** (`tecnativa/docker-socket-proxy`, pinned by tag and digest) that mounts the
  host socket read-only and whitelists only the Engine API groups the provisioner uses
  (containers, networks, images, volumes, info, POST), denying `exec`, auth, secrets,
  configs, swarm, and daemon control. Blast radius drops from "own the host" to "manage
  Elysium's own containers." The endpoint is configured by **`ELYSIUM_DOCKER_PROXY_URL`**
  (e.g. `http://socket-proxy:2375`); when unset, `AppState` selects the in-memory backend, so
  dev and test boot with zero config.
- **Podman, same code path.** Rootless Podman exposes a Docker-compatible API; point
  `ELYSIUM_DOCKER_PROXY_URL` at its socket (through the same proxy pattern) and the Docker
  backend drives it unchanged. There is no forked Podman path.
- **Hardened by construction.** The generated container config is never privileged, joins no
  host namespace, adds no capabilities, and never binds the Docker socket; `ServiceSpec`
  carries no field that could request otherwise. A **bind-mount allow-list** rejects any
  volume whose host source resolves (symlinks and `..` folded) outside the deployment data
  directory, so a spec can never mount `/`, `/etc`, or the socket. The guard runs in the pure
  spec-to-config translation, before any Docker call. Managed containers join a dedicated
  `elysium-managed` bridge network and carry an Elysium managed label plus a fingerprint of
  their desired spec; `ensure` compares the fingerprint to decide no-op versus replace, so a
  re-drive of unchanged state launches nothing new and a rotated secret triggers a clean
  replace.
- **Desired state in Postgres, reconciled on boot.** Managed integrations are stored as
  desired state and reconciled to actual containers when Elysium starts, so a host reboot or
  image upgrade never loses them. This is a small controller/reconciler, not a static
  docker-compose.

## Data model and management API

The model lives in `backend/src/orchestration/integrations.rs` (tables:
`integrations`, `integration_storage_bindings`, `integration_access`, plus
`integration_policies` in `integration_policies.rs`), the wire contract in
`proto/integration.proto` (`elysium.integration.v1`), and the routes in
`backend/src/api.rs` under `/api/v1/integrations` (protobuf, session-authenticated):

- `GET /api/v1/integrations?project_id=`: the integrations the caller can access,
  with live `status` + `status_detail`. With `project_id`, that project's set (project
  access required); without it, everything reachable in the active org: org-shared,
  reachable projects, and the caller's own personal links.
- `POST /api/v1/integrations`: create. Managed and org-linked creation require
  `manage_integrations` (project owners/admins qualify for their project's
  integrations, exactly like `manage_storage`); personal-linked creation is open to
  full members and routes through the per-kind policy hook (the `integration_policies`
  toggles enforce it). A managed personal integration is refused.
- `PATCH /api/v1/integrations/{id}`: name, visibility, non-secret `config_json`,
  or credential. Kind, mode, scope, project, and org are immutable.
- `DELETE /api/v1/integrations/{id}`: removes the container first (managed), then
  the record.
- `POST /api/v1/integrations/{id}/start` / `.../stop`: set the desired lifecycle
  (managed only); the reconciler converges immediately and the response carries the
  resulting status.
- `PUT /api/v1/integrations/{id}/bindings`: replace the storage-role bindings,
  **capability-gated**: the role must exist for the kind and the Drive must advertise
  its required capability. A project-scoped integration binds only its own project's
  Drives.
- `GET /api/v1/orgs/{id}/integration-policies` / `PATCH ...`: the per-kind
  `members_may_link` toggle board, gated by `manage_integrations` and mirroring the
  org auth-policy control plane. The GET always returns the full set (one
  `IntegrationPolicy` per kind, missing rows read as off); the PATCH upserts one
  kind's toggle (`UpdateIntegrationPolicyRequest`), writes an
  `org.integration_policy_updated` audit entry, and answers with the refreshed full
  set.

Credentials are write-only: sealed (`crypto.rs`, bound to the integration id) and
surfaced only as `credential_set`. `ReconcileStatus` is the live provisioning state
the console renders: `PENDING` (recorded, or waiting on a storage-role binding, with
the reason in `status_detail`) -> `PROVISIONING` -> `RUNNING`, `STOPPED` on an
explicit stop, `FAILED` with the reason.

### Storage roles per kind

| Kind | Role | Requires | Container mount |
|------|------|----------|-----------------|
| git | `repos` | `POSIX_FS` | `/data` |
| mail | `bodies` | `OBJECT_STORE` | `/var/mail` |
| registry | `blobs` | `OBJECT_STORE` | `/var/lib/registry` |
| perforce | `metadata` | `POSIX_FS` | `/astraval/metadata` |
| perforce | `archive` | `OBJECT_STORE` | `/astraval/archive` |
| runner | `workspace` | `POSIX_FS` | `/workspace` |

FS-backed bindings mount the Drive root joined with the binding prefix (default: the
reserved `.elysium/<kind>/<role>` area). Object-store roles on a non-FS Drive carry no
host mount; their provider-native service settings are per-kind env wiring on the
env/secrets seam.

### The reconciler

`backend/src/orchestration/reconcile.rs` converges every managed integration on boot
and after every management-API change: it plans a `ServiceSpec` (the kind's pinned
default image, bindings resolved to volumes, the env block rendered by the per-kind
config templates, one container per integration named `elysium-<kind>-<id>`, so
project-scoped services are isolated per project) and drives
`Provisioner::ensure`/`start`/`stop`, persisting the resulting status.
Idempotent throughout: a restart re-converges the same containers, never duplicates.
The `Provisioner` behind `AppState` is the Docker backend when `ELYSIUM_DOCKER_PROXY_URL`
is set, else the in-memory backend, selected without touching the reconciler. Roadmap:
fan status changes out over SSE so the console updates live.

### Config templating and secret injection

`backend/src/orchestration/config.rs` turns an integration's stored ingredients
(non-secret `config_json`, storage-role bindings, one sealed secret) into the env
its kind's blessed image expects. All per-kind knowledge lives in one lookup table
(one template per kind); no other module matches on kind to shape a service's env.

- **Pinned contracts render their real schema.** git renders Forgejo's
  `FORGEJO__<section>__<KEY>` env (installer locked, optional `domain` /
  `root_url` from `config_json`, the service secret as `security.SECRET_KEY`);
  registry renders CNCF distribution's `REGISTRY_*` env (filesystem driver rooted
  at the `blobs` mount, the service secret as `REGISTRY_HTTP_SECRET`).
- **Unpinned contracts render a documented placeholder.** mail carries only the
  base env until M6 pins the image; perforce and runner inject
  `ELYSIUM_SERVICE_TOKEN`, the intended handshake for JalapenoLabs images, with
  the full schemas owned by their per-service milestones. Every managed container
  also carries `ELYSIUM_INTEGRATION_ID`.
- **Secrets unseal at injection time only.** The reconciler opens the sealed
  secret (`crypto.rs`, bound to the integration id) right before spec planning;
  where a kind consumes a secret and none is stored, one is minted (CSPRNG),
  sealed, and persisted. Secret env values are redacted from every debug and log
  rendering of a spec, enforced by type and covered by tests. Rotating the
  credential (PATCH) changes the rendered env, so the next reconcile replaces the
  container with the new injection.
- **Failure parks, never panics.** A malformed `config_json` or an unopenable
  secret parks the row at `FAILED` with the reason in `status_detail`.
- Object-store roles on a non-FS Drive (registry blobs on S3) still need their
  provider-native env (`REGISTRY_STORAGE_S3_*`) wired from the bound Drive's
  config and credential; that rides this seam and is owned by the per-service
  milestones.

## Routing

Elysium's reverse proxy routes inbound traffic to the right managed container:

- **HTTP** by path today, so a service's web UI is reachable and embeddable. An **embedded
  hyper reverse proxy** lives in the API process (`src/orchestration/routing.rs`), not a
  managed Traefik/Caddy container. The tradeoff, recorded deliberately: the proxy sits
  in-process with the Elysium session, so forward-auth needs no external round-trip,
  single-origin SPA embedding stays clean, and there is no extra container to provision. In
  return we handle streaming and connection upgrades ourselves, on hyper (a maintained
  library), never hand-rolled HTTP.
  - **Addressing.** `ANY /services/{integration_id}/*path` on the backend strips the
    `/services/{id}` prefix and forwards `/*path` (plus query) upstream. Host-based routing
    (`git.example.com` → the git container) is a later addition on the same route table.
  - **Reachability.** A managed container publishes its web port on the host; the backend
    reaches it at `http://host.docker.internal:{published_port}`. Compose maps
    `host.docker.internal` to the host gateway (`extra_hosts`) so this resolves on Linux too.
    The future alternative is attaching the backend to the `elysium-managed` network and
    routing by container name.
  - **Route table.** A `RouteTable` (integration id → upstream host/port) lives on
    `AppState`. The reconciler registers a route when it brings a managed integration with an
    HTTP surface and a published port to *running*, and unregisters on stop, removal, or a
    parked reconcile, so a route appears and disappears with its integration; the boot
    reconcile rebuilds the whole table. A kind's HTTP surface is declared in
    `reconcile::http_surface_port` (git → 3000, registry → 5000); the route binds to the host
    port that publishes it, so a per-service milestone that publishes the port lights routing
    up with no extra wiring.
  - **Proxying.** Request and response bodies stream (never buffered whole); hop-by-hop
    headers (`Connection`, `Keep-Alive`, `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`,
    `Proxy-*`, and any header named in `Connection`) are stripped, `X-Forwarded-{For,Host,Proto}`
    and the upstream `Host` are set, and a `101 Switching Protocols` is tunnelled both ways so
    the websocket features of forge/registry UIs work.
  - **Forward-auth seam.** Every request passes through one async choke point,
    `routing::authorize_proxy(&state, &parts, integration_id) -> ProxyAuth`, before the
    upstream connection opens. It returns `Allow` today (open pass-through); M3-02 fills it in
    to require an Elysium session and assert identity to the upstream via trusted headers
    (`X-Elysium-User`, `X-Elysium-Email`).
- **SSH / TCP** for `git@…` and similar (roadmap, M2-05). This is the harder case: an SSH
  front maps a presented key to an Elysium user and routes to the correct backend container.
  Design this explicitly; do not assume HTTP-only.

## Identity: one login, zero re-prompt, for the user and Elysia

The unification crux. Without it, this is a pile of containers with separate logins. Elysium
is the single identity provider and session authority; it already owns identity deeply
(OPAQUE, passkeys, TOTP, orgs, sessions), so it is extended to speak the standard protocols
rather than delegating to a bolted-on Keycloak.

Two auth surfaces, two mechanisms:

1. **Interactive / web login — Elysium is an OIDC/OAuth2 provider.** Managed services
   (Forgejo, Harbor, ...) are OIDC clients of Elysium. Because the user already holds an
   Elysium session, opening a service tab is a pre-authenticated embedded view: no second
   login. For maximum seamlessness the reverse proxy also asserts identity via
   **forward-auth / trusted headers** (which Forgejo/Gitea/Harbor support), so there is not
   even a redirect flicker.
2. **Programmatic / transport auth — Elysium mints credentials.** Git over SSH/HTTPS,
   `docker login`, and SMTP AUTH present a credential, not an OIDC dance. Elysium mints
   per-user SSH keys and scoped access tokens and provisions them into the managed service,
   so `git push` and `docker login` just work without a hand-made PAT.

The transport-credential registry is built (`src/transport.rs`, migration `0027`). Each
account manages its own credentials under the Security Center (`/api/v1/me/ssh-keys`,
`/api/v1/me/tokens`):

- **SSH keys** register a user's public key, or generate an ed25519 keypair whose private
  half is returned once and never stored. Elysium keeps the public key and the
  server-derived SHA256 fingerprint, and resolves an inbound key to its owner by that
  fingerprint (`resolve_ssh_key_owner`).
- **Scoped tokens** are `ely_pat_` bearer secrets (256 CSPRNG bits), shown once and stored
  only as a SHA-256 hash. A token carries a `TokenScope` set (git read/write, registry
  pull/push, mail send), an optional bind to one integration instance, and an optional
  expiry. Verification hashes the presented secret and looks it up, honoring expiry
  (`resolve_access_token`). A hash, not a recoverable seal: a bearer token is only ever
  compared, so a database dump yields no usable token even with the master key.
- **Revocation deletes the row**, so transport auth, which checks live state, fails at once.
  Deleting an integration cascades to the tokens bound to it, so a token never outlives or
  broadens past its service.

Stamping `last_used_at` and enforcing a token's scopes at the SSH/HTTPS front ride the
routing layer (#29) and the git vertical (#38).

**Linked services invert the trust direction:** Elysium is the OAuth *client*, holding the
user's grant to GitHub/Google/Microsoft, silently refreshing it. For an embedded web view of
an external service, Elysium injects the session ("login script" fallback) only where no
clean API exists. The goal is to never re-prompt unless absolutely necessary.

**Elysia (the AI) uses the same fabric.** The token-minting layer serves the agent exactly as
it serves the user, so Elysia has delegated access to every integration under one identity.

## Opinionated image choices (defaults, pluggable)

- **git** → Forgejo (active AGPL fork of Gitea; ships a GitHub-Actions-compatible engine, so
  CI and runner management come largely from the image, not from building CI). Linked:
  GitHub, GitLab.
- **registry** → CNCF `distribution` or Zot (Harbor when a fuller feature set is wanted).
- **mail** → a self-hosted mail server (candidate: Stalwart / Maddy / docker-mailserver).
  Linked: Gmail, Outlook/Microsoft 365.
- **perforce** → JalapenoLabs **Astraval** (Perforce protocol, none of the cost).
- **runners / workers** → Actions runners and Claude agentic workers are the *same* managed-
  container primitive as the services above.

Collaboration features that forges bundle (pull requests, issues, code review, CI) are used
from the managed image where they exist, and where Elysium unifies them they belong to the
**action-center** surfaces (one inbox, "all repos"), shared across every integration rather
than rebuilt per service.

## Roadmap

Delivered as milestones (see the GitHub milestones): Storage & Drives → Service orchestration
control plane → Elysium as identity provider → then per-service integrations (git, registry,
mail, Astraval) → runners & workers → unified action-center surfaces. Each per-service
milestone depends on the storage, orchestration, and identity foundations.
