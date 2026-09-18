# Action items

Action items are the one list of everything the user owes attention to, wherever it came from: a Jira issue, a
GitHub issue or pull request, an inbound email, an ask from a meeting, or something typed by hand. Initiatives group
action items toward a goal that ends, and carry the progress bar. Projects are long lived and group both.

The core is built: items, initiatives, their projects and memberships, comments, history, Next, and progress, served
under `/api/v1/action-items` and `/api/v1/initiatives` (`docs/api.md`) and kept current on the event stream
(`docs/realtime.md`). Links, the watcher, changesets, the `elysium_work` tools, and the frontend are still a design;
the sections about them are the plan, and each stops being one as it lands.

The design goal is that work arrives from anywhere, is triaged and managed in one place, and every change made here
reaches the system it came from. The user approves; Elysium and Elysia, Elysium's AI assistant, do the bookkeeping.

## Terms

| Term        | Lifetime             | Progress | Meaning                                                                   |
|-------------|----------------------|----------|---------------------------------------------------------------------------|
| Project     | Ongoing, never ends  | None     | An area of work (`projects`). Groups initiatives and action items         |
| Initiative  | Ends when achieved   | Yes      | A goal too big for one item: "ship the storage page", "deploy to prod"    |
| Action item | Ends when resolved   | None     | One commitment of attention: reply, review, fix, decide                   |
| Link        | Until unlinked       | None     | The external thing an item or initiative points at: a Jira issue, a PR    |
| Changeset   | Until applied        | None     | A staged batch of proposed changes, waiting for the user's approval       |

An action item is linked to any number of projects and any number of initiatives. An initiative is linked to any
number of projects. Items and initiatives with no project are allowed; they show under "No project".

## An action item is a commitment, not a copy

An item linked to a Jira issue does not copy the issue. The provider stays the source of truth for its own fields
(title, status, assignee, description), and Elysium reads them through the link. The item holds only what no
provider has:

- the triage state, priority, due date, snooze, and who the user is waiting on;
- which projects and initiatives it belongs to;
- where it came from, and its history.

Priority and due date are the item's own even when the issue has fields of the same name. They start from the
issue's values when the item is created from it, and are never synced after that: Next ranks on the item's values,
and the item page shows the issue's beside them. Priority defaults to `normal`. `owner` is the exception, because
it decides whose list an item is on: a linked item's owner follows its primary link's assignee, as the watcher
reports it, and an assignee that is the link credential's own account is the user.

Changes flow both ways, driven by Elysium, never by copying fields back and forth:

- **Provider to Elysium.** When any of an item's links closes, is merged, or reaches a done status, the item
  resolves, with the watcher as the actor. A linked issue that reopens returns a resolved item to `open`, since the
  user already accepted it; a dismissed item stays dismissed.
- **Elysium to provider.** Resolving an item moves every linked issue that is still open: a Jira issue takes its
  project's done transition, and a GitHub issue closes as completed. So a pull request merging resolves the item
  and moves the issue it fixes. A pull request is never merged or closed by resolving an item. A comment written on
  an item is posted to its primary link as the credential's account.
- **A pull request closed without merging** resolves nothing. The close is recorded in the item's history and shown
  on it, and the item stays `open`: the work it stands for may still be owed, so the user resolves or dismisses it.

## States

| State       | Meaning                                                                          |
|-------------|----------------------------------------------------------------------------------|
| `inbox`     | Arrived but not accepted. Items the user did not create or accept start here      |
| `open`      | Accepted; the user intends to do it                                              |
| `resolved`  | Done. "Resolved", not "read": the item was acted on                               |
| `dismissed` | Will not be done. Triaged away from the inbox or dropped later                    |

Two fields sit beside the state rather than being states themselves, so an item keeps its place when they clear:

- `snoozedUntil`: hidden from Next until that moment passes.
- `waitingOn`: someone else owes the next step. Hidden from Next, listed under Waiting.

An item the user creates starts `open`, since creating it accepts it. Container children are the other exception to
the inbox: linking the container accepted them, so they start `open` (see
[Containers](#containers-keep-work-from-being-tracked-twice)).

The state changes by name, and a change the current state does not allow is refused:

| Transition | From                    | To          |
|------------|-------------------------|-------------|
| Accept     | `inbox`                 | `open`      |
| Resolve    | `inbox`, `open`         | `resolved`  |
| Dismiss    | `inbox`, `open`         | `dismissed` |
| Reopen     | `resolved`, `dismissed` | `open`      |

Resolving stamps `resolvedAt` and dismissing `dismissedAt`; each is set exactly while the item is in that state, and
reopening clears both. The table is `Transition` in `api/src/action_items/`.

Deleting is soft. `deletedAt` hides the item everywhere, and it can be restored from its history. Every write but
restore refuses a deleted item, and a deleted item counts toward no initiative's progress.

## Ownership and multiple users

Elysium is used by one person today and is built to be shared by a small team. The API has no authentication yet
(`docs/security.md`) and there is no users table, so ownership is recorded without one:

- `owner` is one of three things: the user, someone else by the name or address a provider reports, or nobody,
  for a linked issue with no assignee. It is stored as `owner_kind` (`user`, `other`, `nobody`) with `owner_name`
  set exactly for `other`. Only the user's items appear in Next. Items an initiative tracks on behalf of others (a
  teammate's Jira issue under a linked epic) count toward progress but never appear in Next, and neither do unowned
  items until the user claims one.
- `waitingOn` is free text: a name or an address.
- Every history entry names its actor: `user`, `elysia`, `session:<number>`, or `watcher:<provider>`.

When users exist, `owner`, `waitingOn`, and the `user` actor become references to them, and Next becomes per user.
Nothing else in this design assumes a single user.

## History

Every write records a history entry in the same transaction as the change, so history and state never disagree. An
entry has a kind, its actor, and `data` holding what changed, with the value before and after wherever one was
replaced: enough to show the change and to undo it. An entry is about an item, an initiative, or both, for an item
joining or leaving one, and then shows in both histories. A write that changes nothing records nothing.

| Kind                 | `data`                                          |
|----------------------|-------------------------------------------------|
| `created`            | The fields as created                           |
| `updated`            | `changes`: each field that changed, `{ from, to }` |
| `state_changed`      | `{ from, to }`                                  |
| `deleted`, `restored` | Empty, or the `deletedAt` a restore undid       |
| `commented`          | `{ commentId, body }`                           |
| `comment_edited`     | `{ commentId, from, to }`                       |
| `comment_deleted`    | `{ commentId, body }`, so it can be put back    |
| `project_added`, `project_removed` | `{ projectId }`                   |
| `initiative_joined`, `initiative_left` | Empty; the entry names both      |

Kinds and actors are stored as text rather than enums, so later stages add kinds without an enum migration. Only a
comment's author edits or deletes it: the user cannot rewrite what Elysia or an agent said.

## Next

Next is the default view: one item at a time, with its context and its likely next step, and the full list one
click away. Resolving, dismissing, snoozing, or marking an item as waiting moves straight on to the next one.

An item is in Next when it is `open`, owned by the user, not deleted, not waiting on anyone, and not snoozed. The
order is fixed in code, not configured:

1. Overdue items before the rest, as two groups. Every key below orders within a group.
2. Priority, `urgent`, `high`, `normal`, then `low`.
3. Due date, soonest first, so the most overdue leads; items with no due date after those with one.
4. Age, oldest first.

The id breaks any remaining tie. The order is `api/src/action_items/next.rs`, and `GET /api/v1/action-items/next`
answers the items in it together with how many wait in the inbox. When the inbox is not empty, Next leads with a card
to triage it.

## Initiatives

An initiative has a name, a description, an optional target date, and a state: `active`, `achieved`, or
`abandoned`. Its progress is `resolved / total` over its items. Total counts every member in `inbox`, `open`, or
`resolved`; a dismissed or deleted item counts toward neither.

Progress is never shown as a percentage alone. A bar that drops when scope grows, or reads 100% because the last
task was never written down, misleads. The initiative page shows resolved and total together, and a burnup chart of
both over time, so added scope is visible as scope. To draw it, membership rows keep when an item joined and left
(leaving closes the row and rejoining opens another), and items keep when they were resolved, dismissed, and deleted.
The burnup has a point at the initiative's creation, one at every moment either count changed, and one now; clients
draw it as steps. An item keeps only its latest resolution, so one resolved, reopened, and resolved again counts as
resolved from the second time on. The rules are `api/src/action_items/progress.rs`.

An item in two initiatives counts fully toward both. Each initiative counts only its own members, so nothing is
counted twice within one bar.

### Containers keep work from being tracked twice

An initiative can link to a container instead of to issues one by one:

| Provider | Container                        |
|----------|----------------------------------|
| Jira     | An epic, or a saved JQL search   |
| GitHub   | A milestone, or a label on a repo |

The watcher keeps the container's children as items of the initiative, each linked to its issue and owned as the
provider reports. They start `open`, never in the inbox: the user accepted this work by linking the container, and
children owned by someone else stay out of Next by their owner alone. A child added in Jira joins the initiative; a
child removed leaves it. The user never enters the same work twice, and progress stays true to the tool the team
actually works in.

## Links and the watcher

A link is a provider, an external id, a URL, and the credential that reaches it. One link on an item is its primary
link, where comments are posted and whose assignee is the item's owner. The primary link is the one the item was
created from, or the first one added, and only the user changes it, so adding a link (such as the pull request an
agent opened) never moves an item's ownership. Links go through a provider trait with one implementation per
provider under the action items module, so routes and tools never match on the provider, the same way
`api/src/storage/` works.

| Provider | Linkable                  | Resolved by the provider when   | Resolving the item does          |
|----------|---------------------------|---------------------------------|----------------------------------|
| Jira     | Issue                     | Its status category is `done`   | Applies the project's done transition |
| GitHub   | Issue                     | Closed as completed             | Closes it as completed           |
| GitHub   | Pull request              | Merged                          | Nothing                          |

Mail threads become linkable with email triage, on the roadmap.

A Jira project's done transition is chosen once per project the credential reaches. When the project's workflow has
exactly one transition into a `done` status category, it is used without asking; otherwise the user picks one, and
until then the move waits. Jira is reached through a Jira Cloud credential, sealed in Postgres and bounded by the
projects it names (`docs/jira.md`); GitHub through a personal access token against the fixed `api.github.com`
(`docs/github.md`). Links never reach past what their credential allows.

A provider write that has not succeeded, because it failed or is waiting on a done transition, leaves its link
`pending` with the provider's last answer, shown on the item. The item's own change stands, and the watcher retries
the write on every pass until it succeeds or the user cancels it, so Elysium and the provider never disagree
silently.

The watcher polls. For each credential it asks the provider for everything updated since its last cursor, applies what
changed to linked items and containers, and publishes the result on the event stream. Applying is idempotent: a change
the watcher sees twice, or one that Elysium itself caused, finds the item already in that state and does nothing, and
every provider write checks the provider's current state first, so a retried write never moves an issue twice. Polling
works behind NAT and needs no public address; webhooks are a later upgrade for installs that can receive them.

The watcher is not a proposer and needs no changeset: it records what already happened in a provider. Besides
retrying pending writes that were already approved, the only provider write it causes is the one the user chose for
every resolve: an item it resolves moves its other linked issues.

A GitHub issue closed as not planned dismisses its item, with the watcher as the actor, and moves nothing else.

Notification emails from Jira and GitHub about a linked issue are matched to that issue's item rather than becoming
items of their own.

## Changesets

Anything that is not the user acting directly (Elysia after a meeting, email triage, a coding agent) proposes
changes instead of making them. A changeset is a batch of operations, each with the reason and, where there is one,
the quote and source it came from. Nothing is written, in Elysium or anywhere else, until the user approves.

- The user approves all of it, some of it, or none of it. Each operation is approved or rejected on its own.
- An operation that acts on something another operation creates (commenting on, linking, or resolving a proposed
  item) depends on it. Rejecting an operation rejects its dependents, and the review shows that before approving.
- Approving applies the approved operations in order: Elysium's own data first, then the provider writes. A provider
  write that fails leaves its operation and link `pending`, retried as above, and the rest still apply.
- Operations: create an item, update an item, resolve or dismiss an item, comment, link, add to or remove from an
  initiative, create an initiative.
- Applied operations are recorded in each item's history with the changeset as their source, so one changeset can be
  undone as a whole.

Rules that apply trusted kinds of operations without review are on the roadmap. Until then everything is reviewed.

## Coding sessions

Coding agents reach action items through one relayed MCP server, `elysium_work` (`api/src/tools/`). It is declared
on every session's thread, whether or not the thread also declares `elysium_storage`, which it does only when the
project has a storage location (`docs/storage.md`). Every thread then has a relay, so `docs/coding.md`'s
`409 RELAY_NOT_DECLARED` path is left only for threads created before `elysium_work`; that doc changes with the
implementation. It speaks Elysium's terms (items, initiatives, projects, comments), and never a provider's; the
provider behind a link is Elysium's business. Every call is scoped to the session's project and re-checked against
the database, the way storage tools are.

- A session can be started from an item. It belongs to the item's project when the item has exactly one, and
  otherwise the user picks: one of the item's projects, or any project when it has none. Its first turn carries the
  item, its links' descriptions and recent comments, its initiatives, and its project, so the agent starts with the
  full picture.
- The agent can read items and initiatives in its project, comment on its item, link the pull request it opened,
  and propose changes as a changeset. It cannot resolve or delete anything directly.
- A pull request linked to an item resolves it when it merges, through the watcher.

## Roadmap

- Email: completing an item that came from an email marks the thread read, then archives or deletes it; which of the
  two is not decided.
- Email triage: Elysia reads new mail and proposes items as changesets.
- Meetings: recordings from Slack, Teams, and others are transcribed and uploaded, and Elysia proposes one changeset
  per meeting, citing the transcript.
- Rules that apply trusted operations without review, loosened as Elysia's track record earns it.
- Project memory that coding agents and Elysia read and write, so a session starts with what earlier sessions learned.
- Users and authentication, turning `owner`, `waitingOn`, and actors into references and making Next per user.
- Webhooks for Jira and GitHub, beside polling.
