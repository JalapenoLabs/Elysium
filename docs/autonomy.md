# Autonomy

Elysium does the work and its person stays the architect: they set direction, and action items reach them only when
they must, such as a message to send, something irreversible, or a decision about direction. Trust is earned, so
autonomy is a dial a person turns per project, and no setting lets work feed itself.

This doc is the design. What is built says so; the rest is the roadmap, the issues in the Autonomy milestone.

## Issues come in

- Built: every connector that provides issues (Jira, GitHub, YouTrack) reads what is assigned to its person, as one
  capability of `Provides::Issues`, and raises an inbox item for each, linked to its issue. Everything already
  assigned when a connector is added comes in too. The provider stays the source of truth for the issue's fields;
  closing it resolves the item, and being unassigned leaves the item and says so. See `docs/action-items.md`,
  Assigned issues.
- Built: nothing starts because an issue arrived. An issue reaching Elysium only makes an item and its link, in no
  project, and neither wakes a Project Manager or Elysia. A YouTrack issue's own timeline record, which the reader makes
  for every new issue in its projects, reaches Elysia as every ingested record does.
- An attribution step, the one meetings and conversations use, proposes the item's projects with a confidence, and a
  person can correct them. An item can sit on several projects, but it has one lead project, and only that project's
  Project Manager works it, one batch at a time.

## Assigning work

A person assigns an action item to its lead project's Project Manager, one at a time or in bulk, and Elysia suggests
which. Assigning wakes the Project Manager to plan. It starts no worker.

## Proposed batches

Built. A Project Manager still plans, orders, and submits everything. Its batches start `proposed`, ahead of
`queued`, and the scheduler never gives a proposed batch a slot. The person sees each with its goal, repositories,
priority, and why, on the work queue's Proposed tab and the project's own, and approves it, edits those fields and
approves it, or rejects it with a reason. Each project with proposals waiting has one inbox item counting them,
resolved once none are left. A rejection's reason wakes the Project Manager, and it is recorded in the batch's history
as the person's feedback for self-improvement. Only a person decides: Elysia and Project Managers list proposals and
approve none. Until the dial below exists, every batch is proposed.

On the roadmap: a rejection's reason also goes into the project brief (The project brief, below).

## The dial

Each project has one level, changed only by a person and recorded with who changed it:

| Level                   | Batches that trace to a person | Its writes to trackers |
|-------------------------|--------------------------------|------------------------|
| Approve every batch     | Wait for approval              | Wait for approval      |
| Approve found work only | Run                            | Wait for approval      |
| Full autonomy           | Run                            | Apply at once          |

Approve every batch is the default. Full autonomy absorbs `projects.manager_writes_directly`. No level lets found
work queue itself: at every level it stays in Findings until a person accepts it.

## Origins and generations

The runaway train is work that files work: a worker notices something, files it, it is queued, and its worker notices
more, until nobody remembers why. The brake is structural.

- Built: every batch has an origin, worked out from its provenance as it is submitted, never asserted.
  **Human-rooted** traces to a person: their message to its Project Manager, an issue assigned to them, a meeting, or
  a coworker's request. **Found** traces only to what an agent noticed.
- Built: every batch has a generation, how many batches led to it: one more than its deepest parent a worker worked
  on, where a batch led to another when the turn that planned it was woken by that batch or it names a record the
  batch made. The limit is a code constant, 3, and a batch past it is refused, telling its Project Manager to leave
  the decision to the person. A person accepting a finding resets it to zero, the seam Findings use.
- Built: a Project Manager cannot submit a batch with no origin. The work queue and every batch's page show both.
  The exact rules are `docs/engine.md`, Origins and generations.
- On the roadmap: each batch names the goal in the project brief it serves.

## Findings

Agent-found work never queues itself, at any level. Workers and Project Managers report it as a finding: what they
saw, where, and what it would cost. A project's Findings list holds them for a person to accept or dismiss. Accepting
files an issue and makes a proposed batch, human-rooted from then on. Dismissing is remembered, so the same finding is
not raised twice. Findings reach the person as a list they read when they choose, never one item each.

## The project brief

Each project has a brief: a page in the knowledge base linked to the project, holding its goals, its decisions, and
what is out of scope. The Project Manager reads it every turn and keeps it current, so what it knows does not live
only in its session, and a person can read one page, correct it, and see every later batch follow. Its folder in the
workspace stays for its working notes.

## What agents know

Workers and Project Managers run with `~/.claude` from the configuration repository: the learnings, docs, manuals,
skills, and instructions that do more for their output than anything else here. An agent never runs without it. With
none set, or one that cannot be fetched, its turn waits with a reason and an inbox item. The repository has a token of
its own, since it may be private to someone other than the company's default GitHub account, and a batch shows the
configuration commit it ran on.

## Staying aware

Every batch shows its why: the meeting quote or issue that called for it, the item, the batch, and its pull request.
Elysia's brief says what shipped, what waits on the person, and what was found. Activity holds everything else.
