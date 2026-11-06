# Autonomy

Elysium does the work and its person stays the architect: they set direction, and action items reach them only when
they must, such as a message to send, something irreversible, or a decision about direction. Trust is earned, so
autonomy is a dial a person turns per project, and no setting lets work feed itself.

This doc is the design. None of it is built yet; the roadmap is the issues in the Autonomy milestone.

## Issues come in

- Jira and GitHub connectors read the issues assigned to their person, as YouTrack's does, and raise an inbox item
  for each, linked to its issue. Everything already assigned when a connector is added comes in too. The provider
  stays the source of truth for the issue's fields, and closing it resolves the item as a link does today.
- Nothing starts because an issue arrived. An issue reaching Elysium only makes an item.
- An attribution step, the one meetings and conversations use, proposes the item's projects with a confidence, and a
  person can correct them. An item can sit on several projects, but it has one lead project, and only that project's
  Project Manager works it, one batch at a time.

## Assigning work

A person assigns an action item to its lead project's Project Manager, one at a time or in bulk, and Elysia suggests
which. Assigning wakes the Project Manager to plan. It starts no worker.

## Proposed batches

A Project Manager still plans, orders, and queues everything. Its batches start `proposed`, ahead of `queued`, and the
scheduler never gives a proposed batch a slot. The person sees each with its goal, repositories, priority, and why,
and approves it, edits those fields and approves it, or rejects it with a reason. A rejection's reason wakes the
Project Manager and goes into the project brief, and it is recorded as feedback for self-improvement.

## The dial

Each project has one level, changed only by a person and recorded with who changed it:

| Level                   | What needs approval                                    |
|-------------------------|--------------------------------------------------------|
| Approve every batch     | Every batch. The default.                              |
| Approve found work only | Nothing human-rooted; found work stays in Findings     |
| Full autonomy           | Nothing human-rooted; found work stays in Findings     |

The second level approves work that traces to a person. Full autonomy adds what those levels do not cover, such as a
Project Manager's writes to trackers, which `projects.manager_writes_directly` governs today. No level lets found
work queue itself.

## Origins and generations

The runaway train is work that files work: a worker notices something, files it, it is queued, and its worker notices
more, until nobody remembers why. The brake is structural.

- Every batch has an origin, worked out from its provenance. **Human-rooted** traces to a person, an assigned issue, a
  meeting, or a coworker's request. **Found** is something an agent noticed.
- Every batch has a generation: how many batches led to it. A person's accepting a finding resets it to zero. The
  limit is a code constant, and a batch past it is refused.
- A Project Manager cannot submit a batch with no origin, and each batch names the goal in the project brief it
  serves.

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

## Staying aware

Every batch shows its why: the meeting quote or issue that called for it, the item, the batch, and its pull request.
Elysia's brief says what shipped, what waits on the person, and what was found. Activity holds everything else.
