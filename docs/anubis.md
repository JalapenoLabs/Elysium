# Anubis

Elysium is an [Anubis](https://github.com/JalapenoLabs/Anubis) application, stamped by
`anubis new` and kept faithful to that stamp. This document covers how Elysium consumes the
framework, and holds the live register of what Elysium is waiting on.

The rule that governs all of it is the first section of [CLAUDE.md](../CLAUDE.md). In short:
if a capability belongs to the framework, it gets built in the framework or filed against the
framework, and the Elysium task is blocked until it lands. It never gets worked around here.

## How Elysium consumes the framework

Neither Anubis package is published yet, so both install from git and both pin an exact
revision rather than a branch:

| Consumer | Declaration | Pinned in |
|---|---|---|
| Backend | `anubis = { package = "anubis-framework", git = "...", rev = "..." }` | `backend/Cargo.toml` |
| Frontend | `@jalapenolabs/anubis` at `#commit=...&workspace=@jalapenolabs/anubis` | `frontend/package.json` |

The crate publishes as `anubis-framework`, because an unrelated crate holds the plain name on
crates.io. Cargo's `package` key renames it back, so every `use anubis::` reads the way the
framework's own docs write it.

`.yarnrc.yml` allowlists the Anubis and Jalapeno Labs repositories, because Yarn Berry
refuses git dependencies otherwise.

**Both pins are the same revision, and bumping them is its own commit.** A floating branch
means the framework changes underneath two builds that were believed identical, which is the
hardest class of bug to notice. When the crate publishes ([#81](https://github.com/JalapenoLabs/Anubis/issues/81))
and stamped applications move to published versions ([#82](https://github.com/JalapenoLabs/Anubis/issues/82)),
both become ordinary version ranges and this section gets simpler.

## What Elysium takes from the framework

Everything in this list is the framework's, and reimplementing any of it here is a bug:

- Registration, sign-in, sessions, password reset, email verification, emailed sign-in codes
- TOTP second factor and passkeys
- OAuth and OIDC sign-in, where Anubis is the relying party
- Organizations, teams, memberships, invitations
- Roles and permissions, compiled from `config/roles.yml` into both a Rust authorization
  module and `frontend/src/roles.generated.ts`
- Ownership-chain guards: `TeamMember`, `OrganizationMember`, `CurrentUser`, at three levels
  of resource nesting
- Platform applications, bearer tokens, and the versioned `/api/v1` surface, including
  application models
- Encryption at rest, via `anubis::auth::secret_box`
- Transactional email, with log, test, and SMTP backends and optional DKIM signing
- Durable background jobs, whose enqueue joins the caller's transaction so a job commits with
  the domain write that caused it
- Realtime channels, in process by default and over Redis across instances
- The notification inbox, the audit log, and webhooks in both directions
- Billing, from `config/billing.yml` through Stripe Checkout to plan limits
- Server hardening: probes, request ids, request logging, security headers, CORS,
  compression, per-request timeouts, a bounded drain on `SIGTERM`, and auth rate limits
- The app shell, auth pages, settings screens, tenancy management screens, and the field
  component library in `@jalapenolabs/anubis`
- Full-stack scaffolding, via the `anubis` CLI

## The living templates

`backend/src/scaffolding/` and its frontend counterparts (`CreativeConceptsPage`,
`TangibleThingForm`, `creativeConceptRoutes.ts`, and the rest) are **Anubis living
templates**, not Elysium demo code.

`anubis scaffold model` reads them out of this repository at runtime and transforms them into
the model being generated. Delete them and the scaffolder stops working for this application.
They are compiling, CI-tested code on purpose, so the templates can never rot.

The same goes for magic anchor comments (`// 🐺 anubis:routes`, `{/* 🐺 anubis:nav */}`). They
mark insertion points in files Elysium owns and edits freely. Removing one silently breaks
every future scaffold against that file.

## The blocked register

Each row is a capability Elysium needs, owned by the framework, with the issue it waits on.
Keep this current: it is how the framework learns what its first real consumer needs.

| Needs | Blocks | Anubis issue |
|---|---|---|
| The sub-tenant tier's scaffolder templates and management screens | Generating a model owned by a Project, and the screens that create one and manage its roster. The backend foundation has landed; these are the rest of [#93](https://github.com/JalapenoLabs/Anubis/issues/93) | [#93](https://github.com/JalapenoLabs/Anubis/issues/93) |
| Multi-profile sessions, with a switcher | Signing several accounts into one browser and switching without signing out. The taskbar's profile badge is a stub until it lands | [#94](https://github.com/JalapenoLabs/Anubis/issues/94) |
| Per-organization authentication policy | An organization tightening allowed methods, mandating a second factor, or setting session lifetime for its own members | [#95](https://github.com/JalapenoLabs/Anubis/issues/95) |
| Presence and person-set status | The roster, the action center, and every "who is around" surface | [#96](https://github.com/JalapenoLabs/Anubis/issues/96) |
| Registration modes: open, invite-only, domain allowlist | Elysium is invite-only by design, and the framework registers openly | [#77](https://github.com/JalapenoLabs/Anubis/issues/77) |
| First-administrator bootstrap for a fresh deployment | Claiming a freshly self-hosted instance as its root user | [#79](https://github.com/JalapenoLabs/Anubis/issues/79) |
| Shared-organization mode for the signup bootstrap | A self-hosted deployment where everyone joins one organization rather than getting a personal one | [#78](https://github.com/JalapenoLabs/Anubis/issues/78) |
| Account disable and forced password rotation | Offboarding a member without deleting their history | [#80](https://github.com/JalapenoLabs/Anubis/issues/80) |
| Theme system: system, dark, and light, with consumer themes | The design language in [ux.md](ux.md) needs a theme engine to live in | [#88](https://github.com/JalapenoLabs/Anubis/issues/88) |
| Mobile navigation for the app shell | The framework's screens on a phone | [#86](https://github.com/JalapenoLabs/Anubis/issues/86) |
| Published crate and npm package | Ordinary version ranges instead of pinned revisions | [#81](https://github.com/JalapenoLabs/Anubis/issues/81), [#82](https://github.com/JalapenoLabs/Anubis/issues/82) |

[Anubis #57](https://github.com/JalapenoLabs/Anubis/issues/57) is the broader consumer gap
register and is worth reading before adding a row here.

## Contributed back

Defects Elysium found in the framework and fixed there rather than working around here. This
is case (a) of the Anubis-first rule in practice, and the record of it.

| Change | What it fixes |
|---|---|
| [Anubis #99](https://github.com/JalapenoLabs/Anubis/pull/99) | A stamped application's Tailwind scanned neither HeroUI's theme nor the framework package, so none of HeroUI's utilities were generated and every framework component rendered unstyled. The template's glob is correct where it lives and wrong once `starter/` is dropped |
| [Anubis #100](https://github.com/JalapenoLabs/Anubis/pull/100) | `anubis new` stamped a tree that fails `cargo fmt --check`, because import order is written for the template's own crate name. It now formats what it stamps, as `scaffold` already did ([#97](https://github.com/JalapenoLabs/Anubis/issues/97)) |
| [Anubis #103](https://github.com/JalapenoLabs/Anubis/pull/103) | `.env.example` named `localhost`, which resolves to `::1` first while Docker publishes IPv4 only, so every pool connection paid a full TCP connect timeout. It presents as a flaky frontend rather than an error. The same change split Playwright's `install-deps` from `install`, which is why E2E now runs on the self-hosted fleet at all ([#85](https://github.com/JalapenoLabs/Anubis/issues/85)) |
| [Anubis #104](https://github.com/JalapenoLabs/Anubis/pull/104) | The rate-limit test spent exactly its quota and assumed nothing replenished, which is false once the burst outlasts one emission interval. It failed on three unrelated branches ([#102](https://github.com/JalapenoLabs/Anubis/issues/102)) |
| [Anubis #101](https://github.com/JalapenoLabs/Anubis/pull/101) | The sub-tenant tier's backend foundation: the migration, the model, one access resolver, the `SubTenantMember` guard, and role scoping. Additive, so an ownership chain ending in `Team` is untouched |
| [jalapenolabs/cli #4](https://github.com/JalapenoLabs/cli/pull/4) | The shared ESLint package shipped a Yarn Classic lockfile and named no package manager, so Yarn 4 handed the pack's install to Yarn 1 and it failed nondeterministically on a clean runner. Every stamped application inherited it |

Two of these are the same defect in different clothes: a template that is correct in its own
position and wrong after transformation. #100 is import order, #99 is a relative path. Both
are silent, and both reach every application the framework generates, which is the argument
for the scaffold proof running against a stamped tree rather than only against `starter/`.

Three others share a different shape: a test or a default that holds on a fast, idle,
IPv4-only machine and breaks anywhere else. None of them were caught by CI, because the job
that would have caught them could not run: Playwright's browser never installed on the
self-hosted runners, so the end-to-end suite had never once executed there. A gate that
cannot run is worse than no gate, because everyone believes it is watching.

## What Elysium does not take

**Protobuf on the browser boundary.** Elysium's original design put protobuf between the
frontend and the API. Anubis generates its OpenAPI document from the Rust handlers and
renders a typed TypeScript client from it, drift-gated in CI, and every scaffolded model
arrives already typed on both ends. Replacing that with protobuf would mean rebuilding the
client, the document, and the gate here, which is the divergence the Anubis-first rule
prevents. The framework's contract wins on that boundary; protobuf stays on the satellite
boundary, where it is the Arsox contract rather than a choice. See
[transport.md](transport.md).

**Redux Toolkit, for now.** Anubis prescribes it. Elysium has no store yet because it has no
cross-page state worth one; the dock layout and the Elysia event stream are the first things
that will plausibly need one. Adding it is a decision to record here when it happens, not a
default to adopt in advance.
