**Source-of-truth policy.** The USER is the primary source of truth. This `CLAUDE.md` is the
SECOND source of truth. The codebase is **not** a source of truth (it can drift). Whenever a
design decision is made or changed, update this file in the same change. Keep it routinely
up to date.

**Style:** No em dashes anywhere in user-facing text (docs, UI, commits, PRs).

# Elysium

Elysium is a self-hosted business workspace: mail, chat, storage, source control, tasking,
automation, and an agentic AI, in one deployment you run on your own machine. The AI inside it
is **Elysia**. Open-sourcing it is the roadmap; see Git below for what is true today.

The frustration it answers is concrete. Google Workspace and Microsoft Teams are several
products stapled together, OneDrive is a bolt-on, Drive is expensive and annoying, the
management UI is bad in all of them, and none of it can be self-hosted. Elysium is the
attempt to unify that surface rather than clone it.

Four repositories make up the whole:

| Repository | Role | Relationship |
|---|---|---|
| **elysium** (this repo) | The workspace and its control plane | Consumer of the other three |
| [Anubis](https://github.com/JalapenoLabs/Anubis) | The SaaS framework Elysium is built on | Elysium is an application stamped by `anubis new` |
| [arsox-satellites](https://github.com/JalapenoLabs/arsox-satellites) | The agent execution plane: satellite images, the protobuf contract, the SDKs | Elysium is an SDK consumer |
| [uikit](https://github.com/JalapenoLabs/uikit) | Shared Jalapeno Labs React components, hooks, and utilities | Elysium consumes and contributes back |

**Arsox keeps its name as the execution plane.** A satellite is an Arsox satellite; Elysium
is what drives it.

# The three rules that govern everything

## 1. The Anubis-first rule

The single most important working rule in this repository. Not negotiable, not a suggestion:

> If there is a feature or upgrade that is appropriate to have Anubis host, but Anubis
> doesn't yet have the functionality for it, then it's required to either (a) build it out
> on Anubis first (if it's light and easy to do) via PR or (b) create an issue or find an
> existing issue on Anubis's github repository and consider your own task blocked until the
> Anubis framework supports your needs.

Ask the question before writing the workaround, not after. The test is ownership, not
difficulty: **would every application built on Anubis want this?** If yes, it belongs to the
framework.

**Belongs to Anubis:** authentication, sessions, MFA, passkeys, OAuth sign-in.
Organizations, teams, memberships, invitations, roles, permission checks. The ownership-chain
guards and tenancy scoping. Mail delivery, background jobs, realtime channels, notifications,
the audit log, webhooks in both directions, billing. Generated API clients, OpenAPI,
scaffolding, the field component library. Server hardening. Anything a second Anubis
application would copy out of this repo verbatim.

**Belongs to Elysium:** storage Drives and the federated file gateway. Service integrations
and the provisioner that runs them. Elysium as an OIDC provider and as an OAuth client for
linked services. Transport credentials (SSH keys, scoped tokens). Mail as a product, above
the framework's transactional mailer. The docked workspace, the Elysia session surface, and
the action center. Satellites, threads, turns, and everything about driving an agent harness.

If you cannot answer the ownership question confidently, ask the user. Do not resolve the
ambiguity by writing the code here and promising to upstream it later, because that promise
is never kept and the framework quietly stops being the framework.

**Blocked is a real outcome.** When (b) applies, the task is blocked and saying so is the
correct deliverable. Do not vendor framework code into Elysium, reach around the framework's
API into its internals, or ship a "temporary" reimplementation of a framework concern. Do
file or find the Anubis issue, link it from [docs/anubis.md](docs/anubis.md), finish every
part of the task that is not blocked, and report exactly what was left undone and why.

The live register of what Elysium is waiting on lives in [docs/anubis.md](docs/anubis.md).
Keep it current. It is how the framework learns what its first real consumer needs.

## 2. Do not redraw what the framework draws

Anubis ships the app shell, every auth page, the settings screens, the organization and team
management screens, billing, the developers console, and the audit log. **Those keep working
as stamped.** Elysium adds surfaces beside them; it does not reimplement them.

The seam is stated in [docs/workspace.md](docs/workspace.md): the stamped `AppShell` frames
the framework's screens and every scaffolded model page, and the rc-dock workspace frames
Elysium's own panels. Two frames, one router.

## 3. Do not reinvent the wheel, orchestrate proven ones

Elysium does not re-implement git, SMTP, a container registry, or Perforce. It **provisions
and manages best-of-breed open-source containers** (Forgejo for git, Postfix and Rspamd for
mail, a registry, Astraval for Perforce) and unifies them behind one identity, one storage
fabric, one reverse proxy, and one UI. Orchestrate proven wheels; own the unification. See
[docs/integrations.md](docs/integrations.md).

# The vision

## Everything in one, and optionally connected to what you already use

Elysium hosts its own everything (Postgres, storage on local disk, mail, git) **and**
optionally connects to what exists. Point storage at Google Drive and use it as the backing
store, or use your own. Use Slack for messaging or use Elysium's. Use GitHub or run Forgejo.
Use Jira or use the built-in tasking. Link Gmail or run the mail server.

**We do not re-create these tools one for one.** They all have real flaws. The point is to
challenge the UX, unify, and unite, not to clone.

## The interface is inverted: things come to you

The user is the passenger. Elysia and automation are driving, and the interface exists to
give the passenger what they need to make quick, informed decisions.

You no longer open a chat tab, then an email tab, then a drive tab, then five git tabs.
Everything is brought to you in one **action center**. Messages and emails live in the same
stream, because the distinction is an implementation detail of the last thirty years, not a
thing a person cares about. When a message arrives, the interface takes you to it. When
Elysia writes a file to a Drive, you can follow it and see it immediately.

The unit is **resolved**, not **read**. Resolving an item moves you to the next action.
There is an auto-pause so you are never yanked out of something you are in the middle of.

Navigation is `ctrl+P`, VS Code style, across people, contacts, emails, messages, and
repositories. Lightning fast, universal.

## The Elysia session is the flagship screen

Not a chat transcript. A full agent session surface:

- A **status-colored ambient field**: orange while running, red on error, pink when it needs
  you, green when done. The background describes what is happening.
- An **icosahedron session selector** the user rotates to switch between concurrent Elysia
  sessions.
- **Voice**, so the agent reports as it goes, and a **sound grammar** for completion, needing
  help, and failure.
- Deep tooling and reporting around the conversation: usage, cost, plans, questions,
  incidents, artifacts.

## The workspace is dockable

rc-dock is the mechanism. Panels split, re-panel, float, maximize, and pop out to real
windows. Open the drive on the left and keep Elysia on the right. In the Electron shell a
person can put the action center on one monitor and the session on another. In a browser,
one window is a fine limit.

There is a built-in browser panel, so Elysia can open a page, scroll, screenshot, and
highlight. "Order a pizza nearby" shows three locations on a map, you pick one, and you watch
it happen.

## Faults are never silent

Every failure, agent and application alike, lands in one internal fault center with a stable
code, so issues are audited and fixed rather than inferred from silence. This mirrors the
satellite contract's incident rule: a code path that swallows a failure is a bug.

# Tenancy: org, project, team, individual

Four tiers, designed in [docs/tenancy.md](docs/tenancy.md), which is the source of truth.

- **Organization** is a thin umbrella: logo, membership, org-level teams and roles,
  cross-project administration, billing. A **full member reaches every project**; guests are
  scoped to specific projects and teams.
- **Project** is the sub-tenant workspace (the GCP-project and Jira-project analog) that
  **owns its domains, Drives, integrations, teams, and membership**. Every organization has
  at least one, auto-created.
- **Teams** are org-level (inherited by every project) or project-level (scoped).
- **Runtime**: a shared control plane (one Postgres, API, router, SMTP router) with
  **per-project service containers** for isolation. One machine hosts many organizations,
  each with many projects.

**Anubis has only two tiers, so the project tier is blocked on
[Anubis #93](https://github.com/JalapenoLabs/Anubis/issues/93).** Per the Anubis-first rule
the tier is being built in the framework, and Elysium's domain model waits for it. Nothing
here models a project in the meantime.

**Configurability principle:** plan for deep, granular, per-tier configuration with sane
defaults, in the spirit of admin.google.com.

# Storage and service integrations

Two foundational pillars, designed in [docs/storage.md](docs/storage.md) and
[docs/integrations.md](docs/integrations.md), which are the sources of truth.

- **Storage is a gateway, not a sync engine.** A project owns many **Drives** (local
  filesystem, S3, SFTP first; WebDAV and OAuth clouds later), each an OpenDAL operator rooted
  where the project points it. The provider is always the source of truth; nothing is
  mirrored. Search is live, federated, streaming, and index-free. **Capacity is your Drive**,
  so there is no artificial ceiling.
- **Providers advertise capabilities; consumers require them.** `PLAIN_FILES` from any
  provider, `OBJECT_STORE` from filesystem and S3, `POSIX_FS` from local filesystem only. A
  consumer binds each storage role to a capability-eligible Drive. This is why git and
  Astraval metadata are local-filesystem-only, and why "git on OneDrive" is an enforced
  impossibility rather than a documented warning.
- **An integration is to services what a Drive is to files.** Many per project, each with a
  `kind` (git, mail, registry, perforce, runner), a `mode` (managed or linked), and an owner
  scope. Unified surfaces aggregate across instances: "all my repos" spans a managed Forgejo
  plus a linked GitHub at once.
- **Provisioning is behind a `Provisioner` abstraction**, Docker-first (Podman-friendly,
  behind a scoped socket proxy, never the raw socket on the API), with Kubernetes as a later
  opt-in backend. Requiring a cluster would gut the "runs on a home machine" premise. Managed
  integrations are desired state in Postgres, reconciled on boot.
- **One login, zero re-prompt, for the person and for Elysia.** Elysium extends the
  framework's identity to be an **OIDC provider** so managed services SSO against it (plus
  reverse-proxy forward-auth for zero flicker), and mints **SSH keys and scoped tokens** for
  git, registry, and SMTP transport auth. For linked services Elysium is the OAuth *client*,
  silently refreshing the grant. The token-minting layer serves the AI exactly as it serves
  the user.
- **Mail is a full per-project server**, designed in [docs/mail.md](docs/mail.md). A Postfix
  and Rspamd **MTA edge** receives, filters, signs, and relays, while **Elysium owns the
  canonical mailbox**. One shared SMTP router on port 25 routes inbound by recipient domain
  to the owning project's edge. Bodies live on an `OBJECT_STORE` Drive, metadata in Postgres.
  Elysium generates and verifies MX, SPF, DKIM, and DMARC; reachability and DNS stay the
  operator's responsibility.

# Agents: satellites, threads, and turns

Designed in [docs/satellites.md](docs/satellites.md).

A **satellite** is a Docker container running on hardware you control that drives a Claude
CLI or Codex CLI harness against a git workspace. Its reason to exist is one sentence: Claude
and Codex emit different events with different shapes, and a satellite normalizes both into a
single protobuf contract, so an application is written once and never rewritten when the
harness, model, or provider changes.

- **The browser never talks to a satellite.** Every call is proxied by Elysium. Satellite
  secrets stay server-side, and a satellite on a home network is not reachable from a browser
  anyway.
- **Elysium mirrors, the satellite owns.** A thread's authoritative state lives on its
  satellite. Elysium persists a mirror so the UI can list, search, and audit without fanning
  out, and so a destroyed thread leaves a record. On any disagreement the satellite wins.
- **Budgets are required at thread creation**, with an explicit unlimited value if that is
  genuinely intended. Never silently defaulted.
- **Elysium stores and forwards permissions; it never enforces them.** Egress allowlists,
  exec allowlists, branch protection, and secret redaction are enforced by satellite
  infrastructure the agent cannot reach. A UI that implies Elysium is a second wall is lying
  about the security model.
- **Organizations can provide organization-wide agent subscriptions**, or teams bring their
  own, or each person brings their own. Elysia OAuths to several subscriptions and fails over
  when one dries up.

# Blessed architecture decisions

Detail lives in [docs/](docs/).

- **Framework**: Anubis, consumed from git until it publishes. Elysium was stamped by
  `anubis new` and stays faithful to the stamp, so framework upgrades are version bumps
  rather than merges.
- **Backend**: one Cargo package, `backend/`, an Axum server composing Anubis routers with
  Elysium's own. Application migrations are Elysium's, applied after the framework's.
- **Frontend**: one npm workspace, `frontend/`, a React SPA on `@jalapenolabs/anubis`,
  HeroUI, Tailwind, with rc-dock for the workspace.
- **Two wire contracts, one per boundary** ([docs/transport.md](docs/transport.md)): REST
  with OpenAPI and a generated TypeScript client between the browser and Elysium, which is
  the framework's pipeline; protobuf between Elysium and a satellite, which is the Arsox
  contract. Protobuf on the browser boundary would mean rebuilding the generated client, the
  OpenAPI document, and the drift gate here, which is exactly the divergence rule 1 prevents.
- **Secrets at rest are sealed**, never stored in the clear: satellite bearer secrets,
  provider credentials, and repository credentials go through `anubis::auth::secret_box`.
  They are returned to clients only as redacted display strings.
- **Money is never a float**, anywhere, on any boundary. A fraction of a cent accumulated in
  a float is how a ceiling drifts away from the invoice it was meant to predict.
- **Versions are pinned everywhere.** Rust 1.96.0, Node 24.13.0, Yarn 4.17.0, Postgres 17.6.
  No floating `latest` tags, ever. Both Anubis dependencies pin the same exact revision and
  bumping them is a commit of its own.

# Frontend conventions

Language conventions come from the global engineering docs and are not restated here. The
ones this repository leans on hardest: named exports only, `type` over `interface`, grouped
imports, ky plus SWR for data, react-hook-form plus zod for forms, i18next for every
user-facing string, and English for every log line.

**Shared UI: `@jalapenolabs/uikit`.** Prefer its components, hooks, and utilities over
rebuilding primitives, and contribute sharable UI back to it rather than duplicating it here.
It is consumed from git, so it must be allowlisted in `.yarnrc.yml`.

# Style

No em dashes anywhere in user-facing text (docs, UI, commits, PRs).

# Documentation

`docs/` holds one document per decision category. Keep it current as decisions are made; a
decision that is not in `docs/` and not in this file did not happen.

We document the **current implementation and future plans only**. We do not document history.
Write "today the system does X" and "the roadmap is Y". Never write "we migrated from", "we
used to", or "we implemented X but not Z".

# Testing

Write unit tests for everything on both ends. The only exception is page-level JSX;
everything else gets tests. Integration suites run against a real Postgres, and Playwright
covers the flows end to end.

# Git

**This repo is private today. Open-sourcing it is the roadmap, not the current state.** Write
every commit message, issue, and document as though it were already public, because one day
it is, but do not claim in user-facing text that it is public yet.

`main` is the working branch, and stakeholders push to it directly. There is no `develop`.
Everyone else opens a pull request into `main`.

GitHub issues track individual pieces of work, grouped by milestones. Never create an issue
as an epic; use a milestone for that.

# CI

After every push, monitor the resulting CI run in the background and keep working. Only block
on the run when the push is the end of your task. When working through several commits in a
row, fold any CI failure into the next wave of commits rather than stopping for each one.
Iterate until it is green.
