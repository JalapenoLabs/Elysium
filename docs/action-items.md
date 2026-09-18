# Action items

Action items are the one list of everything the user owes attention to, wherever it came from: a Jira issue, a
GitHub issue or pull request, an inbound email, an ask from a meeting, or something typed by hand. Initiatives group
action items toward a goal that ends, and carry the progress bar. Projects are long lived and group both.

The design goal is that work arrives from anywhere, is triaged and managed in one place, and every change made here
reaches the system it came from. The user approves; Elysium and Elysia do the bookkeeping.

## Terms

| Term        | Lifetime             | Progress | Meaning                                                                   |
|-------------|----------------------|----------|---------------------------------------------------------------------------|
| Project     | Ongoing, never ends  | None     | An area of work (`projects`). Groups initiatives and action items         |
| Initiative  | Ends when achieved   | Yes      | A goal too big for one item: "ship the storage page", "deploy to prod"    |
| Action item | Ends when resolved   | None     | One commitment of attention: reply, review, fix, decide                   |
| Link        | As long as its item  | None     | The external thing an item or initiative points at: a Jira issue, a PR    |
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

Changes flow both ways, driven by Elysium, never by copying fields back and forth:

- **Provider to Elysium.** When the linked issue closes, is merged, or reaches a done status, the item resolves,
  with the watcher as the actor. A reopened issue reopens the item.
- **Elysium to provider.** Resolving an item moves its linked issue automatically: a Jira issue takes its project's
  done transition, and a GitHub issue closes as completed. A pull request is never merged or closed by resolving an
  item. A comment written on an item is posted to its primary link as the credential's account.

## States

| State       | Meaning                                                                          |
|-------------|----------------------------------------------------------------------------------|
| `inbox`     | Arrived but not accepted. Everything that is not typed by the user starts here    |
| `open`      | Accepted; the user intends to do it                                              |
| `resolved`  | Done. "Resolved", not "read": the item was acted on                               |
| `dismissed` | Will not be done. Triaged away from the inbox or dropped later                    |

Two fields sit beside the state rather than being states themselves, so an item keeps its place when they clear:

- `snoozedUntil`: hidden from Next until that moment passes.
- `waitingOn`: someone else owes the next step. Hidden from Next, listed under Waiting.

Deleting is soft. `deletedAt` hides the item everywhere, and it can be restored from its history.

## Ownership and multiple users

Elysium is used by one person today and is built to be shared by a small team. The API has no authentication yet
(`docs/security.md`) and there is no users table, so ownership is recorded without one:

- `owner` is null for the user's own items, and otherwise the name or address of whoever owns it, as a provider
  reports it. Items an initiative tracks on behalf of others (a teammate's Jira issue under a linked epic) count
  toward progress but never appear in Next.
- `waitingOn` is free text: a name or an address.
- Every history entry names its actor: `user`, `elysia`, `session:<number>`, or `watcher:<provider>`.

When users exist, `owner`, `waitingOn`, and the `user` actor become references to them, and Next becomes per user.
Nothing else in this design assumes a single user.

## Next

Next is the default view: one item at a time, with its context and its likely next step, and the full list one
click away. Resolving, dismissing, snoozing, or marking an item as waiting moves straight on to the next one.

An item is in Next when it is `open`, owned by the user, not deleted, not waiting on anyone, and not snoozed. The
order is fixed in code, not configured:

1. Overdue items, most overdue first.
2. Priority, `urgent`, `high`, `normal`, then `low`.
3. Due date, soonest first; items with no due date after those with one.
4. Age, oldest first.

When the inbox is not empty, Next leads with a card to triage it.

## Initiatives

An initiative has a name, a description, an optional target date, and a state: `active`, `achieved`, or
`abandoned`. Its progress is `resolved / total` over its items, where a dismissed item counts toward neither.

Progress is never shown as a percentage alone. A bar that drops when scope grows, or reads 100% because the last
task was never written down, misleads. The initiative page shows resolved and total together, and a burnup chart of
both over time, so added scope is visible as scope. To draw it, membership rows keep when an item joined and left,
and items keep when they were resolved.

An item in two initiatives counts fully toward both. Each initiative counts only its own members, so nothing is
counted twice within one bar.

### Containers keep work from being tracked twice

An initiative can link to a container instead of to issues one by one:

| Provider | Container                        |
|----------|----------------------------------|
| Jira     | An epic, or a saved JQL search   |
| GitHub   | A milestone, or a label on a repo |

The watcher keeps the container's children as items of the initiative, each linked to its issue and owned as the
provider reports. A child added in Jira joins the initiative; a child removed leaves it. The user never enters the
same work twice, and progress stays true to the tool the team actually works in.

## Links and the watcher

A link is a provider, an external id, a URL, and the credential that reaches it. One link on an item is its primary
link, where comments are posted. Links go through a provider trait with one implementation per provider under the
action items module, so routes and tools never match on the provider, the same way `api/src/storage/` works.

| Provider | Linkable                  | Resolved by the provider when   | Resolving the item does          |
|----------|---------------------------|---------------------------------|----------------------------------|
| Jira     | Issue                     | Its status category is `done`   | Applies the project's done transition |
| GitHub   | Issue                     | Closed as completed             | Closes it as completed           |
| GitHub   | Pull request              | Merged                          | Nothing                          |
| Mail     | Message thread            | Never                           | Marks the thread read            |

A Jira project's done transition is chosen once per project the credential reaches. When the project's workflow has
exactly one transition into a `done` status category, it is used without asking; otherwise the user picks one, and
until then resolving the item says the issue was not moved. Every Jira and GitHub call goes through the existing
credential and its allowlist.

The watcher polls. For each credential it asks the provider for everything updated since its last cursor, applies
what changed to linked items and containers, and publishes the result on the event stream. Polling works behind NAT
and needs no public address; webhooks are a later upgrade for installs that can receive them.

Notification emails from Jira and GitHub about a linked issue are matched to that issue's item rather than becoming
items of their own.

## Changesets

Anything that is not the user acting directly (Elysia after a meeting, email triage, a coding agent) proposes
changes instead of making them. A changeset is a batch of operations, each with the reason and, where there is one,
the quote and source it came from. Nothing is written, in Elysium or anywhere else, until the user approves.

- The user approves all of it, some of it, or none of it. Each operation is approved or rejected on its own.
- Approving applies the approved operations in order: Elysium's own data first, then the provider writes. A provider
  write that fails marks that operation failed with the provider's answer, and the rest still apply.
- Operations: create an item, update an item, resolve or dismiss an item, comment, link, add to or remove from an
  initiative, create an initiative.
- Applied operations are recorded in each item's history with the changeset as their source, so one changeset can be
  undone as a whole.

Rules that apply trusted kinds of operations without review are on the roadmap. Until then everything is reviewed.

## Coding sessions

Coding agents reach action items through one relayed MCP server, `elysium_work`, declared on every session's thread
beside `elysium_storage` (`api/src/tools/`). It speaks Elysium's terms (items, initiatives, projects, comments), and
never a provider's; the provider behind a link is Elysium's business. Every call is scoped to the session's project
and re-checked against the database, the way storage tools are.

- A session can be started from an item. Its first turn carries the item, its links' descriptions and recent
  comments, its initiatives, and its project, so the agent starts with the full picture.
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
