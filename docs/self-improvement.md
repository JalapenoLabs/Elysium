# Self-improvement

Elysium grows without being updated. It learns how its people like things done and changes what it produces next
time, and when a provider changes underneath a connector, it repairs the connector itself. Behavior lives in
documents Elysium rewrites, and the fragile part of every connector is code Elysium can fix in place.

This doc is the design. None of it is built; the roadmap is the issues in the Self-improvement milestone. One kind
of correction is already kept where its feedback will be read: a person rejecting a proposed work batch, whose
`rejected` entry in the batch's history (`work_batch_events`) holds their reason, names them, and names the batch and
the Project Manager's turn that proposed it (`docs/engine.md`, Proposed batches).

## Learning from feedback

- People correct Elysium all the time without saying so: editing a drafted message before sending it, rejecting a
  changeset or one of its operations, rejecting a proposed work batch, fixing a project attribution, approving or
  rejecting a transcript correction, telling Elysia "don't do that". Each is recorded as feedback with what it was
  about.
- Agents' standing instructions (Elysia's, the Project Managers', triage's, correction's, segmenting's) are versioned
  documents in the database, seeded from the repository, with history and undo.
- A scheduled engine workflow distills feedback into changes: to the person's memory and to shared learnings directly,
  with history; to the company doc and to agents' instructions as proposals the person approves, each with the
  feedback that led to it.

## Self-healing connectors

- Each connector lives in one folder, `api/src/connectors/<kind>/`: its definition, client, reader, adapter, page
  script, fixtures, checks, tests, docs, and a `CLAUDE.md`.
- A connector's `CLAUDE.md` is written for the agent that next has to fix it: the provider's quirks, how the
  connector reads it and why, and every past breakage with its cause and fix. It is the one place this project
  records history on purpose, because a connector's past failures are the best guide to its next one. A healing
  worker reads it first and adds what it learned, whether it fixed the adapter or gave up.
- A connector has a core and an adapter. The core, in Rust and never changed by an agent, holds sign-in, sealed
  secrets, allowlists, and rate limits. The adapter is the part a provider breaks: request building, response
  parsing, and page scripts.
- Adapters are scripts run in an embedded sandbox. An adapter reaches only its connector's own hosts, through a fetch
  Elysium provides, and never sees a credential; time and memory are bounded. Browser connectors' page scripts already
  run this way, inside the page.
- An instance can hold its own patched copy of an adapter, stored as data rather than in the checkout, with its diff,
  who made it, and the upstream version it replaces. It can be reverted at any time, and is retired when an upstream
  release changes that adapter.
- An unexpected failure (a rejected request, a response that no longer parses, a missing field), as distinct from
  expired credentials, a rate limit, or an unreachable network, starts a healing run. A worker gets the connector's
  folder and the failing exchange with secrets removed. Either it fixes the adapter, proven by the connector's checks
  and tests, and the patched copy takes over at once; or it gives up with a reason, which becomes an action item.
- When the instance allows it, the worker also opens a pull request upstream with the fix.
