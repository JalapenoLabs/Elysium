# Connectors and the timeline

A connector is a service Elysium reads as its user. What every connector reads lands on one timeline, as one kind of
record, attributed to projects. Today two connectors exist, Outlook and Gmail, which read through a browser the person
signed in to (`docs/browser.md`).

## The connector trait

Every connector fills one trait, `Connector` in `api/src/connectors/mod.rs`: it names its kind and reads what its
account shows now (`Reading`: the sign-in state, and the items newest first). How it signs in and stays signed in
belongs to what it reads through: a browser keeps its own session, a token its own refresh. A connector on an API or
a CLI fills the same trait.

`api/src/connectors/webmail.rs` fills it for webmail. Each service is one row of `SERVICES`:

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

## Accounts

A connector account is a connector bound to one of its owner's browsers, at most one per browser
(`connector_accounts`, `docs/database.md`). It belongs to whoever owns the browser, and removing the browser removes
it. On the browser's page in Settings, Browser sign-ins, a browser started on a mailbox a connector reads offers
"Read this mailbox", and shows the account's sign-in state and last error once it reads.

## Ingest

`api/src/connectors/ingest.rs` reads every active account once a minute (`INGEST_INTERVAL`), until shutdown:

- Each item becomes a timeline record. An item its account already recorded is skipped, by the account and the
  provider's own id, so reading the same mailbox again records nothing twice.
- Each new item that wants attention (an unread message) raises an inbox action item owned by the person, in the
  same transaction as its record, which points at it (`action_item_id`). The item is recorded as the watcher of
  that service, `watcher:outlook`: like the link watcher, it records what already happened in a provider, and the
  person consented when they connected the mailbox.
- The account's first signed-in read records what the mailbox already holds without raising anything, so
  connecting a mailbox does not fill the inbox with its backlog. Its cursor remembers that (`{"baselined": true}`).
- A read that fails keeps the cursor and records the reason in `last_error`.

What is published:

| Event                       | When                                                         |
|-----------------------------|--------------------------------------------------------------|
| `timelineRecord.created`    | A record was stored: its id, kind, owner, and the item it raised. Never its content |
| `actionItem.upserted`       | The item a record raised, like any item write                |
| `connectorAccount.upserted` | An account was added, or a read changed its sign-in state or error |
| `connectorAccount.deleted`  | An account was removed                                       |

`timelineRecord.created` is the seam Elysia's triage builds on: her dispatcher hears it in process and loads the
record by id.

## The timeline

`timeline_records` holds everything ingested: its kind, the account it came from, the provider's id, when it
happened, who it came from, a title, a plain-text body, a link, optionally a span inside a recording (`media`, for a
meeting segment), and free `metadata`. A record belongs to whoever it was ingested for, and only they list it: a
person's mail is theirs even when it is about a shared project.

`timeline_record_projects` attributes records to projects, each with a confidence and who attributed it. A person's
attribution has a confidence of 1 and replaces whatever was guessed for that project; Elysia's carry her
confidence.

`GET /api/v1/timeline` lists the caller's records newest first, optionally for one project and before a time; `PUT`
and `DELETE /api/v1/timeline/{id}/projects/{projectId}` attribute and unattribute one. See `docs/api.md`.

## Roadmap

- Elysia's triage on `timelineRecord.created`: classifying each message, drafting replies for review, and attributing
  records to projects.
- History for attributions, so a correction can be undone like any other edit.
- Project pages showing their timeline, meeting segments included.
- More connectors: Slack and Teams through the browser, meetings from recordings and transcripts.
