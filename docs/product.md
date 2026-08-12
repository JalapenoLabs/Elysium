# The product

Arsox is a web interface for running coding agents. Claude Code on the web is the reference
experience; this document is about where Arsox deliberately differs, and what that implies for the
screens.

## The thesis

Claude Code on the web is excellent and constrained in ways that follow from it being a hosted
product: the compute is someone else's, the harness is fixed, the agent works alone, and the trust
boundary is a policy rather than a wall.

Arsox inverts each of those, and every one of them is a satellite capability rather than an Arsox
invention. Arsox's job is to make them usable.

| Constraint | What Arsox does instead |
|---|---|
| Hosted compute | Satellites run on your machines, reach only what you allow, and bill against credentials you already hold |
| One harness | Claude CLI or Codex CLI behind one normalized event stream, chosen per thread |
| One agent | Opt into team mode: a commander plus peer agents, each with its own context window and git worktree |
| Policy-based limits | Exec allowlists, an egress proxy, branch protection, secret redaction, and token and cost ceilings enforced by infrastructure the agent cannot reach |
| Failures you infer from silence | Every failure recorded as an incident with a stable code, including the ones that recovered |
| Single-player | Organizations, teams, roles, and invitations, from Anubis |

The last two are the ones users will not ask for and will not want to give up.

## The surfaces

None of this is built. It is the shape the satellite contract implies, written down so the build
order is a decision rather than a drift.

### Satellites

List, register, and inspect. Registration takes an address and a secret, and confirms them by
calling the satellite before saving anything, so a typo fails at the form rather than at the first
job.

A satellite's detail view answers the questions an operator actually has: is it reachable, what
version and proto major does it serve, which harnesses does it offer and what does each support,
how many threads is it running against its concurrency cap, and how much disk is left.

**`GET /v1/harness` is the interesting one.** A harness might have no plan mode or no sub-agents,
and a UI that offers a control the harness cannot honor is worse than one that hides it. Capability
reporting exists so the interface can ask up front instead of inferring from silence three turns in.

### Threads

The core of the product. A thread is a conversation with a workspace attached, and the list is per
team, across every satellite that team owns.

Creating one is where the settings live: which satellite, which harness and model, which repositories
and their setup and checker commands, the agents repo, issue prefetch, permissions, the idle TTL,
and the budget. **The budget is required and never silently defaulted.** The contract makes unlimited
an explicit case rather than a sentinel precisely so an unbounded spend is somebody's decision, and
a form that defaults it away throws that guarantee out.

### The live thread view

Where the work is watched. The event stream renders agent messages, tool calls, team chat, direct
messages between members, integration requests and their outcomes, checker results, and service
logs, in sequence order.

Three things make it more than a log:

- **Questions.** The commander asks between one and five at a time, each with up to five options.
  The whole set is answered at once. Two people may be looking at the same thread, so the interface
  has to handle losing the race: `QUESTION_SET_ALREADY_ANSWERED` means someone else answered, and
  the right response is to show what they said, not to retry.
- **Plans.** Same shape, same race, `PLAN_ALREADY_DECIDED`.
- **Incidents.** Inline, at the point in the stream where they happened, colored by disposition.
  `blocked` reads as the permission system working. `recovered` reads as a warning, because a
  failover nobody sees is a bill nobody explains.

Team mode changes the view rather than the model: members are lanes, and the commander's view is
the conversation between them rather than every tool call any of them made.

### Artifacts and files

Browse the workspace, download artifacts, upload files into a thread. Everything proxied, nothing
mirrored: these are the satellite's and go stale the moment they are copied.

### Usage and incidents

Per model, per thread, per team: total, input, and output tokens, and estimated cost. Money is never
a float anywhere in the contract and must not become one here; a fraction of a cent accumulated in a
float is how a ceiling drifts away from the invoice it was meant to predict.

Incidents get their own queryable surface, filterable by code, disposition, thread, and time range,
because "why did last night's run go wrong" is asked after the thread is gone.

## Build order

Each step is useful on its own, and each depends on the one above it.

1. **Satellite registration.** Blocked on the Rust SDK, which does not exist yet. See
   [satellites.md](satellites.md).
2. **Thread creation and the turn queue.**
3. **The live thread view**, which is the first thing that feels like the product.
4. **Questions and plans**, which is the first thing that makes it usable unattended.
5. **Artifacts, usage, and incidents.**
6. **Team mode**, which is a view change over an event stream that already carries it.

Steps 3 and beyond need realtime fanout to more than one replica, which is blocked on Anubis #32.
Single-replica is fine until it is not, and it will not be obvious when that changes, so it is worth
watching rather than discovering.

## What Arsox will not do

- **Enforce anything the satellite enforces.** Arsox stores and forwards permission settings. It
  does not check them, and the interface must never imply it does. The wall is on the satellite; a
  second wall drawn here would be scenery.
- **Hide incidents.** Filtering happens after they are stored and shown, never before.
- **Put a satellite secret in a browser.** Not in a form response, not in a debug panel, not once.
