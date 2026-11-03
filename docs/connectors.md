# Connectors and the timeline

Elysium reads the services its people use, as those people, and puts what it reads on one timeline. Two things make
that up, kept apart on purpose:

- An **account** is how Elysium signs in to a provider as a person: an Atlassian or GitHub token, an API key, or a
  browser the person signed in to.
- A **connector** is one use of one account: a kind, what it reads, and what its reading has found. One account can
  feed several connectors, each kind once: an Atlassian account feeds Jira and Confluence, and a Microsoft browser can
  feed Outlook and, later, Teams.

The code is `api/src/connectors/`; the routes are `/api/v1/connectors` and `/api/v1/accounts` (`docs/api.md`).

## Accounts

Accounts live in one store per way of signing in, each with the columns and constraints its provider needs:

| Account kind | Store                | Signs in with                              | Whose                |
|--------------|----------------------|--------------------------------------------|----------------------|
| `atlassian`  | `jira_credentials`   | A site, an account email, and an API token | The workspace's      |
| `github`     | `github_credentials` | A personal access token                    | The workspace's      |
| `browser`    | `browser_sessions`   | A browser the person signed in to          | Its creator's        |
| `fireflies`  | `token_accounts`     | An API key                                 | Its creator's        |
| `youtrack`   | `token_accounts`     | A site and a permanent token               | Its creator's        |

The stores stay separate rather than becoming one table because each holds what only its provider has: Jira's site
rule and allowlist, GitHub's default token and token kinds, a browser's port and container. They are also what coding
sessions, projects, action item links, and the link watcher already point at by foreign key. One table would trade
those constraints for a blob and move every reference for no change in behavior. `api/src/connectors/accounts.rs`
reads them together for the hub's Accounts list.

`token_accounts` holds every account whose whole sign-in is one API key, named by its provider (`kind`): Fireflies and
YouTrack today, and the providers on the roadmap that work the same way. A key is checked with its provider before it
is stored, sealed bound to its row (`docs/secrets.md`), and never returned. `identity` keeps who the provider said the
key belongs to: an address for Fireflies, the login for YouTrack, which the reader compares authors against.

A provider whose accounts each name their own instance keeps it in `site_url`. YouTrack's is the site the token was
issued on, and like Jira's site it is validated rather than trusted (`crate::youtrack::normalize_site_url`): `https`,
a host of one label on `youtrack.cloud`, no path, port, query, or credentials, stored as the origin alone. So no stored
account can point Elysium at another host. YouTrack Server, on a customer's own host, is not supported.

## Connectors

`connectors` (`docs/database.md`) holds one row per connector. Exactly one of its four account columns is set
(`browser_session_id`, `token_account_id`, `jira_credential_id`, `github_credential_id`), which its kind decides, and an
account feeds each kind at most once. Removing an account removes its connectors.

A connector's **content** is what it reads, in its kind's own terms. A kind keeps it in `content` unless it needs a
table of its own:

- Jira's content is its Atlassian account's allowlist, which stays on the account (`jira_credential_projects`,
  `jira_credential_boards`): every Jira call made with that token is bounded by it (`docs/jira.md`).
- Confluence's content is the spaces it reads, in `content` as `{"spaces": "*"}` or
  `{"spaces": [{"id", "key", "name"}]}`, each as Confluence reported it when it was picked. It is the connector's own,
  not the account's: the same Atlassian account's Jira allowlist does not bound it, nor it the allowlist.
- Fireflies' content is the storage location its recordings are kept in, `storage_location_id`, a foreign key.
- YouTrack's content is the projects it reads, `{"projects": "*"}` or `{"projects": [<project id>, ...]}`. A pick may
  name a project by its id or short name; what is stored is YouTrack's id, matched against what the token reaches
  when it is saved, so a project the token cannot see is refused then rather than ignored later.

Its **reading state** is `sign_in_state`, `cursor`, `last_polled_at`, and `last_error`, and `is_active` false means
paused. Jira and GitHub are read by the link watcher, which records its progress per account in `link_watch_cursors`,
so their last sync and last error come from there.

Every Atlassian and GitHub account carries exactly one connector of its kind, made with it in the same transaction
(`jira_credential::create`, `github_credential::create`). Links can be made through any such account and the watcher
reads through each, so the connector is where that reading is shown, checked, and paused. A new Jira or GitHub
connector is therefore always a new account, and removing one removes its account, and with it any Confluence
connector reading through that account.

Because an Atlassian account also feeds Confluence, anything about the link watcher's reading is matched by kind as
well as account (`ConnectorKind::LINKS`, Jira and GitHub): which accounts the paused connectors stop the watcher on,
whose watcher pass and errors a connector shows, and whose linked issues it counts. Pausing Confluence never pauses
the watcher, and Confluence never shows Jira's pass or count.

### Who sees what

A kind is the workspace's or a person's. Jira and GitHub are the workspace's, like their accounts: every member sees,
checks, pauses, and edits them. Mail and meetings are personal: only their creator sees them, and anyone else is
answered `404`.

## Kinds are defined once

Each kind is one module under `api/src/connectors/kinds/` that declares its `Definition` and fills the hub's verbs for
it. `kinds/mod.rs` is the one place that matches on the kind; routes and the web app never do.

| Kind         | Module                 | Reads through | Accounts | Content                      | Read by                     |
|--------------|------------------------|---------------|----------|------------------------------|-----------------------------|
| `jira`       | `kinds/jira.rs`        | `atlassian`   | New      | `projects`, `boards`         | The link watcher            |
| `confluence` | `kinds/confluence.rs`  | `atlassian`   | Existing | `spaces`                     | `crate::confluence::reader` |
| `github`     | `kinds/github.rs`      | `github`      | New      | None: what the token reaches | The link watcher            |
| `outlook`    | `kinds/webmail.rs`     | `browser`     | Existing | None: the inbox              | `ingest`                    |
| `gmail`      | `kinds/webmail.rs`     | `browser`     | Existing | None: the inbox              | `ingest`                    |
| `teams`      | `kinds/teams.rs`       | `browser`     | Existing | `conversations`              | `ingest`                    |
| `fireflies`  | `kinds/fireflies.rs`   | `fireflies`   | Either   | `storageLocationId`          | `crate::meetings`           |
| `youtrack`   | `kinds/youtrack.rs`    | `youtrack`    | Either   | `projects`                   | `crate::youtrack::reader`   |

A definition is structure only: the kind's scope, the account kind it reads through, which accounts a new connector
takes (`accounts`: a new one made with it, one the person already has, or either), a browser kind's start page, its
content fields (a `selection` picked from what the account reaches, or a `storage_location`), and its ordered checks.
Account definitions say what a new account asks for (text, email, URL, secret, or a choice). `GET /connectors/kinds`
serves both, and the web app draws its forms and check lists from them, naming every id from its `connectors`
translations.

A kind fills these verbs:

| Verb            | What it does                                                                                |
|-----------------|---------------------------------------------------------------------------------------------|
| Checks          | Builds the kind's checks against a stored connector or one being set up                      |
| Create          | Makes the connector, and a new account when one is given, checked with its provider first    |
| Options         | Lists what each selection field can pick from, through an existing account or a new one      |
| Update content  | Replaces what it reads, checked the way a new connector's content is                         |
| Sync            | Reads it now, in the loop that reads its kind                                                |
| Remove          | Removes it, and for Jira and GitHub its account                                              |

Jira and GitHub create their accounts through the same functions as their settings routes
(`create_jira_credential::create_credential`, `create_github_credential::create_credential`), so both check exactly the
same things.

### Adding a kind

A new kind adds a `ConnectorKind`, a module with its definition and verbs, a row in `DEFINITIONS` and in each dispatch
in `kinds/mod.rs`, its name to the web app's `CONNECTOR_KINDS`, and its names and check sentences to the web app's
`connectors` translations. Its definition's `accounts` decides what the wizard offers, so no page names the kind.
Check ids share one set of labels and count sentences across kinds, so a check that counts something different takes
an id of its own: Confluence's `sample_page_comments` beside YouTrack's `sample_comments`. A kind whose account is
a new provider adds an `AccountKind` and an `AccountDefinition`; when its sign-in is one API key, its key lives in
`token_accounts`. `kinds/mod.rs`'s tests fail when a kind has no definition, an account kind none, or a check depends
on one that does not run before it.

## Checks

A check proves one thing a connector needs, such as signing in or reaching its allowlist. Each kind declares its checks
in order (`CheckSpec`): an id, whether the connector is any use without it, and the checks it builds on.

| Kind              | Checks, in order                                                                                  |
|-------------------|----------------------------------------------------------------------------------------------------|
| Jira              | `sign_in`, `list_projects`, `list_boards` (optional), `allowlist_reachable`, `sample_issues`        |
| Confluence        | `sign_in`, `list_spaces` (also proves every chosen space is reached), `sample_pages`, `sample_page_comments` (optional) |
| GitHub            | `sign_in`, `read_scopes` (optional), `list_repositories`, `sample_issues` and `sample_pull_requests` (optional) |
| Outlook and Gmail | `browser_running`, `signed_in`, `inbox_readable`, `sample_message` (optional)                       |
| Teams             | `browser_running`, `signed_in`, `chats_readable`, `sample_message` (optional)                       |
| Fireflies         | `sign_in`, `list_meetings`, `sample_transcript` (optional)                                         |
| YouTrack          | `sign_in`, `list_projects`, `sample_issues`, `sample_comments` (optional)                          |

`checks::run_in_order` runs them one at a time against one value, so a later check uses what an earlier one fetched.
A check whose dependency did not pass is skipped and names that dependency (`blockedBy`), so a token Jira refuses fails
`sign_in` once rather than every check after it. An outcome is passed, failed, or skipped, with a `count` its sentence
fills, a `reason` in Elysium's own terms (`no_messages`, `fine_grained_scopes`), and the provider's own `message`.

Checks run on demand (`POST /connectors/{id}/checks`) and before a connector is saved (`POST /connectors/checks`, with
the same `{ kind, account, content }` a create takes; nothing is stored, and a new account's secret reaches its
provider and no further). A run lives in memory (`CheckRuns`) for half an hour after it ends:

- Its progress is published as `connectorCheckRun.updated`, ids and states only, since every client hears every event
  and a check's message can name what an account reaches.
- `GET /connectors/check-runs/{runId}` answers the run, messages and all, to the person who started it.
- A stored connector's run replaces each check's last result (`connector_check_results`), so the hub shows them after a
  reload.

## Sync status and activity

`api/src/connectors/activity.rs` is where every loop that reads a connector reports its syncs. A sync that recorded
something, or failed, is stored as a run (`connector_runs`): records stored for a mailbox, meetings listed or
processed, changes the watcher read. A sync that found nothing new is not stored, since a mailbox is read every minute.
Each connector keeps its newest 50 runs.

A connector's status is, in order:

| Status    | When                                                                  |
|-----------|-----------------------------------------------------------------------|
| `paused`  | `is_active` is false                                                  |
| `syncing` | A sync someone asked for is running, or one of its meetings is being processed |
| `error`   | Its last read failed, or its account is asking to be signed in again  |
| `active`  | Otherwise                                                             |

`syncing` is kept in memory, since it is only ever true while this process is working on it. A paused Jira or GitHub
connector's account is skipped by the link watcher's reads; writes the user already approved still land. A paused
mailbox or meeting source is not read.

`POST /connectors/{id}/sync` reads a connector now, which is also how a failed sync is retried: a mailbox is read once,
a meeting source lists and processes, and Jira and GitHub wake the watcher.

## In the web app

Settings, Connectors (`/settings/connectors`, `frontend/src/pages/Settings/Connectors/`) is the hub, modeled on Onyx's
connectors:

- **The list** shows every connector the person sees with its kind, account, status, last sync, records, and last
  error, and a gear menu to run its checks, sync it now, pause or resume it, and delete it. A search over kind and
  account names and a status filter narrow it. Above it, a summary and the activity feed of recent syncs, each failed
  one with a retry.
- **Accounts** lists every account with what it feeds; Atlassian, GitHub, and browser accounts are edited on their own
  pages, and an API key's account is removed here.
- **Adding** a connector (`/settings/connectors/new`) picks a kind, then an account the person has or a new one, as
  the kind's `accounts` allows (`offeredAccounts`), then its content, then runs its checks live before it is saved. A
  kind that only reads through an existing account, with none yet, says how to get one: Confluence's Atlassian
  account comes with a Jira connector. A required check that did not pass leaves the save available as "Save anyway".
- **A connector's page** (`/settings/connectors/:connectorId`) shows its status, last error, checks (stored, or a run
  started there as it goes), and content to change, with links to its account's own page.

Every form and check list is drawn from `GET /connectors/kinds`: `AccountFields` for a new account's fields,
`ContentFields` for content (a selection loads its options from `POST /connectors/options` on request), and `CheckList`
for checks. Names, check labels, count sentences, and reasons come from the `connectors` translations, by id.
Connectors reach Redux through `useConnectorsLoader` and stay current through `connector.changed`, each loaded by id;
a followed check run moves along on `connectorCheckRun.updated` and is loaded again when it finishes, for its messages.

Settings, GitHub and Settings, Jira lead to the hub; a GitHub or Jira account's edit page and Jira's done statuses
stay where they were.

## Ingest

`api/src/connectors/ingest.rs` reads every active connector read through a browser (`ConnectorKind::BROWSER_READ`:
Outlook, Gmail, and Teams) once a minute (`INGEST_INTERVAL`), until shutdown; meeting connectors have their own loop
(`docs/meetings.md`).

Each browser kind has a reader behind one trait, `BrowserReader`, and `api/src/connectors/readers.rs` is the one place a
kind meets its reader (`for_kind`). A reader is given its browser, what it kept from its last read, and its connector's
content (`ReadInput`), and answers a reading: the sign-in state, the items it found, and optionally its state for the
next read, such as a watermark. Ingest keeps that state in the connector's cursor beside the baseline,
`{ "baselined": true, "reader": { ... } }`; a reading with no state keeps the old one. A reader whose service has no
tab open may open one (`Devtools::open_tab`). A new browser kind adds its reader, its row in `for_kind`, and itself to
`BROWSER_READ`, and a test fails when any of the three is missing.

`api/src/connectors/webmail.rs` reads a mailbox through its browser. Each service is one row of `SERVICES`:

| Service | Mailbox addresses                                             | Sign-in addresses                          |
|---------|---------------------------------------------------------------|--------------------------------------------|
| Outlook | `outlook.office.com/mail`, `outlook.office365.com/mail`, `outlook.live.com/mail` | `login.microsoftonline.com`, `login.live.com` |
| Gmail   | `mail.google.com/mail`                                        | `accounts.google.com`                      |

A read lists the browser's tabs over DevTools. A tab on a sign-in address and none on the mailbox means the session
ended (`signed_out`); no tab on either is `unknown`. Otherwise the service's script runs in the mailbox tab, as the
page, and answers `{ signedIn, messages: [{ id, from, subject, preview, unread, receivedAt?, url? }] }`. The script
reads the message list and never opens a message, so reading leaves unread mail unread. The check is cheap and
deterministic; reading closely and acting are Elysia's to do through the same browser.

Outlook's script (`api/src/connectors/outlook.js`) reads each conversation by its `data-convid`, its sender, subject,
and preview from the row's lines of text, the sender's address from a `title`, and unread from the row's
`aria-label`. It was checked against a fixture shaped that way, not yet against a signed-in Outlook.

Gmail's script (`api/src/connectors/gmail.js`) reads the inbox's unread mail from the Atom feed Gmail publishes for
the account the tab shows (`/mail/u/<n>/feed/atom`), fetched from the tab so it carries the tab's own session: each
entry's message id, sender, subject, summary, time, and link. Every entry is unread by definition, and a `401` means
the session ended. A date that does not parse is treated as absent, so one odd entry never fails a read. It was
checked against a feed shaped like Gmail's, served locally, not yet against a signed-in Gmail.

### Teams

`api/src/connectors/teams.rs` reads Microsoft Teams through a browser signed in to Microsoft. The same browser can
feed Outlook and Teams: Microsoft's sign-in carries over, so a browser signed in for Outlook opens Teams without
asking again. When no tab shows Teams (`teams.cloud.microsoft`, `teams.microsoft.com`, or `teams.live.com`) and none
is on a sign-in page, the read opens Teams in a new tab over DevTools and answers `unknown`; the next read finds it
loaded. A tab on a sign-in page means the session ended (`signed_out`). Nothing is installed into the organization.

**It reads Teams' own local store, not the page.** New Teams keeps the chats and channels it has loaded in
IndexedDB, in databases named `Teams:conversation-manager:...:<user id>:<tenant>:<locale>` and
`Teams:replychain-manager:...`. `api/src/connectors/teams.js` runs as the page and reads them in read-only
transactions. Those records carry what the rendered page shows only in part, and in markup every redesign changes:
each message's id, sender, arrival time, and mentions, and each conversation's read horizon. Reading them opens no
chat, so unread messages stay unread. The store is the web client's own, not a published API, so the script reads its
fields defensively. When it finds no store it says so (`chats_readable` fails as `teams_store_not_found` and its message
lists the databases the page does hold), so a change to the layout shows up in the checks rather than as silence.

The script only reads. The connector decides what the records mean:

- **What it reads.** Its content is `conversations`, `"*"` for every chat and channel the person sees (the default)
  or a list of conversation ids, picked from what the browser's Teams has stored (`POST /connectors/options`).
- **What is new.** Its reader state is a watermark, `{ "since": <ms> }`, the newest arrival it has read. Each read
  asks for messages after it, less ten minutes in case a message was stored late, and records are deduplicated by
  `<conversation id>/<message id>`, so a message read twice is stored once. A read answers at most 200 messages, newest
  first; a larger backlog is read over the next passes.
- **What wants attention.** A message is unread when it arrived after its conversation's read horizon (the first
  field of `consumptionhorizon`; a conversation without one counts as unread). An unread message from someone else
  raises an inbox item when it is in a chat (one-on-one, group, or a meeting's chat), mentions the person, or replies in
  a channel thread the person wrote in. Other channel messages are recorded without raising anything, and the person's
  own messages are recorded too. System events (members added, calls) are not recorded.
- **Each record** is titled with its conversation (a channel's or group's topic, or for a chat without one, who else
  wrote in it), comes from its sender, holds the message as plain text (the browser parses its HTML), and links to it
  with Teams' documented message link, `https://teams.microsoft.com/l/message/<conversation>/<message>`.

The script was checked against `api/src/connectors/fixtures/teams.html`, a page that seeds IndexedDB the way Teams lays
it out, in headless Chromium (`fixtures/evaluate.mjs` runs any page script against a fixture). Its answer,
`fixtures/teams_page.json`, is what `teams.rs`'s tests read, so the rules above are tested against the script's real
output. Neither has yet read a signed-in Teams.

### On each read

- Each item becomes a timeline record. An item its connector already recorded is skipped, by the connector and the
  provider's own id, so reading the same mailbox again records nothing twice.
- Each new item that wants attention (an unread message) raises an inbox action item owned by the person, in the
  same transaction as its record, which points at it (`action_item_id`). The item is recorded as the watcher of
  that service, `watcher:outlook`: like the link watcher, it records what already happened in a provider, and the
  person consented when they connected the mailbox.
- The connector's first signed-in read records what the mailbox already holds without raising anything, so
  connecting a mailbox does not fill the inbox with its backlog. Its cursor remembers that (`{"baselined": true}`).
- A read that fails keeps the cursor and records the reason in `last_error`.

### Confluence

`api/src/confluence/` is the only caller of Confluence Cloud. It reaches the Atlassian account's own site
(`https://<site>.atlassian.net/wiki`) with the account's email and API token, the same basic auth Jira takes, through
the account's stored, validated origin. Confluence Data Center is not supported.

| Call                                  | Used for                                                     |
|---------------------------------------|--------------------------------------------------------------|
| `GET /wiki/rest/api/user/current`     | `sign_in`: whose token it is                                 |
| `GET /wiki/api/v2/spaces`             | What spaces can be picked from, and `list_spaces`; cursor paged, 2,500 at most |
| `GET /wiki/rest/api/content/search`   | Every read of content: pages and comments by CQL, bodies expanded |

**Bounded to the chosen spaces, in one place.** `Confluence::changes` is the only call that reads content, and it takes
the connector's `SpaceScope`. The scope writes the CQL's `space in (...)` clause, with every key quoted and escaped,
and every result is checked against it again on the way back, so one from another space is logged and dropped rather
than recorded. An empty pick sends no search at all. Listing spaces is what a pick is made from, so it is not bounded.
A next page is followed only when Confluence's `_links.next` is a further page of the same listing or search.

**Reading.** `crate::confluence::reader` reads every active Confluence connector every five minutes (`READ_INTERVAL`),
until shutdown, and on a sync someone asks for:

- A read asks for pages and comments changed since the last read ended, plus a five minute overlap, oldest first. The
  window is relative, `lastmodified >= now("-Nm")`, because CQL reads a written date in the account's own time zone.
- Each change is one record, keyed by its content and version (`page:<id>:<version>`, `comment:<id>:<version>`), so
  every edit is on the timeline once and the overlap records nothing twice. `metadata` keeps the type, id, version,
  space, and a comment's page.
- A read records at most 500 changes; one that stops there ends at its last change, and the next continues from it.
  The cursor is `{"baselined", "through"}`.
- A new connector's first reads look back seven days and raise nothing, as a new mailbox's do. It is baselined once a
  read reaches the end of what matched.
- A read looks back a month at most, so a connector paused for a long time catches up on the last month.
- `401` and `403` read as `signed_out`; anything else that fails keeps the cursor and records the reason.

**What raises an inbox item.** A change by someone else that mentions the account's person (`ri:user` with their
account id), or a comment with a question mark on a page the person created, raises an inbox item for them, recorded as
`watcher:confluence`, in the same transaction as its record. A page or comment raises one at most once, however often
it is edited (`timeline_record::has_raised_item`). Everything else is left to Elysia's triage on
`timelineRecord.created`. The person is the one the Atlassian account's token belongs to (`jira_credentials.account_id`).

**Rendering.** Bodies arrive in storage format, XHTML with Confluence's `ac:` and `ri:` elements, and the record keeps
plain text (`crate::confluence::storage`, at most 20,000 characters):

- Paragraphs, headings, table rows, tasks, and macros are blocks, separated by one blank line. A list item starts with
  `- `, `br` is a line break, and the cells of a row are joined by ` | `.
- Runs of whitespace collapse to one space, except in preformatted text (`pre`, a macro's `ac:plain-text-body`), which
  keeps its lines.
- A mention reads as `@user`. A link to a page reads as that page's title, and a link's own text as written.
- Macro parameters, placeholders, images, and emoticons are dropped.
- Character references are decoded, numeric ones and the named ones storage format writes; an unknown one is kept.

None of these calls has been made against a live site yet: the shapes come from Atlassian's documentation, and
`api/src/confluence/tests.rs` proves Elysium's half against a fake shaped the same way. A scoped Atlassian API token
(the long kind) is refused on the site's own host for Confluence and works only through
`api.atlassian.com/ex/confluence/<cloudId>` (`~/.claude/learnings/jira.md`), which Elysium does not call yet; use a
classic token.

### YouTrack

`api/src/youtrack/reader.rs` reads every active YouTrack connector every two minutes (`READ_INTERVAL`), until
shutdown. A read signs in as the token's user, lists the chosen projects' issues most recently updated first
(`project: JL, WAY sort by: updated desc`, up to four pages of 50), stops at the cursor, and reads the comments on each
changed issue.

- A new issue is recorded as `issue:<id>`, titled with its key and summary, from its reporter.
- Each new comment is recorded as `comment:<id>`, titled `Comment on <key> <summary>`, linking to the comment itself.
- Other changes, such as a state moving, are not recorded: following an issue's state is what linking it to an item
  is for.

A record that wants attention raises an inbox item recorded as `watcher:youtrack`, in the same transaction:

| Record    | Wants attention when                                                                       |
|-----------|--------------------------------------------------------------------------------------------|
| An issue  | It is assigned to the token's user, and someone else reported it                          |
| A comment | Someone else wrote it, on an issue the user is assigned or reported, or it mentions `@<login>` |

The cursor is the newest update read, in YouTrack's own clock (`{"baselined": true, "since": <ms>}`). The first read
records nothing and only sets it, so connecting a site does not fill the timeline and the inbox with its history. A
read the site refuses marks the connector `signed_out`; any other failure keeps the cursor and records the reason.

## What is published

| Event                        | When                                                                          |
|------------------------------|-------------------------------------------------------------------------------|
| `connector.changed`          | A connector was added or changed: paused, its content, a sync starting or ending, a read finding something, its checks. Its id alone |
| `connector.deleted`          | A connector was removed, or the account it read through                       |
| `connectorCheckRun.updated`  | A check in a run moved on, or the run ended (`checkId` null, `state` `finished`) |
| `timelineRecord.created`     | A record was stored: its id, kind, owner, and the item it raised. Never its content |
| `actionItem.upserted`        | The item a record raised, like any item write                                 |

`connector.changed` carries the id alone because a connector's view names whose mailbox or meetings it reads and every
client hears every event; clients load it by id, which answers only those who may see it.

`timelineRecord.created` is the seam Elysia's triage builds on: her dispatcher hears it in process and loads the
record by id.

## The timeline

`timeline_records` holds everything ingested: its kind, the connector it came from, the provider's id, when it
happened, who it came from, a title, a plain-text body, a link, optionally a span inside a recording (`media`, for a
meeting segment), and free `metadata`. A record belongs to whoever it was ingested for, and only they list it: a
person's mail is theirs even when it is about a shared project.

`timeline_record_projects` attributes records to projects, each with a confidence and who attributed it. A person's
attribution has a confidence of 1 and replaces whatever was guessed for that project; Elysia's carry her
confidence.

`GET /api/v1/timeline` lists the caller's records newest first, optionally for one project and before a time; `PUT`
and `DELETE /api/v1/timeline/{id}/projects/{projectId}` attribute and unattribute one. See `docs/api.md`.

## Roadmap

- More kinds: Slack, and Google Meet and Teams recordings (`docs/meetings.md`).
- Confluence through Atlassian's gateway (`api.atlassian.com/ex/confluence/<cloudId>`), so scoped API tokens work.
- Confluence attachments and inline comments, and a mention found by display name as well as account id.
- Teams: confirming the store's layout against a signed-in Teams, and how much of it Teams keeps. The connector reads
  only what Teams has stored; if Teams stores a channel's messages only after showing that channel, a channel never
  opened in that browser is not read. Not yet confirmed either way.
- YouTrack issues as action item links, through the link-provider trait (`api/src/action_items/links/`), as Jira and
  GitHub issues are: resolving an item moves its issue to a resolved state, and resolving the issue resolves the item.
- YouTrack Server, on a customer's own host, beside YouTrack Cloud.
- GitHub content: picking repositories rather than reading whatever the token reaches.
- Keeping a token account's `identity` and `checked_at` current from its checks.
- Elysia's triage on `timelineRecord.created`: classifying each message, drafting replies for review, and attributing
  records to projects.
- History for attributions, so a correction can be undone like any other edit.
