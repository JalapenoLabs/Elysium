# Action items

Action items are the one list of everything the user owes attention to, wherever it came from: a Jira issue, a
GitHub issue or pull request, an inbound email, an ask from a meeting, or something typed by hand. Initiatives group
action items toward a goal that ends, and carry the progress bar. Projects are long lived and group both.

The core is built: items, initiatives, their projects and memberships, comments, history, Next, and progress, served
under `/api/v1/action-items` and `/api/v1/initiatives` (`docs/api.md`) and kept current on the event stream
(`docs/realtime.md`). So is the frontend for them: Next, inbox triage, the item list and pages, initiatives with their
burnup, and the project page's items and initiatives (`docs/frontend.md`). So are the `elysium_work` tools and coding
sessions started from an item ([Coding sessions](#coding-sessions)), and links to Jira and GitHub with the watcher that
keeps them current ([Links and the watcher](#links-and-the-watcher)), with their frontend: links on the item page,
containers on the initiative page, and the Jira done status choice under Settings. Changesets are still a design; that
section is the plan.

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

The frontend computes Next from the items it holds rather than asking for it, with the same rules and order
(`frontend/src/store/nextOrder.ts`), so acting on an item moves to the next one at once and a snooze that runs out
brings its item back without an event. Each quick action has a key, and skipping sets an item aside for the visit
without changing it.

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

How containers behave in detail:

- A child's item is created with the child's title, priority, and due date, in the initiative's projects, and linked to
  the child as its primary. A child already done when first read is created `open` and resolved at once by the
  watcher, so history shows who resolved it and progress counts it.
- A child that already has an item, through the same credential, joins with that item; nothing is created twice.
- Membership a container brought is its own: `initiative_items.via_link_id` names the container. A child that leaves
  the container leaves the initiative, an item also added by hand stays, and a child the user takes out by hand joins
  again on the next pass while the container still holds it.
- Unlinking a container takes out every item it brought in. The items themselves stay, with their links.
- Every read reads the container itself first. One that is gone, such as a GitHub label renamed or an epic deleted,
  records why on the container and takes nobody out until the user unlinks it.
- The watcher reads up to 1,000 children per container. A container past that is marked `truncated`, and while it is,
  no child is taken out, since one missing from a cut short read may still be there.
- A GitHub milestone or label contributes its issues. Pull requests in it are left out: the issues they fix are the
  work.
- An epic is any Jira issue with children: its children are the issues whose parent it is. A saved filter's children
  are the issues its JQL finds, bounded by the credential's allowlist like every search.

## Links and the watcher

A link is a provider, an external id, a URL, and the credential that reaches it. One link on an item is its primary
link, where comments are posted and whose assignee is the item's owner. The primary link is the one the item was
created from, or the first one the user added, and only the user changes it, so adding a link (such as the pull request
an agent opened) never moves an item's ownership. Removing the primary makes the oldest link left the primary, since
the user moved it by removing it. Links go through a provider trait with one implementation per provider under the
action items module, so routes and tools never match on the provider, the same way `api/src/storage/` works.

The code lives in `api/src/action_items/links/`: the `Provider` trait and the `Links` handle in `mod.rs` (its
`provider` function is the one place a provider is picked), `jira.rs` and `github.rs`, and `rules.rs`, the pure
decisions below. The watcher is `api/src/action_items/watcher.rs`.

A link names its thing in two ways. `externalId` is what the thing is, for good: Jira's issue id, which survives a move
to another project, or `owner/name#12` on GitHub. `key` is what a person reads and a call names: `ELY-12`, which the
watcher updates when an issue moves. One external thing is one item: linking something already linked to another item
answers `409`, and a container's child already tracked joins with its item.

Items are linked from what the credential can reach, never from a typed id, the spirit of the Jira doc's "No
freeform entry": a picker lists issues (a Jira search, or a GitHub repository's open issues and pull requests), and
the link request names the one picked. The API reads it through the credential before anything is stored, so the
allowlist and the token bound every link. Creating an item from an issue starts its title, priority, and due date
from the issue, owned as the issue's assignee says. Jira's priority names map to an item's (`Highest`, `Blocker`, and
`Critical` are `urgent`; `High` and `Major` are `high`; `Medium` is `normal`; `Low`, `Minor`, `Lowest`, and `Trivial`
are `low`; anything else is `normal`), and a Jira due date, a day, becomes the last millisecond of that day in UTC.
GitHub has neither, so an item from GitHub starts `normal` with no due date.

Deleting a credential deletes the links and containers that went through it: Elysium can no longer reach what they
point at. The items and initiatives stay, and so do the memberships a deleted container brought in, which then count as
added by hand: the work was accepted, and only the way of watching it is gone. Unlinking a container is the user
saying the work no longer belongs, so that takes its items out.

| Provider | Linkable                  | Resolved by the provider when   | Resolving the item does          |
|----------|---------------------------|---------------------------------|----------------------------------|
| Jira     | Issue                     | Its status category is `done`   | Applies the project's done transition |
| GitHub   | Issue                     | Closed as completed             | Closes it as completed           |
| GitHub   | Pull request              | Merged                          | Nothing                          |

Mail threads become linkable with email triage, on the roadmap.

A Jira project's done transition is chosen once per project the credential reaches. When the project's workflow has
exactly one transition into a `done` status category, it is used without asking; otherwise the user picks one, and
until then the move waits. The choice is stored as the `done` status the transition leads into
(`jira_done_transitions`), because a workflow names a different transition into the same status from each status an
issue can be in: a close takes whichever transition available right now leads there. A project with exactly one status
in the `done` category needs no choice. See `docs/jira.md`. Jira is reached through a Jira Cloud credential, sealed in
Postgres and bounded by the projects it names (`docs/jira.md`); GitHub through a personal access token against the
fixed `api.github.com` (`docs/github.md`). Links never reach past what their credential allows.

A provider write that has not succeeded, because it failed or is waiting on a done transition, leaves its link
`pending` with the provider's last answer, shown on the item. The item's own change stands, and the watcher retries
the write on every pass until it succeeds or the user cancels it, so Elysium and the provider never disagree
silently.

Writes are owed in the same transaction as the change that owes them (`action_item_link_writes`), so a crash between
the two never loses one:

- Resolving an item, by anyone, owes a close to every linked issue still open as last read. A pull request never owes
  one.
- Writing a comment on an item, by the user or an agent, owes it to the item's primary link. An item with no primary
  link owes nothing. Editing or deleting a comment afterwards changes nothing in the provider; deleting one before it
  is posted drops the write.
- The watcher lands every owed write at the start of each pass, and the change that owed one wakes it at once, so a
  write usually lands within a second. A link owing a write is pending until it lands; one that failed shows how many
  times it was tried and the provider's last answer. Cancelling a write records `link_write_cancelled` on the item.
- A close owed by an item that has since been reopened or deleted is dropped rather than sent.

The watcher polls. For each credential it asks the provider for everything updated since its last cursor, applies what
changed to linked items and containers, and publishes the result on the event stream. Applying is idempotent: a change
the watcher sees twice, or one that Elysium itself caused, finds the item already in that state and does nothing, and
every provider write checks the provider's current state first, so a retried write never moves an issue twice. Polling
works behind NAT and needs no public address; webhooks are a later upgrade for installs that can receive them.

How the watcher reads:

- A pass runs at startup, every 60 seconds (`WATCH_INTERVAL`), and whenever a write wakes it. It is a constant in code,
  not configuration.
- Each credential's cursor (`link_watch_cursors`) is the moment its last successful pass began, so a restart resumes
  there. A credential never read before is read from its oldest link. Every read reaches five minutes further back than
  the cursor, so a change made in the moment the cursor was taken, or under a small clock difference, is read again
  rather than missed.
- Jira: the linked issues' ids, fifty to a search, `AND updated >= -<minutes>m`, bounded by the allowlist. A search
  Jira refuses, which is what naming a deleted issue looks like, is read one issue at a time instead.
- GitHub: each linked repository's issues and pull requests `since` the cursor, three pages at most. A busier
  repository, or a credential never read before, is read link by link.
- The watcher acts on a change of what a link last recorded, never on the provider's state alone (`rules.rs`). So an
  item the user reopened stays open while its issue stays done, and a close Elysium made, recorded as done when it
  landed, is not read as news.
- A primary link's assignee changing moves the item's owner, with the watcher as the actor. Only a change moves it, so
  an owner the user set by hand stays until the assignee changes again.
- A credential that cannot be read records why on its cursor and is read again from the same point next pass; a
  container that cannot be read records why on itself. Neither stops the rest of the pass.
- The watcher stops with the process's shutdown token, and the API awaits it before closing the database.

The watcher is not a proposer and needs no changeset: it records what already happened in a provider. Besides
retrying pending writes that were already approved, the only provider write it causes is the one the user chose for
every resolve: an item it resolves moves its other linked issues.

A GitHub issue closed as not planned dismisses its item, with the watcher as the actor, and moves nothing else.

The rules as the watcher applies them, for a link whose state changed from what it last recorded:

| The link now      | The item                                                                          |
|-------------------|-----------------------------------------------------------------------------------|
| Done or merged    | Resolves when it is in the inbox or open; a resolved or dismissed item stays       |
| Not planned       | Is dismissed when it is in the inbox or open                                       |
| Open again, after done or not planned | Returns to `open` when it is resolved; a dismissed item stays dismissed |
| Closed unmerged   | Stays as it is; `pull_request_closed` is recorded in its history                   |

History records links too: `link_added`, `link_removed`, and `primary_link_changed` on items, `container_linked` and
`container_unlinked` on initiatives, with the link's provider, kind, key, and URL.

The frontend shows an item's links on its page, each with its provider's fields read live (status, assignee,
priority, due date) beside what the link last recorded, and the writes it still owes with their tries and a way to
stop. Links are added from a picker (a Jira search, or a GitHub repository's open issues or pull requests), and New
item can start from one. The initiative page lists the containers it follows with when each was last read, and adds
them from a picker of epics, saved filters, milestones, and labels. A close waiting on a Jira project's done status
links to that choice under Settings, Jira (`docs/frontend.md`).

Notification emails from Jira and GitHub about a linked issue are matched to that issue's item rather than becoming
items of their own. That is on the roadmap.

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

Coding agents reach action items through one relayed MCP server, `elysium_work` (`api/src/tools/work.rs`). It is
declared on every session's thread, whether or not the thread also declares `elysium_storage`, which it does only when
the project has a storage location (`docs/storage.md`). Every thread therefore has a relay, and `docs/coding.md`'s
`409 RELAY_NOT_DECLARED` path is left only for threads created before `elysium_work`. It speaks Elysium's terms
(items, initiatives, projects, comments), and never a provider's; the provider behind a link is Elysium's business.
Every call is scoped to the session's project and re-checked against the database, the way storage tools are.

| Tool               | Arguments                                                  | Answers                                                     |
|--------------------|------------------------------------------------------------|-------------------------------------------------------------|
| `work_project`     | none                                                       | The project, its active initiatives, its items in the inbox or open, and `sessionItemId` |
| `work_items`       | optional `states`, `initiativeId`, `waiting`, `search`, `limit` | `{ items, moreItems }`, newest first, without notes    |
| `work_item`        | `itemId`                                                   | The item with its notes, comments, latest 50 history entries, projects, and initiatives |
| `work_initiatives` | optional `states`                                          | `{ initiatives }` with progress, by name                    |
| `work_initiative`  | `initiativeId`                                             | The initiative with its description and the project's items in it, up to 200 with `moreItems` |
| `work_comment`     | `itemId`, `body`                                           | `{ comment }`                                               |
| `work_link_pull_request` | `url` of a pull request on github.com                | `{ link }`: its item, key, URL, title, and state             |

- An item or initiative is reachable while it is live and in the session's project, as the database says at the
  moment of the call. Anything else is refused the same way, deleted or elsewhere, pointing the agent at the list
  tool. That holds for an initiative `work_items` filters on too. An initiative's members in other projects are
  counted, not shown; its progress counts them all.
- `work_items` lists items in the inbox or open unless asked for other states, at most 50 unless `limit` says
  otherwise, up to 200. `search` matches the title or notes, ignoring case.
- The agent reads items and initiatives in its project and comments on its project's items. A comment is written as
  `session:<number>`, recorded in the item's history, published on the event stream, and posted to the item's primary
  link like the user's; only its author may change it, so the user cannot rewrite it either. Proposing changes as a
  changeset arrives with changesets; the server's instructions tell the agent to ask the user for any other change
  until then. It cannot resolve or delete anything directly.
- `work_link_pull_request` links a pull request the agent opened to the item its session was started from, and only
  that item, live and in the project as the database says at the moment of the call. The pull request is read through
  the GitHub token the session started with (`coding_sessions.github_credential_id`), so a session without an item or
  without a token is refused and told to mention the pull request in a comment instead. The link is recorded as
  `session:<number>` and never becomes the item's primary, so it never moves the item's owner.
- A session can be started from an item. It belongs to the item's project when the item has exactly one, and
  otherwise the user picks: one of the item's projects, or any project when it has none. The session records the item
  (`coding_sessions.action_item_id`), the item's page lists the sessions started from it, and a session's
  conversation links back to its item.
- The first turn of a session started from an item carries the item, its latest 10 comments, its initiatives with
  their progress, and its project, then the user's prompt, which such a session requires. It is compact Markdown built
  by `api/src/action_items/session_context.rs`: notes, descriptions, and comments longer than a fixed length are cut
  short and marked, since `work_item` reads them in full. The linked issues' descriptions are not in it yet.
- A pull request linked to an item resolves it when it merges, through the watcher, and the resolve then moves the
  item's other linked issues.

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
- Notification emails from Jira and GitHub folded into the linked issue's item.
- The linked issues' descriptions in the first turn of a session started from an item.
- Editing or deleting a comment carried to the comment already posted to the provider.
