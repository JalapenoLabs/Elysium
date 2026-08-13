# Arsox

Arsox is a web interface for running coding agents on hardware you control. Claude Code on the web
is the reference point; Arsox is that experience with the constraints removed. You register
**satellites**, which are docker containers running on your own machines or self-hosted runners, and
Arsox drives them: queues jobs, streams the agents' work back in realtime, answers their questions,
approves their plans, and reports what everything cost.

Three repositories make up the whole:

| Repository | Role | Relationship |
|---|---|---|
| **arsox** (this repo) | The multi-tenant web application: the control plane | Consumer of both |
| [arsox-satellites](https://github.com/JalapenoLabs/arsox-satellites) | The execution plane: satellite images, the protobuf contract, the SDKs | Arsox is an SDK consumer |
| [Anubis](https://github.com/JalapenoLabs/Anubis) | The SaaS framework Arsox is built on | Arsox is an application stamped by `anubis new` |

Arsox is built as a full consumer of Anubis. Almost everything that is the same in every SaaS
product (accounts, teams, roles, invitations, a versioned public API, scaffolding) comes from the
framework, and Arsox contributes back everything the framework turns out to be missing.

# **Source-of-truth policy.**

The USER is the primary source of truth. This `CLAUDE.md` is the SECOND source of truth. The
codebase is **not** a source of truth (it can drift). Whenever a design decision is made or changed,
update this file in the same change. Keep it routinely up to date.

# The Anubis-first rule

This is the single most important working rule in this repository. It is not negotiable and it is
not a suggestion:

> If there is a feature or upgrade that is appropriate to have Anubis host, but Anubis doesn't yet
> have the functionality for it, then it's required to either (a) build it out on Anubis first (if
> it's light and easy to do) via PR or (b) create an issue or find an existing issue on Anubis's
> github repository and consider your own task blocked until the Anubis framework supports your
> needs.

## How to apply it

Ask the question before writing the workaround, not after. The test is ownership, not difficulty:
**would every application built on Anubis want this?** If yes, it belongs to the framework.

Belongs to Anubis:

- Authentication, sessions, MFA, passkeys, OAuth providers
- Organizations, teams, memberships, invitations, roles, permission checks
- The ownership-chain guards and tenancy scoping
- Mail delivery, background jobs, realtime channels, webhooks, billing
- Generated API clients, OpenAPI, scaffolding, the field component library
- Server hardening: shutdown, timeouts, rate limits, security headers, request logging
- Anything a second Anubis application would copy out of this repo verbatim

Belongs to Arsox:

- Satellites: registration, health, credentials, capability negotiation
- Threads, turns, and their queue, as Arsox mirrors them
- Everything specific to driving an agent harness: prompts, plans, questions, budgets, incidents
- The agent-facing UI: the thread view, the event stream, artifacts, usage

If you cannot answer the ownership question confidently, ask the user. Do not resolve the ambiguity
by writing the code here and promising to upstream it later, because that promise is never kept and
the framework quietly stops being the framework.

## Blocked is a real outcome

When (b) applies, the task is blocked and saying so is the correct deliverable. Do not:

- vendor a copy of the framework code into Arsox
- reach around the framework's API into its internals
- ship a "temporary" reimplementation of a framework concern

Do: file or find the Anubis issue, link it from `docs/anubis.md`, finish every part of the task that
is not blocked, and report exactly what was left undone and why.

The live register of what Arsox is waiting on lives in [docs/anubis.md](docs/anubis.md). Keep it
current. It is how the framework learns what its first real consumer needs.

# What Anubis is

Read this section in full before touching anything. Arsox is not a standalone application that
happens to import a library; it is an Anubis application, and the framework's conventions are
Arsox's conventions.

## The vision

Anubis is an open-source SaaS framework: the developer experience of
[Bullet Train](https://bullettrain.co), rebuilt on a Rust backend and a React SPA frontend.

Bullet Train proved two things. First, that the same-in-every-SaaS plumbing (teams, roles, auth, a
versioned public API, webhooks, billing) belongs in a framework rather than being rebuilt per
product. Second, that a great code generator turns domain modeling into the highest-leverage
activity in application development. Anubis brings that experience to teams who want Rust and React
instead of Ruby and ERB.

The framework's central claim is a compile-time one. Everything Bullet Train does at runtime through
Rails reflection, Anubis does at codegen time. After a scaffold runs, `cargo check` and `tsc` prove
the whole stack links end to end: a scaffold either compiles from the database column through to the
React form field, or it tells you exactly what to fix. That guarantee is the reason to accept a
statically typed stack, and it is the promise Rails structurally cannot make.

Anubis is pre-alpha. The architecture is settled and documented; implementation runs through
milestones M1 (foundation) to M5 (ecosystem), mirrored as GitHub milestones. Expect gaps, and expect
to be the one who reports them.

## The stack

- **Backend**: Rust on tokio, Axum + tower + hyper. World-class idiomatic Rust is the stated bar.
- **ORM**: Diesel + diesel-async, chosen for maximum compile-time typing. No stringly SQL, no
  runtime query surprises.
- **Datastores**: PostgreSQL required, and it is the system of record for everything including
  sessions and the durable job queue. Redis is optional and is only ever a cache and realtime
  pub/sub fanout, never the system of record.
- **Frontend**: React SPA (client-side rendered) on Vite + SWC, TypeScript, HeroUI (pinned to the
  v2.8 line), TailwindCSS themed by the Jalapeno Labs Brand package, Redux Toolkit, React Router
  behind a central `UrlTree`, ky + SWR for data, react-hook-form + zod for forms, i18next for
  copy, Vitest for units and Playwright for E2E.
- **Contract**: REST at `/api/v1`, OpenAPI 3.1 generated from the Rust handlers via utoipa, driving
  a generated and fully typed TypeScript client. No protobuf on this boundary.
- **Codegen**: Bullet Train's philosophy 1:1. Living templates (real compiling code, not a DSL),
  magic anchor comments, and generated code that the application owns outright.
- **Packaging**: one Cargo package (`anubis/`, library plus the `anubis` CLI binary), one npm
  package (`frontend/`, `@jalapenolabs/anubis`), one thin starter template. No crate sprawl.

## Tenancy, the model Arsox inherits

```
User ─< OrganizationMembership >─ Organization
User ─< TeamMembership >─ Team ─> Organization
```

- **User**: a person who can log in. Owns credentials, profile, preferences, and nothing
  domain-related.
- **Organization**: the top-level tenant. Owns teams, billing, and org-wide settings.
- **Team**: the working tenant. Every domain resource chains its ownership back to a team.
- **TeamMembership**: joins a user to a team and carries team-level roles. Resources are assigned to
  memberships, never to users directly, so work can be assigned to an invited person who has not
  signed up yet and assignments survive someone leaving.
- **Invitation**: an emailed 256-bit token, hashed at rest, 14-day expiry.

At signup every user gets a personal organization containing a default team, so solo use requires
zero tenancy ceremony.

Enforcement is extractor-based, not convention-based. A handler that takes `anubis::guard::TeamMember`
gets authentication (401), the route's `{team_id}` resolved against the caller's membership (404 for
non-members, byte-identical to a nonexistent id so probing reveals nothing), and permission checks
(403) before its body runs. Tenant isolation is enforced by walking the ownership chain, never by
trusting a client-supplied id.

**In Arsox this means a Satellite belongs to a Team, and everything downstream of a satellite
inherits that ownership.** There is no other tenancy model available and there should not be.

## Roles and permissions

Roles are declared once, in `config/roles.yml`, with inheritance and per-model action grants
(`read`, `create`, `update`, `destroy`, or `manage` as shorthand). One definition drives both sides:

1. The backend embeds the file at compile time and resolves it at boot through
   `anubis::roles::RoleSet`, which rejects unknown includes, inheritance cycles, and unknown actions
   before the server takes traffic.
2. `anubis roles generate-ts` emits `roles.generated.ts`, which the SPA uses to hide or disable
   controls the member cannot use. CI regenerates it and fails on drift.

Never hand-edit `roles.generated.ts`. Never express a permission in Arsox any other way.

## Scaffolding, and the living templates

`anubis scaffold model <Model> <ParentChain> <field:type ...>` produces, in one command: a Diesel
migration, the `schema.rs` update, the model struct with its ownership chain and `valid_*` scoping
methods, the `roles.yml` grant, account CRUD handlers, routes, integration tests, list/show/form
React pages built from the field component library, the navigation entry, breadcrumbs, the per-model
i18next locale file, and Vitest coverage. `anubis scaffold field` propagates a new attribute through
every one of those artifacts, which is the feature that makes the framework compound.

Two mechanisms carry all of it:

- **Magic anchor comments** (`// 🐺 anubis:routes`, `{/* 🐺 anubis:nav */}`) mark insertion points in
  files the application owns. **Never delete an anchor.** Deleting one silently breaks every future
  scaffold against that file.
- **Living templates**: `backend/src/scaffolding/` and its frontend counterparts are real, compiling,
  CI-tested code that the generator reads and transforms. They are named
  `absolutely_abstract::CreativeConcept` and `completely_concrete::TangibleThing` for the same reason
  Bullet Train names them that way: enough namespacing fidelity to transform into any real-world
  parent/child combination.

**The living templates in this repository are generator input, not Arsox demo code, and must not be
deleted.** They look like leftover example CRUD. They are not. `anubis scaffold model` reads
`backend/src/scaffolding/` at runtime, so removing them breaks the scaffolder for this application.
Treat them as vendored framework infrastructure that happens to live in our tree.

The workflow Bullet Train teaches and Anubis inherits: write the scaffold commands into a scratch
file, review them with people before running them, run them, commit the generated code in its own
commit, then polish. Re-scaffolding is cheap; living with a wrong domain model is not.

## The contract pipeline

The API contract flows one way, Rust to TypeScript: handlers register with utoipa, producing an
OpenAPI 3.1 document; `anubis client generate-ts` renders it as typed ky route functions; the SPA
consumes them through SWR hooks. CI regenerates and fails on drift, so the client can never lag the
API.

Today this pipeline covers the framework's own `/api/v1` surface only. Application endpoints are
not yet mergeable into it (Anubis #58, and A3 of Anubis #57), so Arsox's own routes are hand-written
ky functions under `frontend/src/api/routes/`, in the same shape the scaffolder emits. That is a
tracked gap, not a house style.

## What the framework does not do yet

Verified against Anubis `develop`. Each one is a live constraint on Arsox, and each is either filed
or referenced in [docs/anubis.md](docs/anubis.md):

- **No SMTP mailer.** Only `Mailer::log` and `Mailer::test` exist, so verification, reset,
  sign-in-code, and invitation emails print to the console. Production sign-up is not possible
  until this lands (Anubis #50).
- **No realtime channel layer.** There is no framework pub/sub, so cross-process fanout of satellite
  events has no home (Anubis #32). Arsox runs a single-process in-memory broadcast until it does.
- **The backend does not serve the built SPA.** A production deployment needs its own static host
  (Anubis #49).
- **No production hardening.** No graceful shutdown, request timeouts, request-id logging, rate
  limiting, or security headers (Anubis #52, #53, #45).
- **Neither package is published.** The crate installs from git and `@jalapenolabs/anubis` installs
  from git via `#workspace=`, which is why `.yarnrc.yml` allowlists the repository (Anubis #56, and
  D1 of Anubis #57). Both are pinned to an exact revision here, never to a branch.

Two further gaps stopped Arsox building at all, and both were light enough to fix rather than file,
which is case (a) of the rule above in practice. Anubis PR #66 made the repository fetchable by
cargo and PR #67 made the npm package installable from git; both are merged, and the pinned revision
is develop at the second of them. [docs/anubis.md](docs/anubis.md) has the detail.

# What Arsox Satellites are

The execution plane. Read [arsox-satellites](https://github.com/JalapenoLabs/arsox-satellites) in
full before writing anything that talks to one; this is the summary, not the contract.

A satellite is a docker container (`jalapenolabs/arsox-satellite:ubuntu-1.0.0` and Fedora and Rocky
variants) that runs a Rust API and drives a Claude CLI or Codex CLI harness against a git workspace.
Its reason to exist is one sentence: **Claude and Codex emit different events with different shapes,
and a satellite normalizes both into a single protobuf contract, so an application is written once
and never rewritten when the harness, model, or provider changes.**

What Arsox needs to know:

- **Protobuf is the wire format in both directions**, over ordinary HTTP plus a unidirectional
  WebSocket for events. A JSON rendering of the same contract exists for hand-driven debugging only.
- **`ARSOX_SECRET` is a bearer token** the caller presents. The satellite fails to start without it.
  It never reaches an agent's environment, and it must never reach a browser.
- **Threads are the unit of conversation.** IDs are UUIDv7 generated by the satellite, never by the
  client, because they become filesystem paths. A thread holds a workspace, cloned repos, prefetched
  issues, and an assembled `AGENTS.md`.
- **One turn at a time per thread, strictly FIFO.** Turns queue rather than run in parallel. Threads
  run concurrently with each other up to `ARSOX_MAX_CONCURRENT_THREADS`.
- **Any process holding the URL, the secret, and the thread ID can attach.** There is no lease and
  no ownership, which is what makes a horizontally scaled host application possible. Persist the
  thread ID and any replica can pick the work back up.
- **Every event carries a monotonic `sequence` and is persisted before it is sent.** Reconnect with
  `?from_sequence=<n>` and a network blip costs nothing. A consumer that lags is closed loudly with
  `STREAM_CONSUMER_LAGGED` rather than starved quietly.
- **Budgets are required at thread creation**, with an explicit unlimited value if that is genuinely
  intended. Enforcement is by an LLM proxy that counts tokens, not by prompt.
- **Permissions are deterministic.** Egress allowlist, exec allowlist, push and branch protection,
  filesystem scoping, and secret redaction are enforced by infrastructure the agent cannot reach.
  Instructions in `AGENTS.md` are advisory and always will be.
- **Nothing fails silently.** Every failure at every severity is recorded as an **incident** with a
  stable code and a disposition (`fatal`, `recovered`, `degraded`, `blocked`), emitted on the stream
  and queryable afterward. Incidents outlive their thread. A code path that swallows a failure is a
  bug.
- **Errors are one shape everywhere**: a stable enum code, a message, a `retryable` flag, and
  structured details. Match on the code, never on the message, and fall back to `retryable` for
  codes a client has never heard of.

**Current state: the contract exists, the SDKs do not.** The `arsox-satellites` repository today
carries `proto/` and the committed `gen/` output for TypeScript, Rust, and Python, plus its docs.
The satellite image, the Rust SDK, the Node SDK, and the Python SDK are not built yet. Arsox
therefore has no client to call, which bounds how far the backend can go. See
[docs/satellites.md](docs/satellites.md) for exactly what Arsox needs from that repository and in
what order.

# Blessed architecture decisions

These are decided and current. Detail lives in [docs/](docs/).

- **Framework**: Anubis, consumed from git until it publishes. Arsox was stamped by `anubis new` and
  stays faithful to the stamp so framework upgrades are version bumps rather than merges.
- **Backend**: one Cargo package, `backend/`, an Axum server composing Anubis routers with Arsox's
  own `/account` router. Application migrations are Arsox's, applied by `db::run_app_migrations`
  after the framework's.
- **Frontend**: one npm workspace, `frontend/`, a React SPA on `@jalapenolabs/anubis`, HeroUI, and
  Tailwind.
- **Tenancy**: a Satellite belongs to a Team. Threads belong to a Satellite. Nothing in Arsox is
  owned by a User directly.
- **The browser never talks to a satellite.** Every call is proxied by the Arsox backend. Satellite
  secrets stay server-side, and a satellite on a home network is not reachable from a browser
  anyway.
- **Secrets at rest are sealed**, never stored in the clear: satellite bearer secrets, LLM
  credentials, and repository credentials go through `anubis::auth::secret_box` under
  `ANUBIS_SECRET_KEY`. They are returned to clients only as redacted display strings, following the
  satellites contract's `Secret` discipline (`value` goes up, `display` comes back).
- **Arsox mirrors, the satellite owns.** A thread's authoritative state lives on its satellite.
  Arsox persists a mirror indexed by team so the UI can list, search, and audit without fanning out
  to every satellite, and so a destroyed thread leaves a record behind. On any disagreement the
  satellite wins.
- **Realtime**: satellite to Arsox over the satellites WebSocket, Arsox to browser over its own
  stream. Multi-replica fanout is blocked on Anubis #32.
- **Versions are pinned everywhere.** Rust 1.96.0, Node 24.13.0, Yarn 4.17.0, Postgres 17.6. No
  floating `latest` tags, ever.

# Style

No em dashes anywhere in user-facing text (docs, UI, commits, PRs).

Language conventions come from the global engineering docs and are not restated here. The ones this
repository leans on hardest: named exports only, `type` over `interface`, grouped imports, ky plus
SWR for data, react-hook-form plus zod for forms, i18next for every user-facing string, and English
for every log line.

# Documentation

`docs/` holds one document per decision category. Keep it current as decisions are made; a decision
that is not in `docs/` and not in this file did not happen.

We document the **current implementation and future plans only**. We do not document history. Write
"today the system does X" and "the roadmap is Y". Never write "we migrated from", "we used to", or
"we implemented X but not Z".

# Git

This repo is open-source, public.

`main` is production (stable). `develop` is the working branch (unstable) that pull requests target.
Stakeholders (such as `navarrotech`) may push straight to `develop`; everyone else opens a pull
request. Stakeholders are responsible for promoting `develop` to `main`.

GitHub issues track individual pieces of work, grouped by milestones. Never create an issue as an
epic; use a milestone for that.
