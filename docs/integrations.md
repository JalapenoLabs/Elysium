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

- **`Provisioner` trait.** The control plane talks to a provisioner interface: create / start
  / stop / remove / inspect a managed service, attach volumes (bound to capability-eligible
  Drives), wire network and routes, inject secrets. A **Docker backend** ships first; the
  seam lets Podman (rootless, Docker-API-compatible — likely drives the Docker backend
  unchanged) and later a **Kubernetes backend** (`kube-rs`, ServiceAccount RBAC) slot in
  without touching integrations. Kubernetes is an opt-in backend, never the required floor —
  requiring a cluster would gut the "runs on a home machine" premise.
- **The raw socket stays out of the main API.** Docker access sits behind a **scoped
  socket-proxy** (whitelist only the API calls the provisioner makes) or a small dedicated
  provisioner sidecar. Blast radius drops from "own the host" to "manage Elysium's own
  containers."
- **Desired state in Postgres, reconciled on boot.** Managed integrations are stored as
  desired state and reconciled to actual containers when Elysium starts, so a host reboot or
  image upgrade never loses them. This is a small controller/reconciler, not a static
  docker-compose.

## Routing

Elysium's reverse proxy routes inbound traffic to the right managed container:

- **HTTP** by host/path, so a service's web UI is reachable and can be embedded.
- **SSH / TCP** for `git@…` and similar. This is the harder case: an SSH front maps a
  presented key to an Elysium user and routes to the correct backend container. Design this
  explicitly; do not assume HTTP-only.

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
