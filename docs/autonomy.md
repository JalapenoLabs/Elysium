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
- Built: an attribution step, `segment_meeting`, the one meetings and conversations use, puts each arrived item on its
  projects with Elysia's confidence, led by the one she is surest of. An item can sit on several projects, but it has
  one lead project, and only that project's Project Manager may submit a batch for it. A person corrects the projects
  and the lead on the item's page, each change in its history and undoable, and Elysia never overrides anyone else's
  choice. See `docs/action-items.md`, Projects and the lead.
- Built: nothing starts because an issue arrived. An issue reaching Elysium makes an item, its link, and its
  attribution, and none of them wakes a Project Manager or Elysia. A YouTrack issue's own timeline record, which the
  reader makes for every new issue in its projects, reaches Elysia as every ingested record does.
- The lead project's Project Manager works an item one batch at a time.

## Assigning work

A person assigns an action item to its lead project's Project Manager, one at a time or in bulk, and Elysia suggests
which. Assigning wakes the Project Manager to plan. It starts no worker.

## Proposed batches

Built. A Project Manager still plans, orders, and submits everything. Its batches start `proposed`, ahead of
`queued`, unless the project's level (The dial, below) lets work that traces to a person run, and the scheduler
never gives a proposed batch a slot. The person sees each with its goal, repositories, priority, and why, on the
work queue's Proposed tab and the project's own, and approves it, edits those fields and approves it, or rejects it
with a reason. Each project with proposals waiting has one inbox item counting them, resolved once none are left. A
rejection's reason wakes the Project Manager, and it is recorded in the batch's history as the person's feedback for
self-improvement. Only a person decides: Elysia and Project Managers list proposals and approve none.

On the roadmap: a rejection's reason also goes into the project brief (The project brief, below).

## The dial

Built. Each project has one level, shown and set on its page:

| Level                   | Batches that trace to a person | Its writes to trackers |
|-------------------------|--------------------------------|------------------------|
| Approve every batch     | Wait for approval              | Wait for approval      |
| Approve found work only | Run                            | Wait for approval      |
| Full autonomy           | Run                            | Apply at once          |

- **Approve every batch is the default**, where every project starts, those that existed before the dial too. A
  project whose Project Manager wrote to its trackers directly also starts there, since Full autonomy would let its
  batches run as well, which nobody chose; a person turns it up.
- **Only a person changes it.** Setting it takes a company member, which no machine user is, so neither Elysia nor a
  Project Manager can, and a guest cannot. Every change is recorded with who made it and when, and the project's
  page says who set the level it has.
- **No level lets found work queue itself.** Work an agent noticed stays in Findings until a person accepts it, and
  a found batch is proposed at every level. A finding a person accepts is theirs from then on (Findings, below), so
  its batch follows the dial like any work a person asked for.
- **A batch that runs says why.** Its history's submission names the level that let it skip approval.
- **Full autonomy is how a Project Manager writes directly.** Its changes to items and trackers apply as it makes
  them, and each is still a changeset a person can undo. At the other two levels they wait for approval, and so do
  the action items it makes from a meeting's tasks (`docs/meeting-review.md`, Tasks).
- **Turning it applies to what comes next.** The Project Manager's next submission and next change follow the new
  level; batches already proposed or queued stay where they are, for the person to decide on the work queue.

The rules are `crate::models::project_autonomy`. On the roadmap: turning the dial down holds the batches it let queue
that no worker has started, so they wait for the person again.

## Origins and generations

The runaway train is work that files work: a worker notices something, files it, it is queued, and its worker notices
more, until nobody remembers why. The brake is structural.

- Built: every batch has an origin, worked out from its provenance as it is submitted, never asserted.
  **Human-rooted** traces to a person: their message to its Project Manager, an issue assigned to them, a meeting, or
  a coworker's request. **Found** traces only to what an agent noticed.
- Built: every batch has a generation, how many batches led to it: one more than its deepest parent a worker worked
  on, where a batch led to another when the turn that planned it was woken by that batch or it names a record the
  batch made. The limit is a code constant, 3, and a batch past it is refused, telling its Project Manager to leave
  the decision to the person. A person accepting a finding resets it to zero (Findings, below).
- Built: a Project Manager cannot submit a batch with no origin. The work queue and every batch's page show both.
  The exact rules are `docs/engine.md`, Origins and generations.
- On the roadmap: each batch names the goal in the project brief it serves.

## Findings

Built. Agent-found work never queues itself, at any level. Workers and Project Managers report it as a finding with
`worker_report_finding` and `manager_report_finding`: what they saw, where, and what it would cost. Their instructions
say to report such work rather than do it, file it, or submit a batch for it. Each project's Findings tab lists the
open ones with their count for a person to accept or dismiss. Accepting makes an action item as that person, files its
issue in the project's tracker, and submits a batch for it, human-rooted and generation 0 from then on, which waits
for approval or runs as the project's level says (The dial, above). Dismissing is
remembered, with the person's reason when they give one, so the same finding reported again stays dismissed and its
reporter is told why. Findings reach the person as a list they read when they choose, never one item each, and they
never wake Elysia: she reads each project's count in her overview and tells the person when it matters. The exact
rules are `docs/engine.md`, Findings.

## The project brief

Each project has a brief: a page in the knowledge base linked to the project, holding its goals, its decisions, and
what is out of scope. The Project Manager reads it every turn and keeps it current, so what it knows does not live
only in its session, and a person can read one page, correct it, and see every later batch follow. Its folder in the
workspace stays for its working notes.

## What agents know

Built: workers and Project Managers run with `~/.claude` from the configuration repository: the learnings, docs,
manuals, skills, and instructions that do more for their output than anything else here. Neither runs without it. With
none set, one being fetched, or one whose last fetch failed, the pool hands out no slot and Project Managers start no
turn: batches wait queued and wakes wait, each showing the reason, and one inbox item says why until a fetch succeeds.
The repository has a GitHub token of its own, chosen beside it, since it may be private to an account the company's
default token cannot reach. Settings fetches it on every save and shows the commit and when; every worker fetches it
again, leaves out what only another machine can run, and its batch shows the commit it ran on and what was left out.
Coding sessions and Elysia are not held, since a person watches each session and Elysia is who reports the hold. See
`docs/coding.md`, `~/.claude`.

## Staying aware

Every batch shows its why: the meeting quote or issue that called for it, the item, the batch, and its pull request.
Elysia's brief says what shipped, what waits on the person, and what was found. Activity holds everything else.

Built: Elysia's overview counts each project's open findings, which is what her brief reports as found. The brief
itself, what shipped and what waits, is on the roadmap.
