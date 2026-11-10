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
containers on the initiative page, and the Jira done status choice under Settings. So are changesets
([Changesets](#changesets)): proposing, deciding, applying, and undoing them under `/api/v1/changesets`, and the
`work_propose_changes` tool coding agents propose them with, and their review in the Action items area.

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

An action item is linked to any number of projects and any number of initiatives, and one of its projects leads it
([Projects and the lead](#projects-and-the-lead)). An initiative is linked to any number of projects. Items and
initiatives with no project are allowed; they show under "No project".

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
  and moves the issue it fixes. A pull request is never merged or closed by resolving an item, and Elysium closes
  none. The one pull request Elysium merges is a pool worker's own, in a repository its project set to auto-merge,
  once every fail-closed check passes (`docs/engine.md`, Merging); that merge is GitHub's news like any other, which
  the watcher reads. A comment written on an item is posted to its primary link as the credential's account.
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

A person can change many items at once from the item list: move them to a state, set a priority, snooze them, add
them to or take them out of an initiative, assign them to their Project Managers or take them back, delete, or restore
them (`POST /api/v1/action-items/bulk`, `docs/api.md`).
A state is named as where the items should end up, and each item takes the transition that leads there from its own
state, so a selection of inbox and open items can be resolved together. Every item goes through the write it would take
alone, in one transaction, each in a savepoint of its own: an item the rules refuse is left as it was and reported,
and the rest apply. `api/src/models/action_item_bulk.rs` holds the queries; `Transition::toward` the rule. Initiatives
change many at once the same way: a state, delete, or restore (`POST /api/v1/initiatives/bulk`,
`api/src/models/initiative_bulk.rs`).

## Ownership and multiple users

Elysium is one workspace shared by everyone who signs in (`docs/auth.md`). Every item and initiative records who
created it (`createdBy`), and every history entry names its actor: `user:<id>` for a person, `elysia`,
`session:<number>`, `watcher:<provider>`, or `system` for Elysium reporting on its own work, such as a night's
database backup that failed, or a meeting whose corrected transcript waits for its person's approval
(`docs/meetings.md`, Correction). Entries written before accounts say `user` alone.

Ownership predates accounts and is still recorded as text:

- `owner` is one of three things: the user, someone else by the name or address a provider reports, or nobody,
  for a linked issue with no assignee. It is stored as `owner_kind` (`user`, `other`, `nobody`) with `owner_name`
  set exactly for `other`. Only the user's items appear in Next. Items an initiative tracks on behalf of others (a
  teammate's Jira issue under a linked epic) count toward progress but never appear in Next, and neither do unowned
  items until the user claims one.
- `waitingOn` is free text: a name or an address.

The roadmap is for `owner` and `waitingOn` to reference users, and for Next to be each person's own.

## Projects and the lead

An item can sit on several projects, since work often touches several, but an item in any project has exactly one lead
project, and only that project's Project Manager works it: it alone may submit a batch for the item
(`docs/engine.md`, Work batches).

- **How each got there.** Each membership records how sure whoever put the item there was and who that was
  (`action_item_projects.confidence` and `attributed_by`, the actor as history names it). Everyone's choice is certain;
  only the attribution step guesses.
- **Which leads.** An item's first project leads it: the first one named when it is created, or the first one it
  joins. When the lead leaves, the surest of those left leads instead, a person's choice before a guess and the
  earliest joined among equals, or none once none is left. Each move is recorded as `lead_project_changed`, and one
  that followed a change of projects says so (`passedOn`), since nobody chose it. A person makes any of its projects
  the lead on the item's page.
- **Attribution.** An issue assigned to its person arrives in no project ([Assigned issues](#assigned-issues)), and a
  run of the `assigned_issue` workflow reads it with `segment_meeting`, the step that segments meetings and
  conversations: one turn reads the item's title, notes, and the issues it follows against every project, where each
  files its issues, and the repositories its workers clone, and answers the item's projects with Elysia's confidence
  (`docs/meetings.md`, Segmentation). A guess is kept only for a project that was offered, once, at a confidence of
  at least 0.3, as a segment's attributions are. The item joins each, surest first, and unless someone chose its lead,
  the surest of its projects leads it: a person's choice before her guesses, and her surest guess among those.
- **Anyone else's word stands.** Elysia's reading leaves alone any project anyone else put the item in or took it out
  of, a changeset someone approved included, and never moves a lead anyone else chose, nor takes the item off that
  lead; a lead that only passed on is nobody's choice. Her own earlier reading she replaces, so a reading run again keeps no guess it dropped.
- **Undo.** A person undoes any change to an item's projects or its lead from its history. The undo is an entry of its
  own naming the one it reversed (`undid`), so it can be undone in turn, and a project put back is held as it was,
  Elysia's confidence included. Only the latest change to that project, or to the lead, is undone; one with a change
  made since answers `409` rather than overwriting it. An item still in a project always has a lead, so a lead that
  came with its first project is changed by choosing another rather than undone.
- **Nothing starts.** Attributing an item wakes no Project Manager and starts no worker. The wake for a record newly on
  a project (`docs/connectors.md`, The timeline) is a timeline record's, and an item is not one; assigning an item to
  its lead project's Project Manager is what starts planning ([Assigning to a Project
  Manager](#assigning-to-a-project-manager)).

The rules are `api/src/models/action_item_attribution.rs`, with the memberships and the lead in
`api/src/models/action_item.rs`.

## Assigning to a Project Manager

A person picks which work is taken on: assigning an item to its lead project's Project Manager wakes it to plan the
item. Attribution decides where an item belongs; assigning decides it is to be worked. The code is
`api/src/models/action_item_assignment.rs`.

- **Who assigns.** A person, from the item's page (`PUT /api/v1/action-items/{id}/manager-assignment`) or a selection of
  the item list (`assign-to-manager` in the bulk route), and only the company's members, who alone see Project Managers.
  The model holds this too (`require_member`), so a guest approving Elysia's suggestion, or undoing it, is refused like
  one calling the route. Elysia suggests: an `assign-item` operation in a changeset, with why the item is ready, which
  the person approves or rejects like any other ([Changesets](#changesets)). Approving it is the person assigning the
  item: the item records the person as who assigned it, and its history names Elysia's changeset. No coding agent or
  Project Manager may propose one, since that would be work assigning itself work.
- **To its lead's Project Manager.** Only the lead project's Project Manager may submit work for an item, so an item is
  assigned to that one and no other, and the assignment names no project of its own
  (`action_items.manager_assigned_at` and `manager_assigned_by`). An item no project leads cannot be assigned, and its
  page says so; neither can one resolved, dismissed, or deleted. An item in the inbox is accepted as it is assigned,
  since assigning it is taking it on. Assigning an item already assigned changes nothing.
- **What it starts.** The assignment writes an `assignment` wake for the lead project's Project Manager in the same
  transaction, naming the item and the person (`docs/engine.md`, Project Managers). Its next turn reads the item, its
  notes, its links, and who assigned it, and plans it into the batch or batches it needs, or says in its reply why it
  cannot yet; an item resolved or dismissed before that turn runs is left out of it. It starts no worker: every batch it plans is proposed, for the person to approve (`docs/engine.md`,
  Proposed batches). A batch planned for the item is human-rooted, through the item, since its history says a person
  assigned it (`docs/engine.md`, Origins and generations).
- **When it ends.** A person takes the item back (`DELETE .../manager-assignment`), which drops the wake if no turn has
  taken it; what the Project Manager already planned stays, each batch the person's to decide. Moving the item's lead
  to another project, or taking it out of its last one, ends the assignment too, recorded as `manager_unassigned` with
  `leadMoved`: nobody asked the new lead's Project Manager to plan it, so the person assigns it again if they want it
  planned there. Undoing the lead's move does not bring the assignment back. Deleting the item ends it as well, so a
  restored item is not shown as planned when nothing planned it.
- **Where its plan stands.** `GET .../manager-assignment` answers, while the item is assigned, the lead project, who
  assigned it and when, the stage (`waiting` for its Project Manager's next turn, `planning` while that turn runs,
  `planned` once it finished, or `failed`), the turn that took it, and the lead project's latest ten batches naming the
  item with their states. It is read when asked, from the assignment's wake and the batches, so nothing stored can
  disagree with them, and members alone read it, as they do batches.

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
| `project_added`, `project_removed` | `{ projectId, confidence, attributedBy }`, as it was held |
| `lead_project_changed` | `{ from, to }`, each a project id or null, and `passedOn` when it followed a change of projects |
| `initiative_joined`, `initiative_left` | Empty; the entry names both      |
| `unassigned`         | `{ linkId, provider, key, url, assignee }`: a linked issue the user was assigned is no longer theirs; `assignee` is who has it now, or null |
| `manager_assigned`   | `{ projectId }`: a person assigned the item to that project's Project Manager, its lead then |
| `manager_unassigned` | `{ projectId }`, null for an assignment a stray delete left without a lead, and `leadMoved` when it ended because the lead moved to another project |

An entry that undoes another, from the item's history, also names it (`undid`). Kinds and actors are stored as text
rather than enums, so later stages add kinds without an enum migration. Only a
comment's author edits or deletes it: the user cannot rewrite what Elysia or an agent said. The one exception is undoing
the changeset that wrote it, which the user approved.

An entry recorded by applying a changeset, with the proposer as its actor, or by undoing one, with the user as its
actor, names that changeset as its source (`changesetId`).

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
`provider` function is the one place a provider is picked), `jira.rs`, `github.rs`, and `youtrack.rs`, and `rules.rs`,
the pure decisions below. The watcher is `api/src/action_items/watcher.rs`.

A link names its thing in two ways. `externalId` is what the thing is, for good: Jira's issue id, which survives a move
to another project, `owner/name#12` on GitHub, or YouTrack's database id, such as `2-41`. `key` is what a person reads
and a call names: `ELY-12` or `JL-12`, which the watcher updates when an issue moves. One external thing is one item: linking something already linked to another item
answers `409`, and a container's child already tracked joins with its item.

Items are linked from what the credential can reach, never from a typed id, the spirit of the Jira doc's "No
freeform entry": a picker lists issues (a Jira search, or a GitHub repository's open issues and pull requests), and
the link request names the one picked. The API reads it through the credential before anything is stored, so the
allowlist and the token bound every link. Creating an item from an issue starts its title, priority, and due date
from the issue, owned as the issue's assignee says. Jira's priority names map to an item's (`Highest`, `Blocker`, and
`Critical` are `urgent`; `High` and `Major` are `high`; `Medium` is `normal`; `Low`, `Minor`, `Lowest`, and `Trivial`
are `low`; anything else is `normal`), and a Jira due date, a day, becomes the last millisecond of that day in UTC.
GitHub has neither, so an item from GitHub starts `normal` with no due date. YouTrack's `Priority` field maps the same
way (`Show-stopper` and `Critical` are `urgent`, `Major` is `high`, `Normal` is `normal`, `Minor` is `low`); it keeps
no due date.

A YouTrack issue links through a YouTrack token account (`docs/connectors.md`, YouTrack), which is its owner's alone, so
only that person links through it. The link picker offers Jira and GitHub today; YouTrack issues are linked through
`POST /api/v1/action-items/{id}/links` and by the issues Project Managers file (`docs/engine.md`, Its writes).
Initiatives contain no YouTrack issues yet.

Deleting a credential deletes the links and containers that went through it: Elysium can no longer reach what they
point at. The items and initiatives stay, and so do the memberships a deleted container brought in, which then count as
added by hand: the work was accepted, and only the way of watching it is gone. Unlinking a container is the user
saying the work no longer belongs, so that takes its items out.

| Provider | Linkable                  | Resolved by the provider when   | Resolving the item does          |
|----------|---------------------------|---------------------------------|----------------------------------|
| Jira     | Issue                     | Its status category is `done`   | Applies the project's done transition |
| GitHub   | Issue                     | Closed as completed             | Closes it as completed           |
| GitHub   | Pull request              | Merged                          | Nothing                          |
| YouTrack | Issue                     | YouTrack counts its state resolved | Moves it to a resolved state of its project's state field |

YouTrack's state field is whatever the project calls it (`State`, `Stage`), found by its type. Resolving moves an issue
to the field's only resolved value, or with several to the first of `Done`, `Fixed`, `Resolved`, `Completed`,
`Verified`, and `Closed` it has, never `Duplicate` or `Won't fix`; a field with several and none of those waits, naming
them, like a Jira project with several done statuses.

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
  link owes nothing, including one whose own issue is still being filed.
- Applying a changeset's `file-issue` owes the item its own issue (`action_item_issue_filings`): the watcher opens it in
  the tracker the operation names, with the item's title and notes, and links it as the item's primary, keeping the
  item's owner, since nobody is assigned a new issue yet; the owner follows the issue's assignee from the first change
  the watcher reads. An item is filed
  once at a time, and one already following an issue is refused. A filing records each attempt as it begins, and an
  attempt after the first first looks for the issue the earlier one may have opened (opened by the credential's
  account in that place since then, with exactly the title) and links that rather than opening a second. A filing
  whose item was resolved, dismissed, or deleted first is dropped. One that fails shows on the item with its tries and
  the provider's last answer, and the user can cancel it (`DELETE /api/v1/action-items/{id}/issue-filing`), which
  records `issue_filing_cancelled`. Editing or deleting a comment afterwards changes nothing in the provider; deleting one before it
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
- A linked issue the user was assigned being unassigned, or given to someone else, leaves the item where it is and says
  so: `unassigned` is recorded in its history, naming who has it now, and the item page shows it. Whether the work is
  still the user's is theirs to decide.
- Each pass also brings in the issues assigned to every issue connector's person ([Assigned issues](#assigned-issues)).
- A credential that cannot be read records why on its cursor and is read again from the same point next pass; a
  container that cannot be read records why on itself. Neither stops the rest of the pass.
- The watcher runs in the engine (`docs/engine.md`). It stops with the engine's shutdown token, and the engine awaits it
  before closing the database. A write that owes a provider something wakes it from any process, over the bus.

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

## Assigned issues

Every connector that provides issues (Jira, GitHub, YouTrack, `docs/connectors.md`) brings in what is assigned to its
person, as one capability of `Provides::Issues`: a kind that declares it, and names its link provider in
`kinds::link_provider` (a test fails when it does not), gets this with nothing else to build. The code is
`api/src/action_items/assigned.rs`, run by the watcher on each pass for every active issue connector's account.

- **What is read.** Every open issue assigned to the connector's account, through its link provider's `assigned`,
  bounded by the connector's content: Jira by its account's allowlist (`docs/jira.md`), GitHub by the repositories it
  chose (`docs/github.md`, Assigned issues), YouTrack by the projects it chose. Pull requests are left out. Each issue
  is checked again on the way back, so one the provider lists loosely, not open or not the person's, brings nothing in.
- **What it makes.** Each issue the connector has not brought in before raises one inbox item, owned by the user,
  titled as the issue, starting from its priority and due date, in no project, with the issue linked as its primary
  through the connector's account, and starts a run of the `assigned_issue` workflow that attributes it to its
  projects ([Projects and the lead](#projects-and-the-lead)). It is recorded as the watcher of its service (`watcher:jira`), as ingest raises
  items: like the link watcher, it records what already happened in a provider. From then on the link is followed like
  any other: the issue closing resolves the item, and its being unassigned is recorded on it (Links and the watcher).
- **Everything comes in.** The first read brings in everything already assigned, unlike a mailbox's, which records its
  backlog without raising anything: an assigned issue is owed work however old it is.
- **Never twice.** `assigned_issues` (`docs/database.md`) keeps one row per connector and issue, written in the same
  transaction as the item and its link, and an issue is linked once per account. So an issue never raises a second
  item: not on the next pass, nor after a restart, nor when two engines read at once (the second's transaction is
  refused and writes nothing), nor once its item was dismissed, deleted, or unlinked from it. One already linked by
  hand through the same account raises nothing, and is recorded as brought in, naming its item. Matching is per
  account, by the issue's own id: Jira and YouTrack ids and keys repeat across sites, so a match by key alone could
  join two different issues. So an issue linked by hand through another account of the same provider comes in as an
  item of its own.
- **Every pass reads it all.** Each pass reads the whole of what is assigned rather than what changed since a cursor, so
  a widened content, or an issue assigned in a project just picked, comes in on the next pass with no cursor to reset.
  A read takes at most ten pages of a hundred, most recently updated first; most people's fit in one request. Past a
  thousand, the rest come in once they are next updated, and the pass logs that it was cut short. Items are made
  oldest first, each in its own transaction, so the inbox lists them in the order they were last worked on. One that
  cannot be stored is passed over, so it never holds back the rest, and the pass fails naming it, so its connector
  shows why until it comes in.
- **Attributed, and nothing more.** An issue arriving makes an item, its link, and its attribution run, in one
  transaction, and nothing else. Attributing it puts it on its projects and wakes no Project Manager, and Elysia's
  intake leaves the items and links a watcher makes alone (`docs/elysia.md`, Triggers). It waits in the inbox for a
  person. What a connector records on the timeline is apart
  from this: YouTrack's reader records every new issue in its chosen projects, assigned or not, and that record reaches
  Elysia as every ingested record does, as news of the project rather than of the assignment.
- **Reported as runs.** What a pass brought in counts toward its connector's run with the changes it read, and a read
  that fails is a failed run of the connector, YouTrack's included (`docs/connectors.md`, Sync status and activity).

An issue that was dismissed stays dismissed if it is unassigned and later assigned to the person again. Removing a
connector removes its account and the links made through it, and forgets what it brought in, so adding it again brings
everything assigned in again as new items.

## Changesets

Anything that is not the user acting directly (Elysia after a meeting, email triage, a coding agent, a project's
Project Manager) proposes changes instead of making them. A changeset is a batch of operations, each with the reason
and, where there is one, the quote and source it came from. Nothing is written, in Elysium or anywhere else, until the
user approves, except on a project that lets its Project Manager write directly, whose Project Manager's changesets are
approved and applied as they are made (`docs/engine.md`, Its writes).

- The user approves all of it, some of it, or none of it. Each operation is approved or rejected on its own.
- An operation that acts on something another operation creates (commenting on, linking, or resolving a proposed
  item) depends on it. Rejecting an operation rejects its dependents, and the review shows that before approving.
- Approving applies the approved operations in order: Elysium's own data first, then the provider writes. A provider
  write that fails leaves its operation and link `pending`, retried as above, and the rest still apply.
- Operations: create an item, update an item, resolve or dismiss an item, comment, link, add to or remove from an
  initiative, create an initiative, file an item's issue, assign an item to its Project Manager.
- Applied operations are recorded in each item's history with the changeset as their source, so one changeset can be
  undone as a whole.

A project's switch for its Project Manager is the one rule that applies changes without review today; rules for other
proposers and trusted kinds of operations are on the roadmap.

The rules are `api/src/action_items/changesets.rs`, and the queries that store, apply, and undo changesets are
`api/src/models/changeset.rs` with `apply.rs` and `undo.rs` beside it.

### Proposing

A changeset has a proposer (`elysia`, `session:<number>`, or `project_manager:<project id>`; email triage and meetings
propose as Elysia), a one-line summary, the project a coding session or a Project Manager proposed it in, and up to 50
operations. Each operation is an object whose
`kind` says what it does, with the reason for it and, optionally, the quote and source it came from:

| `kind`                   | Fields                                                                       |
|--------------------------|------------------------------------------------------------------------------|
| `create-item`            | `title`, and optional `notes`, `priority`, `dueAt`, `projectIds`, `initiatives` |
| `update-item`            | `item`, and any of `title`, `notes`, `priority`, `dueAt` (`null` removes it)  |
| `resolve-item`, `dismiss-item` | `item`                                                                 |
| `comment`                | `item`, `body`                                                               |
| `link`                   | `item`, `target`: `{ provider, credentialId, kind, reference }`, as a link the user adds |
| `add-to-initiative`, `remove-from-initiative` | `item`, `initiative`                                    |
| `create-initiative`      | `name`, and optional `description`, `targetAt`, `projectIds`                 |
| `file-issue`             | `item`, `tracker`: `{ provider, credentialId, destination }`, the project's tracker when proposed |
| `assign-item`            | `item`: assigns it to its lead project's Project Manager; Elysia's alone ([Assigning to a Project Manager](#assigning-to-a-project-manager)) |

An operation may also name the timeline record that caused it (`causeRecordId`); every Project Manager's does, and its
write is recorded on the project's timeline with it when it applies.

An `item` or `initiative` is `{ "id": ... }` for one that exists, or `{ "operation": n }` for the one operation `n` of
the same changeset creates, counting from 1. An operation can only name an earlier create of the right kind, and it
depends on every operation it names. A proposal is checked whole before anything is stored: every field fits its
column, every name points backwards at the right kind, and every existing item and initiative it names is live. One
that fails is refused naming the operation, and nothing is stored.

Items and initiatives a changeset creates start the way the user's own do: an item `open` and owned by the user, since
approving it is accepting it, and an initiative `active`. An item a Project Manager's changeset creates without review
starts in the inbox instead, since nobody accepted it. Nothing creates a changeset over HTTP: Elysia, email triage,
and meetings will call the model's `propose` in-process, as `work_propose_changes` and `manager_make_changes` do
today.

### Deciding

While a changeset is `pending`, each operation's decision is `pending`, `approved`, or `rejected`, and the user changes
any number at once, or all of them. Rejecting an operation rejects every operation that depends on it, directly or
through another, and approving one whose dependency stays rejected is refused. Decisions are stored, so every open
page shows the same review, and they are final once the changeset is applied.

### Applying

Applying needs every operation decided. Approved operations run in order as the proposer, through the same writes the
user's own changes use, each in its own savepoint of one transaction:

- An operation the records refuse, such as resolving an item deleted since it was proposed, is `failed` with the
  reason, and writes nothing. Every operation depending on it is `skipped`, and the rest apply.
- A `link` operation's target is read through its credential before the transaction starts, as a link the user adds
  is. A target that cannot be read fails its operation with the provider's answer.
- Provider writes that follow (closing a resolved item's issues, posting a comment to its primary link) are owed in
  the same transaction and landed by the watcher, which applying wakes. One that fails leaves its link pending with the
  provider's last answer, retried as above; the operation's own change stands. The review reads that from the link's
  owed writes.
- Elysia's link becomes the item's primary when it has none, as the user's first link does. A coding agent's never
  does, so its pull request never moves whose list an item is on. An issue filed for an item becomes its primary
  whoever filed it, since it is the item's own.
- A `file-issue` refuses an item resolved or dismissed, or one already following a linked issue, and owes the filing
  otherwise; once the watcher lands it, the operation's `result` names the link it made, for an undo.
- A changeset with any operation approved ends `applied`; one with every operation rejected ends `rejected` and wrote
  nothing.

Each operation keeps what it did (`result`): the ids it created or acted on, and for an update each field's value
before and after. Every history entry an operation recorded names the changeset (`action_item_events.changeset_id`).

### Undoing

An applied changeset can be undone once, as a whole. Its applied operations are reversed last first, as the user,
and the history entries the undo records name the changeset too:

| Operation            | Undo                                                                               |
|----------------------|------------------------------------------------------------------------------------|
| Create an item       | Deletes it softly, so it can still be restored                                     |
| Update an item       | Puts back each field that still holds what the update wrote                        |
| Resolve or dismiss   | Returns the item to the state it left, the inbox or open, if it has not moved since |
| Comment              | Takes the comment back, whoever wrote it                                           |
| Link                 | Removes the link, if the operation added it                                        |
| Add to or remove from an initiative | Reverses the membership, if the operation changed it                |
| Create an initiative | Deletes it softly                                                                  |
| File an issue        | Drops the filing, or once it was filed, removes the link to the issue              |
| Assign an item       | Takes it back from its Project Manager, if the assignment it made still stands; an inbox item it accepted stays open |

What undo cannot reverse, and says so on the operation (`undo`) for the review to show:

- A comment the watcher already posted to Jira or GitHub stays there (`stillPosted`). One not posted yet is dropped.
- An issue the watcher already filed stays in its tracker (`stillFiled`), and the item stops following it; one not filed
  yet is dropped.
- What a Project Manager already planned for an item it was assigned stays planned (`stillPlanned`): each batch is the
  person's to decide. An item the person took back or assigned again since keeps the person's choice (`movedSince`).
- An issue the watcher already closed stays closed when its item returns (`stillClosed`); a pull request was never
  touched. A close not landed yet is dropped, as for any item no longer resolved.
- A field the user changed after the changeset keeps the user's value (`kept`), and an item the user moved since stays
  where the user put it (`movedSince`).
- An operation the records refuse, such as returning an item deleted since, keeps its effect with the reason
  (`refusal`). The rest are still reversed.

An operation reversed ends `undone`; one that could not be stays `applied` with its `undo` saying why. A membership
change is reversed only while the span it opened or closed is still the item's latest in that initiative, counting a
span the undo itself wrote as the state it restored, so several changes to one membership in a changeset all reverse.

### Reviewing

Changesets are a tab of the Action items area, which counts the ones waiting, and Next leads with a card while any
wait. The review shows each operation as a sentence, an edit field by field from what it holds to what it would hold,
with its reason, quote, and source, and what rejecting it would also reject. After applying, each operation shows
how it ended and any provider write it still owes; after undoing, what undo kept. See `docs/frontend.md`.

## Coding sessions

Coding agents reach action items through one MCP server, `elysium_work` (`api/src/tools/work.rs`), served to the
workspace at `/api/agent-tools/elysium_work` (see `docs/coding.md`). Every turn of a session with a project gets it,
beside `elysium_storage` when the project has a storage location (`docs/storage.md`). It speaks Elysium's terms
(items, initiatives, projects, comments), and never a provider's; the provider behind a link is Elysium's business.
Every call is scoped to the session's project and re-checked against the database, the way storage tools are.

| Tool               | Arguments                                                  | Answers                                                     |
|--------------------|------------------------------------------------------------|-------------------------------------------------------------|
| `work_project`     | none                                                       | The project, its active initiatives, its items in the inbox or open, and `sessionItemId` |
| `work_items`       | optional `states`, `initiativeId`, `waiting`, `search`, `limit` | `{ items, moreItems }`, newest first, without notes, each with its `leadProjectId` and `assignedToManagerAt` |
| `work_item`        | `itemId`                                                   | The item with its notes, comments, latest 50 history entries, projects (the lead marked), and initiatives |
| `work_initiatives` | optional `states`                                          | `{ initiatives }` with progress, by name                    |
| `work_initiative`  | `initiativeId`                                             | The initiative with its description and the project's items in it, up to 200 with `moreItems` |
| `work_comment`     | `itemId`, `body`                                           | `{ comment }`                                               |
| `work_link_pull_request` | `url` of a pull request on github.com                | `{ link }`: its item, key, URL, title, and state             |
| `work_propose_changes` | `summary`, `operations`: each a `change` with its `reason`, `quote`, `source` | `{ changeset, message }`: the staged changeset's id and each operation's position, kind, and dependencies |

- An item or initiative is reachable while it is live and in the session's project, as the database says at the
  moment of the call. Anything else is refused the same way, deleted or elsewhere, pointing the agent at the list
  tool. That holds for an initiative `work_items` filters on too. An initiative's members in other projects are
  counted, not shown; its progress counts them all.
- `work_items` lists items in the inbox or open unless asked for other states, at most 50 unless `limit` says
  otherwise, up to 200. `search` matches the title or notes, ignoring case.
- The agent reads items and initiatives in its project and comments on its project's items. A comment is written as
  `session:<number>`, recorded in the item's history, published on the event stream, and posted to the item's primary
  link like the user's; only its author may change it, so the user cannot rewrite it either. It cannot resolve or
  delete anything directly.
- Any other change the agent wants it proposes with `work_propose_changes`, as a changeset proposed by
  `session:<number>` for the user's review ([Changesets](#changesets)). The operations are the changeset's own, in
  Elysium's terms, with one difference: a pull request is linked by its URL (`link-pull-request`), read through the
  session's GitHub token when proposed and stored as a `link`, and no other link can be proposed. Everything it names
  must be live and in the session's project, and what it creates joins that project; `projectIds` are refused. The
  answer tells the agent that nothing changes until the user approves, and it is not told when they do.
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

## Follow-ups

An item can be the follow-up a project's Project Manager drafted to keep a promise the person made ("I'll let you know
here when it's live"), once the work it waited on was done (`docs/engine.md`, Follow-ups). It starts in the inbox, owned
by the person, created as `project_manager:<project id>`, titled with who to tell and what, with the promise quoted in
its notes, and its page carries the drafted message with Send, Edit, and Dismiss. Sending resolves it as the person;
dismissing it, from anywhere an item is dismissed, marks the promise dismissed; resolving it by hand keeps the promise.
An item a promise waits on is its work: resolving it, by the person or by the watcher reading its issue closed or its
pull request merged, makes the promise due (`crate::models::promise::follow_item`, in `action_item::transition`'s
transaction).

## Roadmap

- Email: completing an item that came from an email marks the thread read, then archives or deletes it; which of the
  two is not decided.
- Email triage: Elysia reads the mail connectors ingest (`docs/connectors.md`) and proposes what to do with each
  item as a changeset. Today a new unread message raises an inbox item by itself, recorded as `watcher:<service>`, and
  once its conversation goes quiet, the agent handling it raises an inbox item as `elysia` for each question or
  request the person owes (`docs/engine.md`, Conversations).
- Meetings: recordings from Slack, Teams, and others are transcribed and uploaded, and Elysia proposes one changeset
  per meeting, citing the transcript.
- Rules that apply trusted operations without review, loosened as Elysia's track record earns it.
- Project memory that coding agents and Elysia read and write, so a session starts with what earlier sessions learned.
- Users and authentication, turning `owner`, `waitingOn`, and actors into references and making Next per user.
- Webhooks for Jira and GitHub, beside polling.
- Notification emails from Jira and GitHub folded into the linked issue's item.
- The linked issues' descriptions in the first turn of a session started from an item.
- Editing or deleting a comment carried to the comment already posted to the provider.
- Attributing several arrived issues in one turn. Each assigned issue is one run and one segmenting turn today, so a
  first connect that brings in hundreds spends a turn on each.
