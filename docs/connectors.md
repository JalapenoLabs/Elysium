# Connectors and the timeline

Elysium reads the services its people use, as those people, and puts what it reads on one timeline. A **connector**
is one service read as one person: a kind, the sign-in it owns, what it reads, and what its reading has found.

- Every connector owns exactly one **account**, which is how Elysium signs in to the provider: an Atlassian or GitHub
  token, an API key, or a browser its person signed in to. No account is shared between connectors: Jira and
  Confluence each hold their own Atlassian token, and Outlook and Teams each have their own browser.
- Adding a connector signs in, and removing one removes its account: its token is forgotten, or its browser is
  removed with the profile its person signed in on.

The code is `api/src/connectors/`; the routes are `/api/v1/connectors` (`docs/api.md`).

## Sign-ins

Accounts live in one store per way of signing in, each with the columns and constraints its provider needs:

| Account kind | Store                | Signs in with                              | Owned by                         |
|--------------|----------------------|--------------------------------------------|----------------------------------|
| `atlassian`  | `jira_credentials`   | A site, an account email, and an API token | A Jira or Confluence connector   |
| `github`     | `github_credentials` | A personal access token                    | A GitHub connector               |
| `browser`    | `browser_sessions`   | A browser the person signed in to          | An Outlook, Gmail, Teams, or Slack connector |
| `fireflies`  | `token_accounts`     | An API key                                 | A Fireflies connector            |
| `youtrack`   | `token_accounts`     | A site and a permanent token               | A YouTrack connector             |

The stores stay separate rather than becoming one table because each holds what only its provider has: Jira's site
rule and allowlist, GitHub's default token and token kinds, a browser's port and container. They are also what coding
sessions, projects, action item links, and the link watcher already point at by foreign key. One table would trade
those constraints for a blob and move every reference for no change in behavior. `api/src/connectors/accounts.rs`
names each connector's account beside it.

`token_accounts` holds every account whose whole sign-in is one API key, named by its provider (`kind`): Fireflies and
YouTrack today, and the providers on the roadmap that work the same way. A key is checked with its provider before it
is stored, sealed bound to its row (`docs/secrets.md`), and never returned. `identity` keeps who the provider said the
key belongs to: an address for Fireflies, the login for YouTrack, which the reader compares authors against.

A provider whose accounts each name their own instance keeps it in `site_url`. YouTrack's is the site the token was
issued on, and like Jira's site it is validated rather than trusted (`crate::youtrack::normalize_site_url`): `https`,
a host of one label on `youtrack.cloud`, no path, port, query, or credentials, stored as the origin alone. So no stored
account can point Elysium at another host. YouTrack Server, on a customer's own host, is not supported.

An Atlassian account carries exactly one connector, made with it in the same transaction
(`jira_credential::create_carrying`). Jira's is the account its allowlist lives on, and every Jira route, link, and
watcher pass uses. Confluence's signs in with the same fields but allows no Jira project or board, so its token never
reaches Jira through Elysium, and `jira_credential::list`, which every Jira picker and Settings, Jira read, answers
Jira's accounts alone. A GitHub account likewise carries its one GitHub connector.

A browser is started with its connector, at its kind's start page (`kinds::browser`), and its person signs in on the
stream the connector's page shows (`docs/browser.md`). A browser per connector means the eight media ports the host
streams on are eight browser connectors at most; a new one past that answers `409` (`browser_ports_exhausted`).

### A connector waiting for its browser

An Outlook, Gmail, Teams, or Slack connector can be without its browser, waiting. It reads as signed out and nothing
reads it, its cursor kept so nothing is recorded twice, and its page offers Start browser
(`POST /connectors/{id}/sign-in`), after which its person signs in. A new connector always starts with its browser,
so only a connector that shared its browser with another kind is ever waiting: the migration that gave every
connector its own account (`2026-11-03-210000-0000_give_every_connector_its_own_sign_in`) left each shared browser with
the connector its start page is for, since a profile cannot be copied. It gave Confluence a copy of its Jira
connector's Atlassian account, re-sealed by the API (`docs/secrets.md`), and its comments say what else it does.

## Connectors

`connectors` (`docs/database.md`) holds one row per connector. Exactly one of its four account columns is set
(`browser_session_id`, `token_account_id`, `jira_credential_id`, `github_credential_id`), which its kind decides, and
each is unique, so no two connectors share an account. Removing an account removes its connector.

A connector's **content** is what it reads, in its kind's own terms. A kind keeps it in `content` unless it needs a
table of its own:

- Jira's content is its Atlassian account's allowlist, which stays on the account (`jira_credential_projects`,
  `jira_credential_boards`): every Jira call made with that token is bounded by it (`docs/jira.md`).
- Confluence's content is the spaces it reads, in `content` as `{"spaces": "*"}` or
  `{"spaces": [{"id", "key", "name"}]}`, each as Confluence reported it when it was picked.
- Fireflies' content is the storage location its recordings are kept in, `storage_location_id`, a foreign key, and
  `{"waitForApproval": bool}`, whether each meeting's run waits for its person to approve the corrected transcript
  (`docs/meetings.md`, Correction). Every kind that provides meetings declares that toggle; a test fails on one that
  does not.
- Teams' content is the conversations its chat reader records, `{"conversations": "*"}` or a list of ids, and, since
  it is a meeting source too, `{"readMeetings": bool, "waitForApproval": bool}` and the storage location its
  recordings are kept in, `storage_location_id`. Its meetings are read only while `readMeetings` is on (it starts on)
  and once a storage location is picked ([Teams](#teams)).
- GitHub's content is the repositories whose assigned issues come in (`docs/github.md`, Assigned issues),
  `{"repositories": "*"}` or `{"repositories": ["owner/name", ...]}`, matched against what the token lists when it is
  saved and stored as GitHub spells each name. A token added under Settings, GitHub reads every repository. It bounds
  what comes in, not what can be linked.
- YouTrack's content is the projects it reads, `{"projects": "*"}` or `{"projects": [<project id>, ...]}`. A pick may
  name a project by its id or short name; what is stored is YouTrack's id, matched against what the token reaches
  when it is saved, so a project the token cannot see is refused then rather than ignored later. It bounds both what
  its reader records and which assigned issues come in.
- Teams' and Slack's content is the conversations they read, `"*"` when they are made, since nothing can be listed
  until their person signs in, and narrowed on the connector's page afterwards. Slack's also holds
  `{"readMeetings": bool, "waitForApproval": bool}`: its huddles are read as meetings while `readMeetings` is on (it
  starts on), with no storage location, since a huddle keeps no recording ([Slack](#slack)).

Its **reading state** is `sign_in_state`, `cursor`, `last_polled_at`, and `last_error`, and `is_active` false means
paused. Jira and GitHub are read by the link watcher, which records its progress per account in `link_watch_cursors`,
so their last sync and last error come from there.

Confluence's account is an Atlassian one too, so anything about the link watcher's reading is matched by kind as well
as account: which accounts the paused connectors stop the watcher on, by the kinds that provide issues (Jira, GitHub,
and YouTrack, whose links and assigned issues the watcher reads); and whose watcher pass and errors a connector shows,
and whose linked issues it counts, by `ConnectorKind::LINKS` (Jira and GitHub, whose reading state is the watcher's).

### Who sees what

A kind is the workspace's or a person's. Jira, Confluence, and GitHub are the workspace's: every member sees, checks,
pauses, and edits them. Mail, chat, and meetings are personal: only their creator sees them, and anyone else is
answered `404`.

## Kinds are defined once

Each kind is one module under `api/src/connectors/kinds/` that declares its `Definition` and fills the hub's verbs for
it. `kinds/mod.rs` is the one place that matches on the kind; routes and the web app never do.

| Kind         | Module                 | Provides   | Signs in with | Content                      | Read by                     | Replies |
|--------------|------------------------|------------|---------------|------------------------------|-----------------------------|---------|
| `jira`       | `kinds/jira.rs`        | `issues`   | `atlassian`   | `projects`, `boards`         | The link watcher            | Never   |
| `confluence` | `kinds/confluence.rs`  | `pages`    | `atlassian`   | `spaces`                     | `crate::confluence::reader` | Never   |
| `github`     | `kinds/github.rs`      | `issues`   | `github`      | `repositories`               | The link watcher            | Never   |
| `outlook`    | `kinds/webmail.rs`     | `mail`     | `browser`     | None: the inbox              | `ingest`                    | From a mailbox |
| `gmail`      | `kinds/webmail.rs`     | `mail`     | `browser`     | None: the inbox              | `ingest`                    | From a mailbox |
| `teams`      | `kinds/teams.rs`       | `messages`, `meetings` | `browser` | `conversations`, `readMeetings`, `storageLocationId`, `waitForApproval` | `ingest` and `crate::meetings` | Itself, thread or conversation |
| `slack`      | `kinds/slack.rs`       | `messages`, `meetings` | `browser` | `conversations`, `readMeetings`, `waitForApproval` | `ingest` and `crate::meetings` | Itself, thread or conversation |
| `fireflies`  | `kinds/fireflies.rs`   | `meetings` | `fireflies`   | `storageLocationId`, `waitForApproval` | `crate::meetings`           | Never   |
| `youtrack`   | `kinds/youtrack.rs`    | `issues`   | `youtrack`    | `projects`                   | `crate::youtrack::reader` and the link watcher | Never   |

A definition is structure only: the kind's scope, what it provides, the account kind it signs in with, a browser
kind's start page, its content fields (a `selection` picked from what the account reaches, a `storage_location`, or a
`toggle`, on or off, which a new connector's form starts at the field's `onByDefault`), its ordered checks, and how what it reads is answered (`replies`: never, through the connector itself in the places it
names, or from a mailbox; see [Sending](#sending)). Account definitions say what a new account asks for (text, email, URL, secret, or a choice).
`GET /connectors/kinds` serves both, and the web app draws its forms and check lists from them, naming every id from
its `connectors` translations.

What a kind provides (`Provides`: `issues`, `pages`, `mail`, `messages`, `meetings`) is how a loop that reads one kind
of thing finds every kind that reads it, without naming them: `crate::meetings` reads every kind
`kinds::providing(Provides::Meetings)` answers, each through its source (`crate::meetings::sources`), and the web app's
Meetings page lists every connector whose kind provides meetings. The link watcher brings in what is assigned to the
person of every active connector whose kind provides issues, through the link provider `kinds::link_provider` names
for it (`docs/action-items.md`, Assigned issues).

A kind fills these verbs:

| Verb            | What it does                                                                                |
|-----------------|---------------------------------------------------------------------------------------------|
| Checks          | Builds the kind's checks against a stored connector or one being set up                      |
| Create          | Makes the connector with its new account: a token checked with its provider first, or a browser started at the kind's start page |
| Options         | Lists what each selection field can pick from, through a new account or a stored connector's |
| Update content  | Replaces what it reads, checked the way a new connector's content is                         |
| Sync            | Reads it now, in the loop that reads its kind                                                |
| Sign in         | Starts a browser for a browser connector waiting for its own                                 |
| Remove          | Removes it with its account                                                                  |
| Reply target    | Where a reply to one of its records goes, in the kind's terms, for a kind that replies through itself |
| Send            | Posts a draft's text to that target as its person, through the account it reads with        |

A new connector of a token kind can be checked before it is stored, with the fields it was given; one of a browser kind
cannot, since there is nothing to check until its person signs in on its browser, so its checks run on its page. Jira
and GitHub create their accounts through the same functions as their settings routes
(`create_jira_credential::create_credential`, `create_github_credential::create_credential`), so both check exactly the
same things. Fireflies and YouTrack store their key's account and the connector in one transaction, and a browser kind
removes the browser it started when its connector cannot be stored.

### Adding a kind

A new kind adds a `ConnectorKind`, a module with its definition and verbs, a row in `DEFINITIONS` and in each dispatch
in `kinds/mod.rs`, its name to the web app's `CONNECTOR_KINDS`, its logo to `connectorLogos`, and its name,
description, and check sentences to the web app's `connectors` translations. Its definition decides what its form
asks for, so no page names the kind. Check ids share one set of labels and count sentences across kinds, so a check
that counts something different takes an id of its own: Confluence's `sample_page_comments` beside YouTrack's
`sample_comments`. A kind whose account is a new provider adds an `AccountKind` and an `AccountDefinition`; when its
sign-in is one API key, its key lives in `token_accounts`. `kinds/mod.rs`'s tests fail when a kind has no definition,
provides nothing, an account kind has none, a browser kind names no start page, or a check depends on one that does not
run before it. A kind that provides meetings also adds its source to `crate::meetings::sources` (`docs/meetings.md`), and
a test there fails when it has none. A kind that provides issues names its link provider in `kinds::link_provider`, and a
test fails when a kind providing issues has none, or one providing none has one; its provider's `assigned` read is then
all it takes for what is assigned to its person to come in.

## Checks

A check proves one thing a connector needs, such as signing in or reaching its allowlist. Each kind declares its checks
in order (`CheckSpec`): an id, whether the connector is any use without it, and the checks it builds on.

| Kind              | Checks, in order                                                                                  |
|-------------------|----------------------------------------------------------------------------------------------------|
| Jira              | `sign_in`, `list_projects`, `list_boards` (optional), `allowlist_reachable`, `sample_issues`        |
| Confluence        | `sign_in`, `list_spaces` (also proves every chosen space is reached), `sample_pages`, `sample_page_comments` (optional) |
| GitHub            | `sign_in`, `read_scopes` (optional), `list_repositories`, `sample_issues` and `sample_pull_requests` (optional) |
| Outlook and Gmail | `browser_running`, `signed_in`, `inbox_readable`, `sample_message` (optional)                       |
| Teams             | `browser_running`, `signed_in`, `chats_readable`, `sample_message`, `list_meetings`, and `sample_transcript` (optional) |
| Slack             | `browser_running`, `signed_in`, `workspace_reachable`, `sample_conversation` (optional), `list_meetings` and `sample_transcript` (optional; skipped while meetings are off) |
| Fireflies         | `sign_in`, `list_meetings`, `sample_transcript` (optional)                                         |
| YouTrack          | `sign_in`, `list_projects`, `sample_issues`, `sample_comments` (optional)                          |

`checks::run_in_order` runs them one at a time against one value, so a later check uses what an earlier one fetched.
A check whose dependency did not pass is skipped and names that dependency (`blockedBy`), so a token Jira refuses fails
`sign_in` once rather than every check after it. An outcome is passed, failed, or skipped, with a `count` its sentence
fills, a `reason` in Elysium's own terms (`no_messages`, `fine_grained_scopes`), and the provider's own `message`.

Checks run on demand (`POST /connectors/{id}/checks`) and, for a token kind, before a connector is saved
(`POST /connectors/checks`, with
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
processed, changes the watcher read and assigned issues it brought in. A sync that found nothing new is not stored,
since a mailbox is read every minute. Each connector keeps its newest 50 runs.

A connector's status is, in order:

| Status    | When                                                                  |
|-----------|-----------------------------------------------------------------------|
| `paused`  | `is_active` is false                                                  |
| `syncing` | A sync someone asked for waits or runs, or one of its meetings is being processed |
| `error`   | Its last read failed, or its account is asking to be signed in again  |
| `active`  | Otherwise                                                             |

`syncing` is a mark on the connector's row (`syncing_since`), so the API shows what the engine is doing. A paused issue
connector's account (Jira, GitHub, or YouTrack) is skipped by the link watcher's reads, so neither its links nor what is
assigned to its person are read; writes the user already approved still land. A paused mailbox or meeting source is not
read.

`POST /connectors/{id}/sync` reads a connector now, which is also how a failed sync is retried. Every reader runs in the
engine (`docs/engine.md`), so the API marks the connector syncing and signals the engine, which reads it with its kind's
reader and clears the mark: a mailbox is read once, a meeting source lists and processes, and a Confluence space or
YouTrack site is read. Jira and GitHub wake the link watcher instead, which reads every Atlassian and GitHub account in
one pass and brings in what is assigned to each issue connector's person; a YouTrack sync wakes it too, beside its
reader's read. A sync asked for while the engine is down runs when it starts, as does one an engine that stopped left
marked.

## In the web app

Settings, Connectors (`/settings/connectors`, `frontend/src/pages/Settings/Connectors/`) is the hub, modeled on Onyx's
connectors. Every page sits in the shared settings frame (`SettingsPage`, `docs/frontend.md`), so the breadcrumbs and
Esc lead back.

- **The list** shows every connector the person sees with its kind's logo, the account it signs in as, status, last
  sync, records, and last error, and a gear menu to run its checks, sync it now, pause or resume it, and delete it. A
  search over kind and account names and a status filter narrow it. Above it, a summary and the activity feed of
  recent syncs, each failed one with a retry.
- **Adding** a connector starts at `/settings/connectors/new`, a searchable grid of every kind's logo and name; each
  card's `?` opens what the kind reads in a popover, and the search matches that too. `?provides=<what>` narrows the
  grid to the kinds that provide it, such as `meetings` from the Meetings page. A card opens its kind's page,
  `/settings/connectors/new/<kind>`:
  - A token kind asks for its sign-in, what it reads (a selection loads its options from what the typed account
    reaches), and Test connection, which runs its checks live against what was typed. The page's Save, "Create
    connector", waits for a finished run, and reads "Create anyway" when a required check failed; changing anything
    asks for another run.
  - A browser kind's page has Start browser, which creates the connector with its browser and opens the connector's
    page, where its person signs in, narrows what it reads, and runs its checks.
  - An unknown kind goes back to the grid.
- **A connector's page** (`/settings/connectors/:connectorId`) shows its status and last error; for a browser kind, its
  sign-in (`BrowserSignIn`: the browser's stream, Start over, and Reset, or Start browser while it waits for one); what
  it reads, saved with the page's Save; and its checks, stored or a run started there as it goes. Jira and GitHub link
  to their account's own edit page, and Jira to its done statuses.
- **Deleting** a connector confirms what goes with it: its Atlassian or GitHub account and what uses it, its key, or its
  browser and everything signed in on it.

Every form and check list is drawn from `GET /connectors/kinds`: `AccountFields` for a new account's fields (each secret
with a reveal toggle), `ContentFields` for content, and `CheckList` for checks. Names, descriptions, check labels, count
sentences, and reasons come from the `connectors` translations, by id, and logos from `connectorLogos.ts`, bundled
since the pages load nothing from another origin. Connectors reach Redux through `useConnectorsLoader` and stay current
through `connector.changed`, each loaded by id; a followed check run moves along on `connectorCheckRun.updated` and is
loaded again when it finishes, for its messages.

Settings, GitHub and Settings, Jira lead to the hub. `/settings/browsers` leads to the list, and a browser's own address,
`/settings/browsers/<id>`, to the connector that owns it.

## Ingest

`api/src/connectors/ingest.rs` reads every active connector read through a browser (`ConnectorKind::BROWSER_READ`:
Outlook, Gmail, Teams, and Slack) once a minute (`INGEST_INTERVAL`), until shutdown; meeting connectors have their own loop
(`docs/meetings.md`).

Each browser kind has a reader behind one trait, `BrowserReader`, and `api/src/connectors/readers.rs` is the one place a
kind meets its reader (`for_kind`). A reader is given its browser, what it kept from its last read, and its connector's
content (`ReadInput`), and answers a reading: the sign-in state, the items it found, and optionally its state for the
next read, such as a watermark. Ingest keeps that state in the connector's cursor beside the baseline,
`{ "baselined": true, "reader": { ... } }`; a reading with no state keeps the old one. Every loop writes only its own
keys of a cursor, merged over what is stored in the same statement (`connector::record_poll`), so a connector two loops
read, as Teams is by ingest and the meetings listing (`{"since": ...}`), keeps both. A reader whose service has no
tab open may open one (`Devtools::open_tab`). A new browser kind adds its reader, its rows in `for_kind` and
`service_page` (which tabs show its app and which its sign-in), and itself to `BROWSER_READ`, and a test fails when any
is missing or its start page is not its app.

Each pass also refreshes at most one browser, the one longest without, once a day: it reloads the service's tab so its
rolling session renews, and the connector's read in the same pass is its sign-in check (`docs/browser.md`,
Refreshing).

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

`api/src/connectors/teams.rs` reads Microsoft Teams through the connector's own browser, signed in to Microsoft,
which starts at Teams' own address. When no tab shows Teams (`teams.cloud.microsoft`, `teams.microsoft.com`, or `teams.live.com`) and none
is on a sign-in page, the read opens Teams in a new tab over DevTools and answers `unknown`; the next read finds it
loaded. A tab on a sign-in page means the session ended (`signed_out`). Nothing is installed into the organization.

**It reads Teams' own local store, not the page.** New Teams keeps the chats and channels it has loaded in
IndexedDB, in databases named `Teams:conversation-manager:...:<tenant id>:<user id>:<locale>` and
`Teams:replychain-manager:...`; the person's own messages carry their id as `8:orgid:<user id>`. `api/src/connectors/teams.js` runs as the page and reads them in read-only
transactions. Those records carry what the rendered page shows only in part, and in markup every redesign changes:
each message's id, sender, arrival time, and mentions, and each conversation's read horizon. Reading them opens no
chat, so unread messages stay unread. The store is the web client's own, not a published API, so the script reads its
fields defensively. When it finds no store it says so (`chats_readable` fails as `teams_store_not_found` and its message
lists the databases the page does hold), so a change to the layout shows up in the checks rather than as silence.

The script only reads; posting is a script of its own ([Sending](#sending)). The connector decides what the records
mean:

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
  wrote in it), comes from its sender, holds the message as plain text, and links to it with Teams' documented message
  link, `https://teams.microsoft.com/l/message/<conversation>/<message>`. Its `metadata` keeps where it was said
  (`conversationId`, `messageId`, `parentId` for a message in a thread, and `isChat`), which a reply to it needs.

**Message text is read off the markup, never parsed.** Teams enforces Trusted Types (`require-trusted-types-for
'script'`), and its `trusted-types` directive lists every policy it allows by name, none of them Elysium's, so the page
refuses any string handed to an HTML parser (`DOMParser`, `innerHTML`, a template). The script drops each tag instead,
leaving a space for one that ends a line (a paragraph, a break, a list item, a quote, a table cell) so the words on
either side stay apart, and then decodes character references: the five that HTML's serializer writes (`&amp;`, `&lt;`,
`&gt;`, `&quot;`, `&nbsp;`), `&apos;`, and numeric ones. Decoding after the tags are gone keeps escaped markup as text.
Over every message a signed-in Teams had stored, this reads the same words as the browser's own parser.

**It is a meeting source too.** Unless its person turns off "Read meeting recordings and transcripts"
(`readMeetings`), the same browser lists the recorded meetings whose chats Teams has stored, and fetches each one's
recording and transcript from the organizer's SharePoint with the token Teams keeps for it, through
`api/src/connectors/teams_meetings.js` (`docs/meetings.md`, Teams). Meetings wait for a storage location to keep the
recordings in, which the connector's page asks for once its person has signed in; until one is picked, `list_meetings`
says so (`no_storage_location`). `list_meetings` lists the last two weeks' recordings, and `sample_transcript` reads
the newest one's transcript, saying when its recording cannot be downloaded (`recording_not_downloadable`); both are
skipped while meetings are off (`meetings_off`). A sync someone asks for reads the chats and lists the meetings.

The script was checked against `api/src/connectors/fixtures/teams.html`, a page that seeds IndexedDB the way Teams lays
it out and enforces Trusted Types as Teams does, in headless Chromium (`fixtures/evaluate.mjs` runs any page script
against a fixture). Its answer, `fixtures/teams_page.json`, is what `teams.rs`'s tests read, so the rules above are
tested against the script's real output; `teams.test.mjs` runs the script against a fake store under the same
restriction in CI. The store's layout (`conversation-manager` and `replychain-manager`, the fields read from them, and
where the database name holds the person's id) is the one a signed-in Teams keeps.

### Slack

`api/src/connectors/slack.rs` reads Slack through a browser signed in to Slack's web client, `app.slack.com`. Nothing
is installed into a Slack workspace: a Slack app or bot token would need a workspace admin to approve it, which this
project never asks for, so the account is the browser and its sign-in, as for mail. A tab elsewhere on `slack.com` (a
sign-in page, or a workspace's own address, which redirects to one) means the session ended; with no Slack tab at all
the read opens `https://app.slack.com/client` over DevTools and answers `unknown`, and the next read finds it.

**It reads Slack's own web API, not the page.** The web client keeps each workspace it is signed in to in
`localStorage.localConfig_v2`, with the session token it sends with every call, and loads everything it shows through
Slack's web API on the workspace's own host. `api/src/connectors/slack.js` runs as the page and makes the same calls
with that session, then answers what Slack said; the token never leaves the page, and a workspace stored at any host
that is not `*.slack.com` is never called. Only read methods can be called (`client.counts`, `conversations.history`,
`subscriptions.thread.getView`, `users.info`, `conversations.info`, `users.conversations`, `auth.test`, and for
huddles `huddles.history`, `huddles.get`, and `files.info`), so reading never posts and never marks a conversation
read. Posting is a script of its own ([Sending](#sending)). The API was chosen over the DOM because the DOM shows only the
conversation on screen and its markup changes freely, while the API answers every conversation, by id, with Slack's
own timestamps, whatever the page shows.

**Every call carries the web client's marker.** The page is on `app.slack.com` and each workspace's API on its own
host, so every call is a credentialed cross-origin request. Slack answers one with `app.slack.com` as its allowed
origin only when it carries `_x_gantry=true`, as the web client's own calls do; without it Slack answers
`Access-Control-Allow-Origin: *`, which a browser refuses alongside cookies, and the call never reaches the script
(`fetch_failed`, with the browser's own message). Each call's address is `api/<method>?_x_gantry=true&slack_route=<team>`
on the workspace's host, the posting script's too, and `slack.test.mjs` models that rule.

A read is up to three evaluations: `client.counts` for every workspace (each conversation's newest message, and
whether the person's threads have unread replies); `conversations.history` for each conversation with something newer
than the connector has read, and the threads view where threads have unread replies; then `users.info` and
`conversations.info` for names it does not know yet. One read fetches at most 25 conversations, the most recently
active first; the rest keep their place and are read next time.

- **Its state** is the newest message read in each conversation, by workspace and conversation id, and the names it
  has looked up. A conversation it has never read is baselined at its newest message, so connecting Slack or joining
  a channel never pulls its history in.
- **Content** is `conversations`: `"*"`, or picks of a whole workspace (`T1`), one conversation (`T1/C1`), or a
  workspace's direct and group messages (`T1/dms`), from what the browser's Slack lists (`POST /connectors/options`).
  An option whose words are Elysium's rather than Slack's names them by key (`labelKey`, `detailKey`) for the web app.
  Beside it are `readMeetings` (on by default) and `waitForApproval`: Slack is a meeting source too, and while
  `readMeetings` is on the meetings loop lists the transcribed huddles of the conversations it picks
  (`docs/meetings.md`, Slack huddles), keeping its `since` in the cursor beside the reader's keys.
- **Each message** becomes a record, its id `team/conversation/ts`, its sender's name as actor, a title naming where
  it was said (`#launch in Mooreslab`, `Reply in #launch in Mooreslab`, `Ada in a direct message in Mooreslab`),
  Slack's markup as plain text, the link Slack's Copy link gives, and in `metadata` where it was said (`team`,
  `conversation`, `ts`, and `threadTs` for a reply), which a reply to it needs. Joins, topic changes, and similar notices are
  not recorded.
- **What wants attention:** a direct or group message, a message that mentions the person, and an unread reply in a
  thread they follow, unless they wrote it. Other channel messages are only recorded.

The page script is tested in Node against a fixture of the web client, its stored session and a fake web API
(`api/src/connectors/slack.test.mjs`, run with `node --test api/src/connectors/*.test.mjs`, in CI under
`Frontend test`); the reading rules are tested in `slack/tests.rs` against answers shaped as Slack's web API gives
them. `client.counts` and `conversations.history` are confirmed against a signed-in Slack; the threads view
(`subscriptions.thread.getView`) is the web client's own method, not Slack's published API, and is not yet, so its
shape is read defensively.

### On each read

- Each item becomes a timeline record. An item its connector already recorded is skipped, by the connector and the
  provider's own id, so reading the same mailbox again records nothing twice.
- Each new item that wants attention (an unread message) raises an inbox action item for the user, in the same
  transaction as its record, which points at it (`action_item_id`). Action items are one list for the whole workspace
  (`docs/action-items.md`), so the item, with the message's subject, sender, and body, is seen by everyone signed in.
  The item is recorded as the watcher of that service, `watcher:outlook`: like the link watcher, it records what
  already happened in a provider, and the person consented when they connected the mailbox.
- Each new message joins the conversation it was said in, in the same transaction: one that wants attention always,
  another only while its conversation is being worked on. Each reader names the conversation (`Read.conversation`):
  in Slack a direct or group message, or a channel thread; in Teams a chat, or a channel thread; in mail one sender's
  mail under one subject. Once the conversation goes quiet, a request run starts work on it (`docs/engine.md`,
  Conversations).
- The connector's first signed-in read records what the mailbox already holds without raising anything, so
  connecting a mailbox does not fill the inbox with its backlog. Its cursor remembers that (`{"baselined": true}`).
- A read that fails keeps the cursor and records the reason in `last_error`.

### Confluence

`api/src/confluence/` is the only caller of Confluence Cloud. It reaches the connector's Atlassian account's site
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

Every field the client reads is typed as Atlassian's OpenAPI specifications declare it, and
`api/src/confluence/tests.rs` proves Elysium's half against a fake shaped the same way. The two API versions
disagree on ids: content ids are strings, but a v1 space's id is a number. A search result's `container` is the page
for a comment and the space for a page, so only a comment's is kept. `api/src/confluence/CLAUDE.md` records
Confluence's quirks and every way it has broken the reader. A scoped Atlassian API token
(the long kind) is refused on the site's own host for Confluence and works only through
`api.atlassian.com/ex/confluence/<cloudId>` (`~/.claude/learnings/jira.md`), which Elysium does not call yet; use a
classic token.

### YouTrack

`api/src/youtrack/reader.rs` reads every active YouTrack connector every two minutes (`READ_INTERVAL`), until
shutdown. The issues assigned to its person come in through the link watcher, as every issue connector's do
(`docs/action-items.md`, Assigned issues), each as an item linked to its issue; the reader puts what happens on the
site on the timeline. A read signs in as the token's user, lists the chosen projects' issues most recently updated first
(`project: JL, WAY sort by: updated desc`, up to four pages of 50), stops at the cursor, and reads the comments on each
changed issue.

- A new issue is recorded as `issue:<id>`, titled with its key and summary, from its reporter.
- Each new comment is recorded as `comment:<id>`, titled `Comment on <key> <summary>`, linking to the comment itself.
- Other changes, such as a state moving, are not recorded: following an issue's state is what linking it to an item
  is for.

A YouTrack account also links issues to action items (`api/src/action_items/links/youtrack.rs`, `docs/action-items.md`),
reached through `crate::youtrack` alone: the watcher reads linked issues by key, `issue id: JL-1, JL-2`, the issues
assigned to the token's user in the chosen projects, `project: JL for: me #Unresolved` (at most ten pages of a
hundred), and its writes open issues, comment, and move an issue's state (`POST /api/issues`,
`/api/issues/<id>/comments`, and `/api/issues/<id>` with the state field), as the token's user. A Project Manager files
issues in a YouTrack project through a person's account when its project names that tracker (`docs/engine.md`, Its
writes). A watcher pass over the account that read a change, brought an issue in, or failed is reported as a run of
its connector, beside the reader's. The `issue id:` and `for: me` searches, creating issues, and moving a state through
the state field's `$type` (`StateProjectCustomField`, `StateIssueCustomField`) follow JetBrains' REST documentation and
are tested against a fake site (`api/src/youtrack/tests.rs`); they have not yet been run against a live YouTrack Cloud
site.

A comment that wants attention raises an inbox item recorded as `watcher:youtrack`, in the same transaction: one
someone else wrote, on an issue the user is assigned or reported, or that mentions `@<login>`. Its record names the
item, which is how Elysia hears of it. An issue's record raises nothing, since an issue assigned to the user already
comes in as an item linked to it.

The cursor is the newest update read, in YouTrack's own clock (`{"baselined": true, "since": <ms>}`). The first read
records nothing and only sets it, so connecting a site does not fill the timeline and the inbox with its history. A
read the site refuses marks the connector `signed_out`; any other failure keeps the cursor and records the reason.

## Sending

A person answers what a connector read as themselves, through what the record's kind declares (`Definition.replies`).
`crate::drafts` is the one way anything leaves Elysium in someone's name: the web app's Send calls it, on a reply of
the person's own and on a follow-up a Project Manager drafted for a promise of theirs (`docs/engine.md`, Follow-ups).
Sending is the person's own act, so it needs no changeset; anything else can only write a draft for them.

- **A draft first.** Everything sent is a draft (`drafts`, `docs/database.md`): its text, what sends it (a connector or
  a mailbox), and where it goes (`target`, in the sender's own terms, decided when it is written from the record it
  answers through `kinds::reply_target`). A draft is the person's alone.
- **One send at a time.** Sending claims the draft (`draft` or `failed` to `sending`) in one statement, so a second
  press cannot post twice. A send has 20 seconds, inside the request's own limit, and answers how it ended. A draft
  left `sending` by a process that stopped mid-send can be sent again after two minutes.
- **A failure keeps the draft.** A send the provider refused, or that could not reach it, leaves the draft `failed`
  with the reason in words its person can act on (Slack signed out in this browser, Teams has not loaded its chats
  yet), and its text untouched. Nothing is retried on its own: posting to a person twice is worse than not posting, and
  a send that timed out may have gone out, which its reason says.
- **A success is on the timeline.** The sent message becomes one of the person's records, named the way the kind's
  reader names what it reads (Slack's `team/conversation/ts`, Teams' `conversation/message`), so reading it back later
  records nothing twice; the draft points at that record. Its actor is the person, its title the conversation it
  answers, and its metadata names the draft (`draftId`).
- **A follow-up keeps its promise.** A draft written for a promise (`drafts.promise_id`) is decided exactly as
  `draft_reply` decides a reply (`drafts::reply_draft`, which the Project Manager's tool stores with its item in one
  transaction). Sending it marks the promise kept and resolves its item, as the person, in the transaction that records
  the send, so a follow-up is never sent and left owed. It belongs to its item's page: the reply box of the record it
  answers lists only the person's own drafts.

| Kind              | Replies                  | Sent through                                                         |
|-------------------|--------------------------|----------------------------------------------------------------------|
| Slack             | In the thread, or in the conversation | `slack_send.js`, in the connector's browser: Slack's published `chat.postMessage` with the web client's session |
| Teams             | In the thread, or in the conversation; a chat has no threads | `teams_send.js`, in the connector's browser: the chat service the web client posts to |
| Outlook and Gmail | To the sender            | SMTP, from a mailbox of the person's own (`docs/mail.md`)            |

**Slack.** `slack_send.js` can call `chat.postMessage` and nothing else, so the reader's script stays read-only. It
finds the workspace's session in `localConfig_v2` as the reader does, posts on the workspace's own host, and answers
Slack's `ok`, `error`, `channel`, and `ts`, never the token. A reply in the thread names the thread's first message as
`thread_ts` (the record's `threadTs`, or its own `ts`), as Slack asks. Typed text is plain: `&`, `<`, and `>` are
escaped, so nothing typed becomes a mention or a link. The script is tested in Node against a fake web API
(`slack_send.test.mjs`); it has not yet posted to a signed-in Slack.

**Teams.** `teams_send.js` posts to `<teams host>/api/chatsvc/<region>/v1/users/ME/conversations/<id>/messages`, the
web client's own chat service, which is not a published API. A channel reply posts to `<channel>;messageid=<first
message>`. Its token is the web client's access token for `ic3.teams.office.com` from the MSAL cache in localStorage,
and its address, region included, is taken from a call the page already made; each missing piece is named
(`token_expired`, `token_not_found`, which is also what an encrypted cache looks like, and `region_unknown`) and becomes
a reason the person can act on. The text is sent as HTML, escaped, with its line breaks. The chat service names the
posted message by its arrival time, which is also its id. This follows what the web client is known to do (the
open-source `teams-api` client posts the same way) and is tested in Node against a fake chat service
(`teams_send.test.mjs`); none of it has yet been checked against a signed-in Teams.

**Mail.** Outlook and Gmail are read through a browser, which has no way to send that is not the page's own markup or
the services' internal calls, so mail is answered over SMTP from a mailbox the person connected under Settings, Email
(`mail_accounts`; `docs/mail.md`): the one named, or their only active one. A mailbox sends only for the person who
connected it. A reply goes to the record's sender under its subject with `Re: ` in front, and carries the answered
message's `Message-ID` and `References` when its record keeps them; the browser readers keep neither, so those replies
do not thread in the recipient's client until mail is read over IMAP. A new mail names its mailbox, recipients, and
subject. A mail sent is recorded as `mail`, with no connector.

## What is published

| Event                        | When                                                                          |
|------------------------------|-------------------------------------------------------------------------------|
| `connector.changed`          | A connector was added or changed: paused, its content, a sync starting or ending, a read finding something, its checks. Its id alone |
| `connector.deleted`          | A connector was removed, or the account it read through                       |
| `connectorCheckRun.updated`  | A check in a run moved on, or the run ended (`checkId` null, `state` `finished`) |
| `timelineRecord.created`     | A record was stored, read or sent: its id, kind, owner, the item it raised, and the conversation it joined. Never its content |
| `draft.changed`              | A draft was written, edited, discarded, or a send of it began, landed, or failed: its id and writer |
| `actionItem.upserted`        | The item a record raised, like any item write                                 |

`connector.changed` carries the id alone because a connector's view names whose mailbox or meetings it reads and every
client hears every event; clients load it by id, which answers only those who may see it.

`timelineRecord.created` is the seam Elysia's triage builds on: her intake hears it in process and loads the record by
id, except a message that joined a conversation, which that conversation's request run handles.

## The timeline

`timeline_records` holds everything ingested: its kind, the connector it came from, the provider's id, when it
happened, who it came from, a title, a plain-text body, a link, optionally a span inside a recording (`media`, for a
meeting segment), and free `metadata`. A record belongs to whoever it was ingested for, and only they list it: a
person's mail is theirs even when it is about a shared project. That ownership is of the feed, its attributions, and
its recordings, not of the text: the knowledge base indexes every record for everyone in the workspace, in the
transaction that stores it (`docs/knowledge.md`, Timeline records, and Who sees what).

What Elysium's own agents put on a project's timeline is nobody's feed: it belongs to the workspace, and every company
member reads it on the project's timeline beside their own records; a guest reads only their own. Nobody moves it
between projects or answers it. It is a Project Manager's turns and the batches it submits (kind `project_manager`,
`docs/engine.md`, Project Managers), owned by the Project Managers' machine user, and every session's turn summaries
(kind `turn_summary`, `docs/coding.md`, Turn summaries), owned by the coding agent's machine user, since every session
is listed to everyone, and every merge of a pool worker's pull request (kind `merged_pull_request`, `docs/engine.md`,
Merging), owned by the coding agent's machine user too (`timeline_record::SHARED_OWNERS`).

`timeline_record_projects` attributes records to projects, each with a confidence and who attributed it. Elysia
attributes as segmenting reads a meeting or a conversation (`docs/meetings.md`, Segmentation), with her confidence;
the record's owner confirms, adds, or removes. Every change goes through `models::attribution`:

- **A person's word stands.** A person's attribution has a confidence of 1 and replaces whatever Elysia guessed for
  that project. Elysia never changes or removes a person's attribution, and never attributes a record to a project a
  person decided on for it, either way, so a segment a person moved stays moved however often it is read again.
- **History.** Each change is recorded in `timeline_record_events` in the same transaction, with what the attribution
  held before and after (`docs/database.md`), and announced as `timelineRecord.changed`. A write that changes nothing
  records nothing.
- **Undo.** Any entry can be undone by the record's owner, and the undo is an entry of its own naming the one it
  reversed (`undid`), so it can be undone in turn. Only an attribution still holding what the entry wrote is undone; one
  changed since answers `409` (`attribution_changed`) rather than losing that change.
- **A new attribution is news for the project.** A record newly on a project (Elysia's reading, a person adding it, an
  undo putting it back) wakes that project's Project Manager in the same transaction; a change of how sure an
  attribution is does not, and nor does what Elysium's agents attribute: a Project Manager its own records, and a
  session's agent (`session:<n>`) its turn summaries.

`GET /api/v1/timeline` lists the caller's records, and for a member the workspace's, newest first, optionally for one project or one meeting's segments
and before a time, with `total`, how many records the filters match across every page (a project page's Timeline tab
shows it); `PUT` and `DELETE /api/v1/timeline/{id}/projects/{projectId}` attribute and unattribute one, and
`/api/v1/timeline/{id}/history` lists and undoes its changes. See `docs/api.md`.

## Roadmap

- Changing a token connector's sign-in from its page: a new key for Fireflies or YouTrack, and a new token for
  Confluence, whose Atlassian account has no edit page of its own; today such a connector is removed and added again.
- More kinds: Google Meet recordings (`docs/meetings.md`).
- Confluence through Atlassian's gateway (`api.atlassian.com/ex/confluence/<cloudId>`), so scoped API tokens work.
- Confluence attachments and inline comments, and a mention found by display name as well as account id.
- Slack: confirming the threads view against a signed-in Slack, and an Enterprise Grid org,
  whose workspaces the web client may keep differently in `localConfig_v2`.
- Teams: how much of its store Teams keeps. The connector reads only what Teams has stored; if Teams stores a channel's
  messages only after showing that channel, a channel never opened in that browser is not read. Not yet confirmed
  either way.
- YouTrack issues in the link picker, and YouTrack containers for initiatives.
- YouTrack Server, on a customer's own host, beside YouTrack Cloud.
- Keeping a token account's `identity` and `checked_at` current from its checks.
- Drafting replies for review with what a conversation's agent raises (`docs/engine.md`, Conversations).
- Sending checked against a signed-in Slack and Teams: Slack's `chat.postMessage` with the web client's session, and
  Teams' chat service token, address, and message shape.
- Replies that thread in the recipient's mail client, once mail is read over IMAP and records keep `Message-ID`.
- Writing a new message to a conversation picked rather than answered, and new mail, from the web app; the API takes
  new mail today.
