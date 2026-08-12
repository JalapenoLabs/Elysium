# Anubis

Arsox is an [Anubis](https://github.com/JalapenoLabs/Anubis) application, stamped by `anubis new` and
kept faithful to that stamp. This document covers how Arsox consumes the framework, and holds the
live register of what Arsox is waiting on.

The rule that governs all of it is the first section of [CLAUDE.md](../CLAUDE.md). In short: if a
capability belongs to the framework, it gets built in the framework or filed against the framework,
and the Arsox task is blocked until it lands. It never gets worked around here.

## How Arsox consumes the framework

Neither Anubis package is published yet, so both install from git and both are pinned to an exact
revision rather than a branch:

| Consumer | Declaration | Pinned in |
|---|---|---|
| Backend | `anubis = { git = "...", rev = "..." }` | `backend/Cargo.toml` |
| Frontend | `@jalapenolabs/anubis` at `#commit=...&workspace=@jalapenolabs/anubis` | `frontend/package.json` |

`.yarnrc.yml` allowlists the Anubis and Jalapeno Labs CLI repositories, because Yarn Berry refuses
git dependencies otherwise.

**Both pins are the same revision, and bumping them is its own commit.** A floating branch means the
framework changes underneath two builds that were believed identical, which is the hardest class of
bug to notice. When the crate publishes to crates.io (Anubis #56) and the package publishes to npm
(D1 of Anubis #57), both become ordinary version ranges and this section gets simpler.

## What Arsox takes from the framework

Everything in this list is the framework's, and reimplementing any of it here is a bug:

- Registration, sign-in, sessions, password reset, email verification, emailed sign-in codes
- TOTP second factor and passkeys
- Organizations, teams, memberships, invitations
- Roles and permissions, compiled from `config/roles.yml` into both a Rust authorization module and
  `frontend/src/roles.generated.ts`
- Ownership-chain guards: `TeamMember`, `OrganizationMember`, `CurrentUser`
- Platform applications, bearer tokens, and the versioned `/api/v1` surface
- Encryption at rest, via `anubis::auth::secret_box`
- The app shell, auth pages, and field component library in `@jalapenolabs/anubis`
- Full-stack scaffolding, via the `anubis` CLI

## The living templates

`backend/src/scaffolding/` and its frontend counterparts (`CreativeConceptsPage`,
`TangibleThingForm`, `creativeConceptRoutes.ts`, and the rest) are **Anubis living templates**, not
Arsox demo code.

`anubis scaffold model` reads them out of this repository at runtime and transforms them into the
model being generated. Delete them and the scaffolder stops working for this application. They are
compiling, CI-tested code on purpose, so the templates can never rot.

The same goes for magic anchor comments (`// 🐺 anubis:routes`, `{/* 🐺 anubis:nav */}`). They mark
insertion points in files Arsox owns and edits freely. Removing one silently breaks every future
scaffold against that file.

## Contributions made

Two blockers were light enough to fix rather than file, which is case (a) of the rule. Both are
merged, and the pinned revision above is `develop` at the second of them.

| Pull request | What it fixes |
|---|---|
| [Anubis #66](https://github.com/JalapenoLabs/Anubis/pull/66) | A `bullet_train/bullet_train` gitlink with no `.gitmodules`, which made `cargo` refuse to fetch the repository. No cargo consumer could depend on Anubis at all |
| [Anubis #67](https://github.com/JalapenoLabs/Anubis/pull/67) | `@jalapenolabs/anubis` installing from git as a lone `package.json`, because `files` is `["dist"]` and nothing built `dist`. Every stamped application failed its first typecheck |

They are recorded here rather than dropped because they are the reference for how the Anubis-first
rule gets applied: the gap was found here, fixed there, and consumed back through an exact pin.

## The blocked register

Each row is a capability Arsox needs, owned by the framework, with the issue it waits on. Keep this
current: it is how the framework learns what its first real consumer needs.

| Needs | Blocks | Anubis issue |
|---|---|---|
| Realtime channel layer for cross-process fanout | Running more than one Arsox replica. Satellite events reach only the replica holding the socket | [#32](https://github.com/JalapenoLabs/Anubis/issues/32) |
| SMTP mailer backend | Production sign-up, invitations, password reset. Mail only prints to the log today | [#50](https://github.com/JalapenoLabs/Anubis/issues/50) |
| Backend serves the built SPA | Any single-origin production deployment. Emailed links 404 without it | [#49](https://github.com/JalapenoLabs/Anubis/issues/49) |
| Background job queue | Satellite health reconciliation, mirror repair, usage rollups | [#27](https://github.com/JalapenoLabs/Anubis/issues/27) |
| Graceful shutdown, request timeouts, request-id logging | Deploying without dropping in-flight turns | [#52](https://github.com/JalapenoLabs/Anubis/issues/52) |
| Security headers and an explicit CORS posture | Production exposure | [#53](https://github.com/JalapenoLabs/Anubis/issues/53) |
| Auth endpoint rate limiting | Production exposure | [#45](https://github.com/JalapenoLabs/Anubis/issues/45) |
| Tenancy management endpoints (create and rename teams, change roles, remove members) | Team administration beyond invitations | [#51](https://github.com/JalapenoLabs/Anubis/issues/51) |
| Application paths in the generated TypeScript client | Arsox route functions are hand-written rather than generated | [#58](https://github.com/JalapenoLabs/Anubis/issues/58), A3 of [#57](https://github.com/JalapenoLabs/Anubis/issues/57) |
| Published crate and npm package | Ordinary version ranges instead of pinned revisions | [#56](https://github.com/JalapenoLabs/Anubis/issues/56), D1 of [#57](https://github.com/JalapenoLabs/Anubis/issues/57) |
| `anubis new` stamps CI and environment-driven development config | Every stamped app rebuilds both by hand, as Arsox did | [#69](https://github.com/JalapenoLabs/Anubis/issues/69) |

[Anubis #57](https://github.com/JalapenoLabs/Anubis/issues/57) is the broader consumer gap register
and is worth reading before adding a row here; several of these appear there with more context.

## What Arsox does not take

Anubis prescribes Redux Toolkit alongside the rest of its frontend stack. Arsox has no store yet
because it has no cross-page state worth one; the satellite event stream is the first thing that
will plausibly need it. Adding one is a decision to record here when it happens, not a default to
adopt in advance.

Arsox's own API route functions are hand-written under `frontend/src/api/routes/`, in the shape the
scaffolder emits, because the generated client cannot yet carry application paths. That is the
tracked gap above, not a house style, and those files should be deleted in favor of generated ones
when it closes.
