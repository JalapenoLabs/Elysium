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
  registry renders CNCF distribution's `REGISTRY_*` env (`REGISTRY_HTTP_ADDR` on the
  routed port, `REGISTRY_STORAGE_DELETE_ENABLED`, the service secret as
  `REGISTRY_HTTP_SECRET`, and the storage driver picked from the bound `blobs` Drive:
  the filesystem driver rooted at the `blobs` container mount for an FS Drive, or the
  full `REGISTRY_STORAGE_S3_*` family for an S3 Drive). The registry runs with **no
  registry-level auth** (`REGISTRY_AUTH` unset); the proxy is its only reachable
  entrance and its gate (see *Managed registry* below).
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
- Object-store roles on a non-FS Drive carry no host mount; their provider-native
  service settings ride this seam instead. The registry's S3 blobs are wired: the
  reconciler resolves the bound S3 Drive's bucket settings and **unseals** its
  provider credential (like the git OIDC client is resolved live), and the template
  renders `REGISTRY_STORAGE_S3_*` with `ACCESSKEY`/`SECRETKEY` secret-tagged so
  neither survives a debug or log rendering.

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
    `reconcile::http_surface_port` (git → 3000, registry → 5000), and both publish their port,
    so the forge web UI and the docker registry v2 API are reachable and routed; the route binds
    to the host port that publishes it.
  - **Proxying.** Request and response bodies stream (never buffered whole); hop-by-hop
    headers (`Connection`, `Keep-Alive`, `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`,
    `Proxy-*`, and any header named in `Connection`) are stripped, `X-Forwarded-{For,Host,Proto}`
    and the upstream `Host` are set, and a `101 Switching Protocols` is tunnelled both ways so
    the websocket features of forge/registry UIs work.
  - **Forward-auth.** Every request passes through one async choke point,
    `routing::authorize_proxy(&state, &parts, integration_id) -> ProxyAuth`, before the
    upstream connection opens; the logic lives in `src/orchestration/forward_auth.rs`. It
    authenticates the request against the Elysium session cookie (`session::session_id` +
    `session::principal`) and enforces the **same** auth policies the OIDC authorize endpoint
    does (`oidc::endpoints::enforce_policies`: suspension, org membership, mandatory 2FA), so a
    suspended or non-member caller never reaches a service. A signed-out or policy-failed
    browser navigation (its `Accept` lists `text/html`) is bounced to the SPA login carrying
    `return_to` and returns afterward; every other request (API, git) gets `401`, never a
    redirect. On success the proxy injects the trusted identity header set the upstream is
    configured to trust: `X-WEBAUTH-USER` (the stable login, the sanitized email local-part),
    `X-Elysium-User` (the account uuid, matching the OIDC `sub`), `X-Elysium-Email`,
    `X-Elysium-Name`, and `X-Elysium-Groups` (comma-joined active-org team slugs), built from
    the same `Subject` an OIDC token carries.
  - **Spoofing defense.** The proxy strips every trusted identity header from the inbound
    request *before* it reads the session or forwards anything, on every path, so a
    client-forged `X-WEBAUTH-USER` / `X-Elysium-*` can neither reach an upstream nor influence
    the auth decision — only Elysium sets them. A managed Forgejo is configured for
    reverse-proxy auth (`config::render_git`): `ENABLE_REVERSE_PROXY_AUTHENTICATION`,
    `ENABLE_REVERSE_PROXY_AUTO_REGISTRATION` (header-driven JIT account creation),
    `ENABLE_REVERSE_PROXY_EMAIL` / `_FULL_NAME` with the email/full-name header names pointed at
    the injected `X-Elysium-*` headers, and `REVERSE_PROXY_TRUSTED_PROXIES` set to the private
    ranges plus loopback (the backend reaches Forgejo from the Docker gateway; the header strip,
    not this list, is the real barrier). Forward-auth composes with the OIDC login source:
    forward-auth is the zero-flicker web path, OIDC the standards path, both asserting one
    Elysium identity.
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

**Git and the container registry over HTTPS are wired at the routing front**
(`src/orchestration/forward_auth.rs`). A transport request with no Elysium session
authenticates with an `ely_pat_` token supplied as the HTTP Basic password (the username is
ignored; an empty password falls back to the username), so
`git clone https://anything:<token>@host/services/{id}/org/repo.git` and `docker login` against
`host/services/{id}` both work with no separate service login. One shared core
(`authorize_token_transport`) decodes the credential, resolves it, enforces a required scope and
the token's optional per-integration bind, runs the same account-policy gate as the
session/OIDC paths (a suspended, non-member, or mandatory-2FA-delinquent owner is refused), then
asserts the token owner's trusted identity and stamps `last_used_at` best-effort. Each transport
differs only in how it classifies a request and which realm it challenges under:

- **Git** classifies by the smart-HTTP service: a fetch (`git-upload-pack`) needs `git.read`, a
  push (`git-receive-pack`) needs `git.write`; realm `Elysium Git`.
- **Registry** classifies a docker v2 API request (path `/v2/…` after the `/services/{id}`
  strip) by method: a read (`GET`/`HEAD`) needs `registry.pull`, a write
  (`POST`/`PUT`/`PATCH`/`DELETE`) needs `registry.push`; realm `Elysium Registry`. A
  `registry.pull`-only token can never satisfy a push.

A missing or unresolvable token yields `401 WWW-Authenticate: Basic realm=…` (so git re-prompts
and `docker login` sends Basic credentials); an authenticated-but-unauthorized token yields
`403`. Revocation fails transport at once, since resolution checks live state.

**Git over SSH is deferred to the SSH router (#30).** SSH transport auth waits on the SSH
front that #30 builds; that router resolves a presented key to its owner via the existing
`resolve_ssh_key_owner` seam, then asserts the same identity, mirroring the HTTPS path.

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

## Managed git (Forgejo)

The git kind provisions a managed Forgejo, pinned by tag **and** digest
(`codeberg.org/forgejo/forgejo:13.0.1@sha256:…`). The reconciler renders its env through the
per-kind config template (`backend/src/orchestration/config.rs`, `render_git`):

- **Self-contained storage.** Repos and a **SQLite** database live together on the one
  POSIX_FS `repos` Drive (`FORGEJO__database__DB_TYPE=sqlite3`, `PATH` under the bound mount),
  so no separate Postgres container is provisioned. Postgres-backed Forgejo (a `db`-kind
  binding) is a later option.
- **Served under the proxy sub-path.** `FORGEJO__server__ROOT_URL = <app_origin>/services/{id}/`
  with `PROTOCOL=http` and `HTTP_PORT=3000`, so every generated URL (assets, clone, OAuth
  callback) is correct behind the single-origin routing proxy (#29). The container's web port
  (3000) is published on a host port and the route auto-registers when the service runs.
- **Locked down.** `INSTALL_LOCK=true` (no first-boot setup UI) and self-service local
  registration off (`DISABLE_REGISTRATION=true`): SSO is the only entry.

**SSO — Elysium is the OIDC provider.** At provision time the reconciler registers a
confidential, first-party OIDC client per git integration under a deterministic id
(`forgejo-{integration_id}`) whose single redirect URI is the Forgejo callback under the
sub-path (`.../services/{id}/user/oauth2/elysium/callback`). Registration is idempotent
(`oidc::clients::ensure_integration_client`): a re-reconcile recovers the sealed secret and
re-injects the same credentials rather than registering a duplicate. The client id, secret,
and Elysium's discovery URL (`<app_origin>/.well-known/openid-configuration`) are templated
into the container env (`ELYSIUM_OIDC_*`, secret redacted) alongside Forgejo's
`oauth2_client` account-linking behavior (auto-register + link by email on first login).

**Login-source delivery is the remaining git-SSO piece.** Forgejo stores OAuth2 login
sources in its database, not in `app.ini`/env, so the templated `ELYSIUM_OIDC_*` parameters
are consumed by a first-boot init running `forgejo admin auth add-oauth`; delivering that
idempotent init (a container command hook or one-shot init container) is tracked in the
roadmap below.

**Host-port allocation** is persisted per integration. On first provision the reconciler
(`ensure_host_port`) claims the lowest free port in the band `[20000, 31999]` in one
race-safe statement (`UPDATE ... FROM (generate_series EXCEPT taken-ports LIMIT 1) WHERE
host_port IS NULL RETURNING`), stores it on `integrations.host_port`, and returns it
unchanged on every later reconcile, so the port is stable across restarts (no container
churn) and unique per deployment host. A partial UNIQUE index (`WHERE host_port IS NOT NULL`)
is the collision authority: two reconciles racing for the same lowest-free port cannot both
win, and the loser retries onto the next gap. Deleting an integration frees its port (the
column travels with the cascade-deleted row), so a later allocation reuses the lowest freed
gap. A CHECK pins the column to the band. This replaces the former hash-of-id derivation,
whose birthday-problem collisions could fail the second container's publish.

## Managed registry (CNCF distribution)

The registry kind provisions a managed **CNCF `distribution`** (registry v3), pinned by tag
**and** digest (`registry:3.0.0@sha256:…`). distribution is the choice over Zot or Harbor: it is
the simplest, the reference implementation of the OCI registry API, and ships no bundled web UI
to secure. Zot or Harbor become the choice when a web console or vulnerability scanning matters.
The reconciler renders its env through the per-kind config template
(`backend/src/orchestration/config.rs`, `render_registry`):

- **Blobs on an OBJECT_STORE Drive.** The `blobs` role requires `OBJECT_STORE`, which only FS
  and S3 Drives advertise, so the capability gate makes binding blobs to SFTP/WebDAV an enforced
  impossibility ("git on OneDrive" for images). The storage driver follows the bound Drive:
  - **FS Drive** → the filesystem driver rooted at the `blobs` container mount
    (`/var/lib/registry`), which the reconciler bind-mounts to the host Drive area (the binding
    prefix, or the reserved `.elysium/registry/blobs`).
  - **S3 Drive** → distribution's S3 storage driver (`REGISTRY_STORAGE_S3_*`): bucket, region,
    region endpoint, and a key `rootdirectory` under the Drive's own prefix joined with the
    reserved area, plus `ACCESSKEY`/`SECRETKEY` from the Drive's **unsealed** provider
    credential (secret-tagged, never logged). The two drivers are mutually exclusive.
- **Served under the proxy.** `REGISTRY_HTTP_ADDR=:5000` matches the routed surface port; the
  container's port 5000 is published on a host port and the route auto-registers when the
  service runs, so the v2 API is reachable at `host/services/{id}/v2/`. `REGISTRY_HTTP_SECRET`
  (the sealed service secret) pins upload sessions across container replacement, and
  `REGISTRY_STORAGE_DELETE_ENABLED=true` lets image garbage collection work through the API.
- **Trust boundary: the proxy is the only auth.** The registry runs with **no registry-level
  auth** (`REGISTRY_AUTH` unset). Its published host port is Elysium-only; every request arrives
  through Elysium's reverse proxy, which authenticates the `ely_pat_` token, enforces
  `registry.pull`/`registry.push` scope, the per-integration bind, and the account-policy gate,
  and asserts the token owner's identity before forwarding. `docker login`/`push`/`pull` against
  `host/services/{id}` use Elysium credentials; no htpasswd or registry-native user store exists.
- **Host-port allocation** is the same persisted band mechanism as git (`ensure_host_port`).

Deferred: a registry **web console with its own SSO** (distribution has no UI; a Zot/Harbor
image or a separate console is the later addition) and **linked external registries** (Docker
Hub/GHCR as an OAuth-client integration, mirroring linked git).

## Roadmap

Delivered as milestones (see the GitHub milestones): Storage & Drives → Service orchestration
control plane → Elysium as identity provider → then per-service integrations (git, registry,
mail, Astraval) → runners & workers → unified action-center surfaces. Each per-service
milestone depends on the storage, orchestration, and identity foundations.

Remaining on the git milestone (M4), each an individual issue: the Forgejo login-source
first-boot init (deliver + idempotent `add-oauth`/`update-oauth`), git transport auth over SSH
(the SSH router, #30; the HTTPS scoped-token half is wired), linked GitHub/GitLab (OAuth +
repos into the unified view), the repo resource model (repos as resources with org/team
ownership and access), and the embedded git panel + federated "all repos" surface.
