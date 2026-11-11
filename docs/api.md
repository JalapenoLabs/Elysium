# API

Rust binary in `api/`, built on axum and tokio. Every route is mounted under `/api` because nginx forwards that
prefix unchanged. Health and build routes sit at the top level. Resource routes are versioned under `/api/v1`.

## Routes

| Method | Path                                  | Response                                            |
|--------|---------------------------------------|-----------------------------------------------------|
| GET    | `/api/ok`                             | `200` text `ok`                                     |
| GET    | `/api/ping`                           | `200` text `pong`                                   |
| GET    | `/api/version`                        | `200` JSON, see below                               |
| GET    | `/api/v1/events`                      | Server-sent event stream, see `docs/realtime.md`    |
| GET    | `/api/v1/password-vault`              | `200` `{ vault: PasswordVault \| null, status }`; admins |
| PUT    | `/api/v1/password-vault`              | `200` `{ vault, status, collections }`; signs in first |
| DELETE | `/api/v1/password-vault`              | `204`; the CLI signs out                            |
| GET    | `/api/v1/password-vault/collections`  | `200` `{ collections: Collection[] }`, as the CLI answers |
| PUT    | `/api/v1/password-vault/collection`   | `200` `{ vault, status }`; the collection agents read |
| POST   | `/api/v1/password-vault/sync`         | `200` `{ status }`                                  |
| GET    | `/api/v1/browser-sessions`            | `200` `{ browserSessions: BrowserSession[] }`, the caller's, oldest first |
| GET    | `/api/v1/browser-sessions/{id}`       | `200` `{ browserSession }`                          |
| POST   | `/api/v1/browser-sessions/{id}/start-over` | `204`; its start page in a fresh tab, other tabs closed, profile kept; `502` when Chromium does not answer |
| POST   | `/api/v1/browser-sessions/{id}/reset` | `202` `{ browserSession }`, `starting`; empties its profile and starts it again |
| GET    | `/api/v1/browser-sessions/{id}/stream` | WebSocket: the browser's signaling, see `docs/browser.md` |
| GET    | `/api/v1/connectors`                  | `200` `{ connectors: Connector[] }`, those the caller sees |
| POST   | `/api/v1/connectors`                  | `201` `{ connector }`, with the new account it owns |
| GET    | `/api/v1/connectors/{id}`             | `200` `{ connector }`                                |
| PATCH  | `/api/v1/connectors/{id}`             | `200` `{ connector }`; `isActive`, `content`, or both |
| DELETE | `/api/v1/connectors/{id}`             | `204`; removes its account too; its records stay on the timeline |
| GET    | `/api/v1/connectors/kinds`            | `200` `{ kinds, accounts }`: every kind's definition |
| GET    | `/api/v1/connectors/activity`         | `200` `{ runs }`: the newest syncs worth showing     |
| POST   | `/api/v1/connectors/options`          | `200` `{ fields }`: what each selection field can pick from |
| POST   | `/api/v1/connectors/checks`           | `202` `{ run }`: checks a connector not stored yet   |
| GET    | `/api/v1/connectors/check-runs/{runId}` | `200` `{ run }`, to whoever started it             |
| POST   | `/api/v1/connectors/{id}/checks`      | `202` `{ run }`: runs a stored connector's checks    |
| POST   | `/api/v1/connectors/{id}/sync`        | `202`: reads it now; `409` when paused or already syncing |
| POST   | `/api/v1/connectors/{id}/sign-in`     | `200` `{ connector }`: starts a browser for one waiting for its own |
| GET    | `/api/v1/meetings`                    | `200` `{ meetings: Meeting[] }`, the caller's 100 newest; `reviewItemId` keeps the one an inbox item asks to review |
| GET    | `/api/v1/meetings/{id}`               | `200` `{ meeting }`                                 |
| POST   | `/api/v1/meetings/{id}/retry`         | `200` `{ meeting }`; retries its run's failed step, `404` when its run has not failed |
| GET    | `/api/v1/meetings/{id}/words`         | `200` `{ words, marks, captures }`: its final transcript as it reads, every mark on it, and its captures |
| GET    | `/api/v1/meetings/{id}/captures/{captureId}/transcript` | `200` `{ transcript }`: one capture's own lines as they came, on the meeting's clock |
| GET    | `/api/v1/meetings/{id}/captures/{captureId}/media` | The capture's recording, streamed from storage: `206` for one `Range`, `416` past its end, else `200`; `404` when none is kept |
| POST   | `/api/v1/meetings/{id}/edits`         | `200` `{ entry, change }`; replaces `startWordId` to `endWordId` with `text`, `409` (`meeting_words_changed`) when a word of it was replaced meanwhile |
| GET    | `/api/v1/meetings/{id}/history`       | `200` `{ entries }`: every change to it, newest first |
| POST   | `/api/v1/meetings/{id}/history/{entryId}/undo` | `200` `{ entry, change }`; `409` (`meeting_changed_since`, `meeting_change_not_undoable`, `meeting_corrected`) when it stays |
| POST   | `/api/v1/meetings/{id}/correction/approve` | `200` `{ meeting }`; approves the corrected copy and lets a run held for it go on, `404` before it is corrected |
| POST   | `/api/v1/meetings/{id}/hold`          | `200` `{ meeting }`; waits for the caller's approval, `409` (`meeting_corrected`) once corrected |
| DELETE | `/api/v1/meetings/{id}/hold`          | `200` `{ meeting }`; stops waiting, `409` (`meeting_corrected`) once corrected |
| PUT    | `/api/v1/meetings/{id}/captures/{captureId}/offset` | `200` `{ capture: { id, offsetMs, offsetSource } }`; body `{ offsetMs }`, at most twelve hours either way (`400` `offset_out_of_range`), set by hand and lined up at once |
| POST   | `/api/v1/meetings/{id}/captures/{captureId}/split` | `200` `{ meeting }`; splits out the merge that brought the capture, `409` (`capture_not_merged`, `meeting_changed`) when none did or it moved on since |
| GET    | `/api/v1/meetings/duplicates?itemId=` | `200` `{ duplicate: MeetingDuplicate \| null }`: the two meetings an inbox item asks the caller about, with their titles and the source of each one's first capture |
| POST   | `/api/v1/meetings/duplicates/{id}/merge` | `200` `{ duplicate }`; merges the meeting asked about into the other, `409` (`duplicate_decided`) once decided |
| POST   | `/api/v1/meetings/duplicates/{id}/not-the-same` | `200` `{ duplicate }`; remembers they are not the same, `409` (`duplicate_decided`) once decided |
| GET    | `/api/v1/timeline`                    | `200` `{ records: TimelineRecord[], total }`, the caller's and, for a member, the workspace's, newest first |
| GET    | `/api/v1/timeline/{id}`               | `200` `{ record }`                                  |
| GET    | `/api/v1/timeline/{id}/media`         | The record's clip, streamed from storage a range at a time, as a capture's recording is; `404` when it has none |
| PUT    | `/api/v1/timeline/{id}/projects/{projectId}` | `200` `{ record, entryId }`; the caller attributes it to a project |
| DELETE | `/api/v1/timeline/{id}/projects/{projectId}` | `200` `{ record, entryId }`; the caller takes it off a project |
| GET    | `/api/v1/timeline/{id}/history`       | `200` `{ history: HistoryEntry[] }`, every change to its attributions, oldest first |
| POST   | `/api/v1/timeline/{id}/history/{entryId}/undo` | `200` `{ record, entryId }`; reverses one change, `409` (`attribution_changed`) when it changed since |
| GET    | `/api/v1/drafts?recordId=`            | `200` `{ drafts: Draft[] }`: the caller's unsent drafts answering a record, newest first |
| POST   | `/api/v1/drafts`                      | `201` `{ draft }`: a reply or a new mail, sent nowhere yet |
| GET    | `/api/v1/drafts/{id}`                 | `200` `{ draft }`                                   |
| PATCH  | `/api/v1/drafts/{id}`                 | `200` `{ draft }`; its `body`; `409` while sending or once sent |
| DELETE | `/api/v1/drafts/{id}`                 | `204`; `409` while it is being sent                 |
| POST   | `/api/v1/drafts/{id}/send`            | `200` `{ draft }`, `sent` or `failed` with `lastError`; `409` while sending or once sent |
| GET    | `/api/v1/action-items`                | `200` `{ items: ActionItem[] }`, newest first       |
| POST   | `/api/v1/action-items`                | `201` `{ item }`                                    |
| GET    | `/api/v1/action-items/next`           | `200` `{ items: ActionItem[], inboxCount }`, in Next's order |
| POST   | `/api/v1/action-items/bulk`           | `200` `{ items, refused }`; one change to many items |
| GET    | `/api/v1/action-items/{id}`           | `200` `{ item }`, deleted or not                    |
| PATCH  | `/api/v1/action-items/{id}`           | `200` `{ item }`                                    |
| DELETE | `/api/v1/action-items/{id}`           | `204`; soft, restorable                             |
| POST   | `/api/v1/action-items/{id}/restore`   | `200` `{ item }`                                    |
| POST   | `/api/v1/action-items/{id}/accept`    | `200` `{ item }`; also `resolve`, `dismiss`, `reopen` |
| POST   | `/api/v1/action-items/{id}/snooze`    | `200` `{ item }`                                    |
| POST   | `/api/v1/action-items/{id}/wait`      | `200` `{ item }`                                    |
| GET    | `/api/v1/action-items/{id}/history`   | `200` `{ history: HistoryEntry[] }`, oldest first   |
| POST   | `/api/v1/action-items/{id}/history/{entryId}/undo` | `200` `{ item }`; reverses a change to its projects or lead, `409` when it changed since |
| PUT    | `/api/v1/action-items/{id}/lead-project` | `200` `{ item }`; `{ projectId }`, one of its projects |
| GET    | `/api/v1/action-items/{id}/manager-assignment` | `200` `{ plan }`, where its plan stands, or `null` while it is not assigned; members only |
| PUT    | `/api/v1/action-items/{id}/manager-assignment` | `200` `{ item }`; assigns it to its lead project's Project Manager, `409` with no lead; members only |
| DELETE | `/api/v1/action-items/{id}/manager-assignment` | `200` `{ item }`; takes it back; members only |
| GET    | `/api/v1/action-items/{id}/comments`  | `200` `{ comments: Comment[] }`, oldest first       |
| POST   | `/api/v1/action-items/{id}/comments`  | `201` `{ comment }`                                 |
| PATCH  | `/api/v1/action-items/{id}/comments/{commentId}` | `200` `{ comment }`                      |
| DELETE | `/api/v1/action-items/{id}/comments/{commentId}` | `204`                                    |
| PUT    | `/api/v1/action-items/{id}/projects/{projectId}` | `200` `{ item }`                         |
| DELETE | `/api/v1/action-items/{id}/projects/{projectId}` | `200` `{ item }`                         |
| PUT    | `/api/v1/action-items/{id}/initiatives/{initiativeId}` | `200` `{ item }`                   |
| DELETE | `/api/v1/action-items/{id}/initiatives/{initiativeId}` | `200` `{ item }`                   |
| POST   | `/api/v1/action-items/from-link`      | `201` `{ item, link }`; an item for a linked thing  |
| GET    | `/api/v1/action-items/{id}/links`     | `200` `{ links: ActionItemLink[] }`, the primary first |
| POST   | `/api/v1/action-items/{id}/links`     | `201` `{ link }`                                    |
| GET    | `/api/v1/action-items/{id}/links/remote` | `200` `{ remotes }`, each link read live         |
| DELETE | `/api/v1/action-items/{id}/links/{linkId}` | `204`                                          |
| PUT    | `/api/v1/action-items/{id}/links/{linkId}/primary` | `200` `{ links }`                      |
| DELETE | `/api/v1/action-items/{id}/links/{linkId}/writes/{writeId}` | `204`; cancels a pending write |
| DELETE | `/api/v1/action-items/{id}/issue-filing` | `204`; cancels the issue it is owed before it is filed |
| GET    | `/api/v1/initiatives`                 | `200` `{ initiatives: Initiative[] }`, by name      |
| POST   | `/api/v1/initiatives/bulk`            | `200` `{ initiatives, refused }`; one change to many |
| POST   | `/api/v1/initiatives`                 | `201` `{ initiative }`                              |
| GET    | `/api/v1/initiatives/{id}`            | `200` `{ initiative }`, deleted or not              |
| PATCH  | `/api/v1/initiatives/{id}`            | `200` `{ initiative }`                              |
| DELETE | `/api/v1/initiatives/{id}`            | `204`; soft, restorable                             |
| POST   | `/api/v1/initiatives/{id}/restore`    | `200` `{ initiative }`                              |
| GET    | `/api/v1/initiatives/{id}/progress`   | `200` `{ resolved, total, burnup }`                 |
| GET    | `/api/v1/initiatives/{id}/history`    | `200` `{ history: HistoryEntry[] }`, oldest first   |
| PUT    | `/api/v1/initiatives/{id}/projects/{projectId}` | `200` `{ initiative }`                    |
| DELETE | `/api/v1/initiatives/{id}/projects/{projectId}` | `200` `{ initiative }`                    |
| GET    | `/api/v1/initiatives/{id}/links`      | `200` `{ links: InitiativeLink[] }`, oldest first   |
| POST   | `/api/v1/initiatives/{id}/links`      | `201` `{ link }`; links a container                 |
| DELETE | `/api/v1/initiatives/{id}/links/{linkId}` | `204`; its items leave the initiative           |
| POST   | `/api/v1/issue-reports/preview`       | `200` `{ draft, repository, credentials, suggestedCredentialId, previousReport, remainingToday }`; nothing reaches GitHub |
| POST   | `/api/v1/issue-reports`               | `201` `{ report }`: an issue opened, or a comment on the problem's open issue |
| GET    | `/api/v1/changesets`                  | `200` `{ changesets: Changeset[] }`, newest first    |
| GET    | `/api/v1/changesets/{id}`             | `200` `{ changeset }`                               |
| POST   | `/api/v1/changesets/{id}/decide`      | `200` `{ changeset }`; approves or rejects operations |
| GET    | `/api/v1/elysia`                      | `200` `{ elysia }`: her status, hold, questions, work, active turns |
| PATCH  | `/api/v1/elysia/settings`             | `200` `{ elysia }`; admins: `paused`, `availability`, `resumeUsage` |
| GET    | `/api/v1/elysia/messages`             | `200` `{ messages }`, newest first; `before`, `limit` |
| POST   | `/api/v1/elysia/messages`             | `201` `{ message }`; queues a turn                  |
| GET    | `/api/v1/elysia/turns`                | `200` `{ turns }`, newest first; `limit`            |
| POST   | `/api/v1/elysia/turns/{id}/cancel`    | `200` `{ turn }`; `409 turn_not_queued` once started |
| POST   | `/api/v1/elysia/questions/{id}/answer` | `200` `{ question }`; queues a turn; `409 question_not_open` |
| POST   | `/api/v1/elysia/questions/{id}/dismiss` | `200` `{ question }`; `409 question_not_open`     |
| POST   | `/api/v1/changesets/{id}/apply`       | `200` `{ changeset }` with each operation's outcome |
| POST   | `/api/v1/changesets/{id}/undo`        | `200` `{ changeset }` with what each undo did       |
| GET    | `/api/v1/llms`                        | `200` `{ llms: Llm[] }`, in priority order          |
| POST   | `/api/v1/llms`                        | `201` `{ llm }`                                     |
| GET    | `/api/v1/llms/{id}`                   | `200` `{ llm }`                                     |
| PATCH  | `/api/v1/llms/{id}`                   | `200` `{ llm }`                                     |
| DELETE | `/api/v1/llms/{id}`                   | `204`                                               |
| POST   | `/api/v1/llms/claude-sign-ins`        | `201` `{ signIn: { id, authorizeUrl, expiresAt } }` |
| POST   | `/api/v1/llms/claude-sign-ins/{id}`   | `201` (new) or `200` (signed in again) `{ llm }`    |
| DELETE | `/api/v1/llms/claude-sign-ins/{id}`   | `204`                                               |
| GET    | `/api/v1/mail/capabilities`           | `200` `{ capabilities }`                            |
| GET    | `/api/v1/mail/server`                 | `200` `{ server }`                                  |
| POST   | `/api/v1/mail/server`                 | `202` `{ server }`; starts creating the mail server |
| GET    | `/api/v1/mail/domains`                | `200` `{ domains: MailDomain[] }`, by name          |
| POST   | `/api/v1/mail/domains`                | `201` `{ domain }`                                  |
| DELETE | `/api/v1/mail/domains/{id}`           | `204`; removes the domain and its DKIM keys         |
| GET    | `/api/v1/mail/domains/{id}/dns`       | `200` `{ records, zoneFile }`, checked live         |
| GET    | `/api/v1/mail/accounts`               | `200` `{ accounts: MailAccount[] }`, by address     |
| POST   | `/api/v1/mail/accounts`               | `201` `{ account }`; creates a self-hosted mailbox  |
| PATCH  | `/api/v1/mail/accounts/{id}`          | `200` `{ account }`                                 |
| DELETE | `/api/v1/mail/accounts/{id}`          | `204`; destroys a self-hosted mailbox and its mail  |
| POST   | `/api/v1/mail/accounts/{id}/test`     | `200` `{ account }` with the check recorded         |
| POST   | `/api/v1/mail/accounts/{id}/test-message` | `200` `{ sentTo }`                              |
| GET    | `/api/v1/mail/oauth/{kind}/start`     | `303` to the OAuth broker                           |
| GET    | `/api/v1/mail/oauth/callback`         | `303` to `/settings/email`                          |
| GET    | `/api/v1/environment-variables`       | `200` `{ variables: EnvironmentVariable[] }`, by key |
| POST   | `/api/v1/environment-variables`       | `201` `{ variable }`                                |
| GET    | `/api/v1/environment-variables/{id}`  | `200` `{ variable }`                                |
| PATCH  | `/api/v1/environment-variables/{id}`  | `200` `{ variable }`                                |
| DELETE | `/api/v1/environment-variables/{id}`  | `204`; a running turn keeps it                      |
| GET    | `/api/v1/learnings`                   | `200` `{ learnings: Learning[] }`, last changed first; `?deleted=true` the deleted ones |
| POST   | `/api/v1/learnings`                   | `201` `{ learning, outcome: "created" }`, or `200` with `revised` or `unchanged` when its title is taken |
| GET    | `/api/v1/learnings/feed`              | `200` `{ entries: { entry, learning }[], nextBefore }`, newest first; admins |
| GET    | `/api/v1/learnings/{id}`              | `200` `{ learning }`, deleted or not                |
| PATCH  | `/api/v1/learnings/{id}`              | `200` `{ learning }`                                |
| DELETE | `/api/v1/learnings/{id}`              | `204`; softly                                       |
| POST   | `/api/v1/learnings/{id}/restore`      | `200` `{ learning }`                                |
| GET    | `/api/v1/learnings/{id}/history`      | `200` `{ history: LearningHistoryEntry[] }`, oldest first |
| POST   | `/api/v1/learnings/{id}/history/{entryId}/undo` | `200` `{ learning }`                      |
| GET    | `/api/v1/page-attachments`            | `200` `{ policy }`: where attachments go and what a page takes; members |
| PUT    | `/api/v1/page-attachments/destination` | `200` `{ policy }`; admins choose `storageLocationId`, or `null` to stop, after the location answers |
| GET    | `/api/v1/pages`                       | `200` `{ pages: PageSummary[] }`, in order among their siblings; `?deleted=true` the deleted ones; members |
| POST   | `/api/v1/pages`                       | `201` `{ page }`, as its version 1; members         |
| GET    | `/api/v1/pages/{id}`                  | `200` `{ page }`, deleted or not; members           |
| DELETE | `/api/v1/pages/{id}`                  | `204`; softly, with the pages under it, or `?keepChildren=true` alone; members |
| POST   | `/api/v1/pages/{id}/publish`          | `200` `{ page, outcome }`, `published` or `unchanged`; members |
| POST   | `/api/v1/pages/{id}/move`             | `200` `{ page }`; members                           |
| POST   | `/api/v1/pages/{id}/restore`          | `200` `{ page }`, with the pages its deletion took; members |
| POST   | `/api/v1/pages/{id}/labels`           | `200` `{ page }`; puts a label on it; members       |
| DELETE | `/api/v1/pages/{id}/labels/{label}`   | `200` `{ page }`; takes a label off it; members     |
| POST   | `/api/v1/pages/{id}/attachments?name=` | `201` `{ attachment, page }`; the body is the file; members |
| GET    | `/api/v1/pages/{id}/attachments/{attachmentId}/{name}` | `200` the file; members |
| DELETE | `/api/v1/pages/{id}/attachments/{attachmentId}` | `200` `{ page }`; takes a file off it; members |
| POST   | `/api/v1/pages/{id}/attachments/{attachmentId}/restore` | `200` `{ page }`; puts a file back; members |
| GET    | `/api/v1/pages/{id}/history`          | `200` `{ history: PageHistoryEntry[] }`, oldest first; members |
| GET    | `/api/v1/pages/{id}/versions`         | `200` `{ versions: PageVersionSummary[] }`, newest first; members |
| GET    | `/api/v1/pages/{id}/versions/{version}` | `200` `{ version: PageVersion }`; members         |
| GET    | `/api/v1/pages/{id}/passages/{position}` | `200` `{ passage: { position, content } }`, `404` when the index holds none there; members |
| POST   | `/api/v1/pages/{id}/versions/{version}/restore` | `200` `{ page, outcome }`, `published` or `unchanged`; members |
| PUT    | `/api/v1/pages/{id}/projects/{projectId}` | `200` `{ page }`, linked to the project; members  |
| DELETE | `/api/v1/pages/{id}/projects/{projectId}` | `200` `{ page }`, off the project; members        |
| GET    | `/api/v1/knowledge/search?q=&source=&project=&person=&label=&from=&to=&limit=` | `200` `{ results: KnowledgeResult[], searchedByMeaning }`, best first; members |
| GET    | `/api/v1/knowledge/documents/{id}?fromPosition=&toPosition=` | `200` `{ document: KnowledgeDocument }`, whoever's record it came from; members |
| GET    | `/api/v1/knowledge/meetings?from=&to=&person=&project=&limit=` | `200` `{ meetings: KnowledgeMeeting[] }`, newest first, whoever recorded them; members |
| POST   | `/api/v1/knowledge/asks`              | `201` `{ ask: KnowledgeAsk }`, queued, or answered when the search found nothing; members |
| GET    | `/api/v1/knowledge/asks`              | `200` `{ asks: KnowledgeAsk[] }`, the asker's 50 newest, newest first; members |
| GET    | `/api/v1/knowledge/asks/{id}`         | `200` `{ ask }`; the asker's alone, `404` for anyone else; members |
| GET    | `/api/v1/learning-categories`         | `200` `{ categories: LearningCategory[] }`, built-in ones first, each kind in its order |
| POST   | `/api/v1/learning-categories`         | `201` `{ category }`; admins                        |
| PATCH  | `/api/v1/learning-categories/{key}`   | `200` `{ category }`; admins                        |
| DELETE | `/api/v1/learning-categories/{key}`   | `204`; admins; `?moveTo=` names where its learnings go |
| GET    | `/api/v1/model-settings`              | `200` `{ kinds: KindModel[], agents: AgentModel[], usageSince, choices }`; admins |
| PUT    | `/api/v1/model-settings/kinds/{kind}` | `200` `{ kind }`; admins; `{ model, effort }`        |
| DELETE | `/api/v1/model-settings/kinds/{kind}` | `200` `{ kind }`; admins; back on its default        |
| PUT    | `/api/v1/model-settings/{agent}`      | `200` `{ agent }`; admins; `{ model, effort }`       |
| DELETE | `/api/v1/model-settings/{agent}`      | `200` `{ agent }`; admins; follows its kind again    |
| GET    | `/api/v1/search?q=&limit=`            | `200` `{ results: SearchHit[] }`: items, projects, sessions, and initiatives by title |
| GET    | `/api/v1/runs`                        | `200` `{ runs: Run[], hasMore }`, newest first, 50 at a time; `?before=` a run id |
| GET    | `/api/v1/runs/{id}`                   | `200` `{ run, steps: Step[], history: RunHistoryEntry[] }` |
| GET    | `/api/v1/runs/{id}/steps/{stepId}/log` | `200` `{ log }`, every attempt's                   |
| POST   | `/api/v1/runs/{id}/steps/{stepId}/retry` | `200` as `GET /runs/{id}`; `409` unless the step failed |
| POST   | `/api/v1/runs/{id}/steps/{stepId}/skip` | `200` as `GET /runs/{id}`; `409` while it runs or once done |
| POST   | `/api/v1/runs/{id}/steps/{stepId}/resume` | `200` as `GET /runs/{id}`; `409` unless the step is held |
| GET    | `/api/v1/github-credentials`          | `200` `{ credentials: GithubCredential[] }`, by name |
| POST   | `/api/v1/github-credentials`          | `201` `{ credential }`; checked with GitHub first   |
| GET    | `/api/v1/github-credentials/{id}`     | `200` `{ credential }`                              |
| PATCH  | `/api/v1/github-credentials/{id}`     | `200` `{ credential }`                              |
| DELETE | `/api/v1/github-credentials/{id}`     | `204`; the token itself stays valid on GitHub       |
| POST   | `/api/v1/github-credentials/{id}/test` | `200` `{ result }` from asking GitHub now          |
| POST   | `/api/v1/github-credentials/{id}/repository-access` | `200` `{ result }` for one repository |
| GET    | `/api/v1/github-credentials/{id}/repositories` | `200` `{ repositories, truncated }` the token can see |
| GET    | `/api/v1/github-credentials/{id}/repositories/{owner}/{name}/issues` | `200` `{ issues, truncated }`, open ones |
| GET    | `/api/v1/github-credentials/{id}/repositories/{owner}/{name}/milestones` | `200` `{ milestones, truncated }` |
| GET    | `/api/v1/github-credentials/{id}/repositories/{owner}/{name}/labels` | `200` `{ labels, truncated }` |
| GET    | `/api/v1/jira-credentials`            | `200` `{ credentials: JiraCredential[] }`, by name  |
| POST   | `/api/v1/jira-credentials`            | `201` `{ credential }`; checked with Jira first     |
| POST   | `/api/v1/jira-credentials/discover`   | `200` what a token can reach; stores nothing        |
| GET    | `/api/v1/jira-credentials/{id}`       | `200` `{ credential }`                              |
| PATCH  | `/api/v1/jira-credentials/{id}`       | `200` `{ credential }`                              |
| DELETE | `/api/v1/jira-credentials/{id}`       | `204`; the token itself stays valid on Atlassian    |
| POST   | `/api/v1/jira-credentials/{id}/test`  | `200` `{ credential, result }` from asking Jira now |
| GET    | `/api/v1/jira-credentials/{id}/projects` | `200` `{ projects, truncated }` it can reach now |
| GET    | `/api/v1/jira-credentials/{id}/boards`   | `200` `{ boards, truncated }` it can reach now   |
| GET    | `/api/v1/jira-credentials/{id}/issues`   | `200` `{ issues, nextPageToken, isLast }`        |
| POST   | `/api/v1/jira-credentials/{id}/issues`   | `201` `{ issue }`                                |
| GET    | `/api/v1/jira-credentials/{id}/issues/{key}` | `200` `{ issue }`, with text and comments    |
| PATCH  | `/api/v1/jira-credentials/{id}/issues/{key}` | `200` `{ issue }`                            |
| GET    | `/api/v1/jira-credentials/{id}/issues/{key}/transitions` | `200` `{ transitions }`          |
| POST   | `/api/v1/jira-credentials/{id}/issues/{key}/transitions` | `200` `{ issue }` after the move |
| POST   | `/api/v1/jira-credentials/{id}/issues/{key}/comments`    | `201` `{ comment }`              |
| GET    | `/api/v1/jira-credentials/{id}/filters`  | `200` `{ filters, truncated }` it can see        |
| GET    | `/api/v1/jira-credentials/{id}/projects/{key}/done-transition` | `200` the project's done statuses and choice |
| PUT    | `/api/v1/jira-credentials/{id}/projects/{key}/done-transition` | `200` `{ chosen }`         |
| DELETE | `/api/v1/jira-credentials/{id}/projects/{key}/done-transition` | `204`                      |
| GET    | `/api/v1/projects`                    | `200` `{ projects: Project[] }`, by name            |
| POST   | `/api/v1/projects`                    | `201` `{ project }`                                 |
| PATCH  | `/api/v1/projects/{id}`               | `200` `{ project }`                                 |
| DELETE | `/api/v1/projects/{id}`               | `204`; `409` while sessions belong to it            |
| GET    | `/api/v1/projects/{id}/cover`         | `200` WebP image; `404` without a cover             |
| PUT    | `/api/v1/projects/{id}/cover`         | `200` `{ project }`; the body is the image file     |
| DELETE | `/api/v1/projects/{id}/cover`         | `200` `{ project }`; the image is kept to restore    |
| POST   | `/api/v1/projects/{id}/cover/restore` | `200` `{ project }`, showing the removed cover again |
| GET    | `/api/v1/projects/{id}/pages`         | `200` `{ pages: ProjectPage[] }`, newest published first; members |
| GET    | `/api/v1/projects/{id}/findings`      | `200` `{ findings: Finding[], total }`: its open findings, newest first, at most 200, and how many are open; members only |
| POST   | `/api/v1/findings/{id}/accept`        | `200` `{ finding, itemId, batchId, issue, issueRefusal }`; `409` `finding_decided`; members only |
| POST   | `/api/v1/findings/{id}/dismiss`       | `200` `{ finding }`; an optional `reason`; `409` `finding_decided`; members only |
| GET    | `/api/v1/projects/{id}/model-settings` | `200` `{ kinds: ProjectKindModel[], usageSince, choices }` |
| PUT    | `/api/v1/projects/{id}/model-settings/{kind}` | `200` `{ kind }`; `{ model, effort }`        |
| DELETE | `/api/v1/projects/{id}/model-settings/{kind}` | `200` `{ kind }`; follows the workspace again |
| GET    | `/api/v1/projects/{id}/autonomy`      | `200` `{ autonomy: { autonomy, setBy, setAt } }` |
| PUT    | `/api/v1/projects/{id}/autonomy`      | `200` `{ project, autonomy }`; `{ autonomy }`; members only |
| GET    | `/api/v1/projects/{id}/project-manager` | `200` `{ projectManager }`; members only     |
| PUT    | `/api/v1/projects/{id}/project-manager/tracker` | `200` `{ projectManager }`; `{ provider, credentialId?, destination }`; members only |
| DELETE | `/api/v1/projects/{id}/project-manager/tracker` | `200` `{ projectManager }`; follows the precedent again; members only |
| GET    | `/api/v1/projects/{id}/project-manager/destinations?provider=&credentialId=` | `200` `{ destinations: [{ reference, name }] }`; members only |
| POST   | `/api/v1/projects/{id}/project-manager/messages` | `201` `{ projectManager }`; `{ body }`, wakes it; members only |
| GET    | `/api/v1/projects/{id}/project-manager/turns/{turnId}` | `200` `{ turn, log }`; members only |
| GET    | `/api/v1/projects/{id}/worker-setup`  | `200` `{ workerSetup }`: its repositories and tools script |
| PUT    | `/api/v1/projects/{id}/worker-setup`  | `200` `{ workerSetup }`; replaces the whole setup   |
| GET    | `/api/v1/promises?followUpItemId=`    | `200` `{ promises: Promise[] }`: the one promise an item follows up, or none; members only |
| GET    | `/api/v1/work-queue`                  | `200` `{ workQueue }`: the pool, its slots, every batch waiting, in order, and every batch proposed; members only |
| GET    | `/api/v1/work-batches/{id}`           | `200` `{ batch, why, history }`; members only       |
| POST   | `/api/v1/work-batches/{id}/approve`   | `200` as `GET`; optional `title`, `goal`, `repositories`, `priority` (with `scheduledFor`) changed first; `409` `work_batch_moved_on`; members only |
| POST   | `/api/v1/work-batches/{id}/reject`    | `200` as `GET`; a required `reason`; `409` `work_batch_moved_on`; members only |
| PATCH  | `/api/v1/work-batches/{id}`           | `200` as `GET`; `priority` (with `scheduledFor`), `paused`, `runNext`; `409` `work_batch_moved_on`; members only |
| POST   | `/api/v1/work-batches/{id}/cancel`    | `200` as `GET`; an optional `reason`; `409` `work_batch_moved_on`; members only |
| GET    | `/api/v1/work-batches/{id}/worker-log` | `200` `{ log }`: its worker's log; members only    |
| GET    | `/api/v1/work-batches/{id}/pull-requests` | `200` `{ pullRequests }`: what its worker recorded; members only |
| GET    | `/api/v1/storage-locations`           | `200` `{ locations: StorageLocation[] }`, by name   |
| POST   | `/api/v1/storage-locations`           | `201` `{ location }`                                |
| GET    | `/api/v1/storage-locations/{id}`      | `200` `{ location }`                                |
| PATCH  | `/api/v1/storage-locations/{id}`      | `200` `{ location }`                                |
| DELETE | `/api/v1/storage-locations/{id}`      | `204`; files already saved stay with the provider; `409` `backup_destination`, `attachment_destination`, or `attachment_files` while one of those holds it |
| POST   | `/api/v1/storage-locations/{id}/test` | `200` `{ result }` from listing its directory       |
| GET    | `/api/v1/coding-sessions`             | `200` `{ sessions: CodingSession[] }`, newest first |
| POST   | `/api/v1/coding-sessions`             | `201` `{ session }`; clones its repositories        |
| PATCH  | `/api/v1/coding-sessions/{id}`        | `200` `{ session }`; renames                        |
| DELETE | `/api/v1/coding-sessions/{id}`        | `204`; stops its turns, removes its directory       |
| GET    | `/api/v1/coding-sessions/{id}/events` | `200` `{ events: SessionEvent[], truncated }`       |
| POST   | `/api/v1/coding-sessions/{id}/turns`  | `201` `{ turn }`; queues a prompt                   |
| GET    | `/api/v1/coding-sessions/{id}/desktop` | WebSocket: the running turn's desktop as VNC, view-only |
| GET    | `/api/v1/auth/status`                 | `200` `{ signupOpen, firstSignUp }`; needs no session |
| POST   | `/api/v1/auth/sign-out`               | `204`, clearing the session cookie; needs no session |
| GET    | `/api/v1/me`                          | `200` `{ user, hasSecondFactor, mfaEnrollmentRequired }`; pending people too |
| DELETE | `/api/v1/me/sessions`                 | `204`; ends every other session of yours; pending people too |
| DELETE | `/api/v1/me/sessions/{id}`            | `204`; ends one other session of yours, `400` for the one asking |
| GET    | `/api/v1/users`                       | `200` `{ users: User[] }`, by name, machines included |
| PATCH  | `/api/v1/users/{id}`                  | `200` `{ user }`; admins: `role`, `disabled`         |
| DELETE | `/api/v1/users/{id}`                  | `204`; admins reject a pending sign-up               |
| POST   | `/api/v1/users/{id}/approve`          | `200` `{ user }`; admins, with a `role`              |
| POST   | `/api/v1/users/{id}/revoke-sessions`  | `204`; admins                                        |
| POST   | `/api/v1/users/{id}/reset-mfa`        | `204`; admins                                        |
| POST   | `/api/v1/users/{id}/recovery-link`    | `200` `{ recoveryLink, expiresAt }`; admins          |
| GET    | `/api/v1/workspace-settings`          | `200` `{ settings }`; admins                         |
| PATCH  | `/api/v1/workspace-settings`          | `200` `{ settings }`; admins: `signupOpen`, `requireMfa`, `agentConfigurationRepository` (`null` clears), `workerPoolSize` (at least 1; `null` follows the default) |
| GET    | `/api/v1/backups`                     | `200` `{ backups }`: where backups go, the next attempt, and history; admins |
| POST   | `/api/v1/backups`                     | `202` `{ backup }`; admins back up now; `409` `backup_running` or `no_backup_destination` |
| PUT    | `/api/v1/backups/destination`         | `200` `{ backups }`; admins choose `storageLocationId`, or `null` to stop, after the location answers |
| GET    | `/api/v1/updates`                     | `200` `{ updates }`: the release running, auto-update's state, and history; admins |
| POST   | `/api/v1/updates/check`               | `200` `{ updates }` after asking Docker Hub now; admins |
| POST   | `/api/v1/updates/apply`               | `200` `{ updates }`; admins ask for the newest release, even one that rolled back |
| POST   | `/api/v1/oauth/login/accept`          | `200` `{ redirectTo }`; answers Hydra's sign-in challenge |
| GET    | `/api/v1/oauth/consent?challenge=`    | `200` `{ client, scopes }`, or `{ redirectTo }` when approved before |
| POST   | `/api/v1/oauth/consent/accept`        | `200` `{ redirectTo }`                               |
| POST   | `/api/v1/oauth/consent/reject`        | `200` `{ redirectTo }`                               |
| GET    | `/api/v1/oauth/grants`                | `200` `{ grants }`: the MCP clients you connected    |
| DELETE | `/api/v1/oauth/grants/{clientId}`     | `204`; disconnects one                               |
| GET    | `/api/v1/oauth/clients`               | `200` `{ clients }`; admins                          |
| DELETE | `/api/v1/oauth/clients/{clientId}`    | `204`; admins, for everyone                          |
| POST   | `/api/mcp`                            | MCP over Streamable HTTP, with a bearer token; see `docs/mcp.md` |
| GET    | `/.well-known/oauth-protected-resource` | `200` the MCP server's protected resource metadata; also under `/api/mcp` |
| POST   | `/oauth2/register`                    | Hydra's answer without its empty fields: an MCP client registers itself; see `docs/mcp.md` |
| GET, PUT, DELETE | `/oauth2/register/{clientId}` | Hydra's answer without its empty fields: a client manages its registration with its token |

Each route lives in its own file under `api/src/routes/`, and the directory mirrors the URL.

## Authentication

Every route under `/api/v1` needs a Kratos session cookie, except `GET /api/v1/auth/status` and
`POST /api/v1/auth/sign-out`. Every route but those and `/me` needs an active, approved person; admin routes need an
admin. Unsafe methods must carry Elysium's own `Origin`.
Refusals carry a stable `code` beside the `message`:

| Status | `code`                     | Meaning                                                        |
|--------|----------------------------|----------------------------------------------------------------|
| `401`  | `unauthenticated`          | No session, or one that ended                                  |
| `403`  | `second_factor_required`   | Signed in with one factor; the authenticator app's code is due |
| `403`  | `account_pending`          | Waiting for an admin's approval                                |
| `403`  | `account_disabled`         | Disabled by an admin                                           |
| `403`  | `mfa_enrollment_required`  | The workspace requires an authenticator app                    |
| `403`  | `admin_only`               | Only an admin may do this                                      |
| `403`  | `members_only`             | A guest asked for the knowledge base, which only admins and members reach |
| `403`  | `cross_origin`             | An unsafe request from another origin                          |

`/internal/kratos/registration`, `/internal/kratos/registered`, `/internal/kratos/settings`, and
`/internal/kratos/courier` answer Kratos alone, on the compose network, with the hook key it carries.
`/internal/updater/instruction` and `/internal/updater/outcome` answer the deployment's updater alone, with a key
derived the same way (`docs/infrastructure.md`). `/internal/backups` and `/internal/backups/{id}`
take the same key and let tooling on the host back up before a deploy (`docs/infrastructure.md`, Backups). nginx
never forwards any of them. See `docs/auth.md`.

Every resource that records its creator returns `createdBy`, a user id; `GET /api/v1/users` names every one.

### `/api/version`

Returns the crate name and version, the release the image was published as (`release`, such as `1.4.2`, or null
for `main` and development builds), build profile, `rustc` version, build timestamp, and git metadata. The git
metadata is the current branch, `HEAD`, and the last 20 commits, each with hash, short hash, author, ISO 8601
date, and subject. `api/build.rs` captures all of it at compile time. A build with no git history reports an empty
history rather than failing.

### `/api/v1/llms`

An `Llm` has `id`, `name`, `description`, `type`, `priority`, `isActive`, `expiresAt`, `createdAt`, `updatedAt`, and
`account`. The secret token is never returned.

`type` is one of `chatgpt-oauth`, `chatgpt-api-token`, `claude-api-token`, `claude-code-oauth`, or `claude-account`.

`account` is `null` except for a `claude-account`, where it is `{ email, organizationName, subscriptionType,
isSignedIn, accessTokenExpiresAt, lastRefreshedAt, lastRefreshError, usage, usageError }`. `usage` is
`{ fiveHour, sevenDay, readAt }`, each window `{ utilization, resetsAt }` or `null`, `utilization` a percentage that can
pass 100. No token is ever included.

A `claude-account` is created only by signing in: `POST /llms/claude-sign-ins` starts the CLI's sign-in and answers the
page to open; `POST /llms/claude-sign-ins/{id}` takes `{ code, name?, priority? }`, the code that page shows, and
answers the credential, named after the account's email unless `name` is given. Both answer `404` for a sign-in that
is not waiting or is someone else's, and `400` with the CLI's reason for a code it refused. `POST /llms` refuses the
type, and `PATCH` refuses a `claude-account`'s `secretToken` or `expiresAt` and any change of type to or from it.

`POST` requires `name`, `type`, and `secretToken`. It accepts optional `description` (default empty), `priority`
(default 0, lower is tried first), `isActive` (default true), and `expiresAt`.

`PATCH` accepts any subset of the same fields. Omitted fields are unchanged. `expiresAt: null` clears the expiry.
A new `secretToken` is re-sealed. An empty body is rejected.

Timestamps sent to the API must include an offset. Responses are always UTC with a `Z` suffix.

### `/api/v1/browser-sessions`

A `BrowserSession` has `id`, `name`, `startUrl`, `port` (its media port), `status`, `createdBy`, `createdAt`, and
`updatedAt`. `status` is `{ state, error }`: `state` is `starting`, `running`, or `failed`, with `error` set when it
failed. The password is never returned.

Every route answers only the browser's creator; anyone else's is `404`. A browser is a connector's account: it starts
with its connector (`POST /connectors`, or `POST /connectors/{id}/sign-in`) and is removed with it
(`DELETE /connectors/{id}`), so nothing here adds or removes one.

`stream` is a WebSocket upgrade. It requires Elysium's own `Origin`, answers `403` otherwise, and `502` when the
browser does not answer. See `docs/browser.md` for what it relays.

### `/api/v1/password-vault`

Admins only; see `docs/passwords.md`. A `PasswordVault` has `server` (`bitwarden-us`, `bitwarden-eu`, or
`self-hosted`), `serverUrl` (a self-hosted server's, else null), `clientId`, `accountEmail`, `collection` (`{ id,
name, organizationId }`, or null until chosen), `checkedAt`, `createdBy`, `createdAt`, and `updatedAt`. `status` is
`{ state, error, lastSyncedAt }`, with `state` one of `unconfigured`, `starting`, `ready`, or `failed`. The client
secret and master password are never returned.

`PUT` takes `server`, `serverUrl` (required for, and only for, `self-hosted`: https, no credentials, query, or
fragment), `clientId` (starting `user.`), and `clientSecret` and `masterPassword`, each required the first time and
kept when left out afterwards. It signs the CLI in and unlocks before storing anything: a refusal answers `400`
`Bitwarden refused: <its words>`, and a CLI that cannot reach the server `502`. The chosen collection is kept when
the account and server are unchanged and still reach it. `collection` takes `{ collectionId }`, one the account
reaches now, or `null` to clear it. `collections` and `sync` answer `503` until the vault is ready, and every route
`404` when no vault is set up.

### `/api/v1/connectors` and `/api/v1/timeline`

A `Connector` has `id`, `kind`, `scope` (`workspace` or `personal`), `account` (`{ id, kind, name }`), `status`
(`active`, `syncing`, `error`, or `paused`), `isActive`, `signInState`, `lastSyncAt`, `lastError`, `records`, `content`
(in its kind's terms; Jira's is its allowlist), `storageLocationId`, `checks` (each check's last result), `lastRun`,
`createdBy`, and `createdAt`; a secret is never returned. `account` is `null` only for a connector read through a
browser that waits for one of its own. `POST` and `POST /checks` take `{ kind, account, content }`, where `account` is
`{ new: { ...its fields } }`: every connector is given a new account, which it alone owns, so `{ id }` is refused. A
kind read through a browser takes no fields: its browser starts with it, `starting`, at the kind's start page, and
`POST /checks` refuses it, since there is nothing to check until its person signs in. Fireflies' content is a
`storageLocationId`. A field a kind does not take is refused. Creating answers `400` for content or an account the
kind cannot use or a secret its provider refuses, `409` (`browser_ports_exhausted`) when every browser media port is
taken, and `502` when the provider cannot be reached. `POST /options` takes `{ kind, account }`, where `account` may
also be `{ id }`, the account a stored connector owns. `POST /{id}/sign-in` answers `400` for a kind that signs in with
a token and `409` for one that has its browser. Someone else's personal connector answers `404`. A `CheckRun` has `id`, `kind`, `connectorId`, `startedAt`,
`finishedAt`, and `checks`, each `{ checkId, required, state, count, reason, message, blockedBy }`. See
`docs/connectors.md`.

A `Meeting` has `id`, `connectorId`, `sourceKind`, `title`, `startedAt`, `durationMs`, `organizer`,
`participants`, `url`, `state` (`pending`, `recorded`, `segmented`, or `done` as far as it got, or `failed` or
`skipped` as its newest run says), `lastError` (why its run failed, while it has),
`hasRecording`, `segmentCount`, `holdForApproval`, `correction` (`{ changedLines, approvedAt, approvedBy,
reviewItemId, createdAt }`, or `null` before it is corrected), `runId` (its newest run of the meeting workflow),
`awaitingApproval` (whether that run is held for the caller's approval), `createdAt`, and `updatedAt`. `sourceKind` is
the kind of connector it came from, such as `fireflies`, and stays when `connectorId` becomes `null` because its
connector was removed.

Every meeting route answers the meeting's owner alone, `404` for anyone else, until meetings are public by default
(`docs/meeting-review.md`, Who sees a meeting). `GET /meetings/{id}/words` reads one snapshot: `words` are its final
transcript's standing words in order, each `{ id, position, text, speaker, startMs, endMs, origin, captureId }`, empty
until it is built; `marks` are its standing marks, each `{ id, kind, startWordId, endWordId, data, createdBy, agent,
createdAt }`, with `agent` `{ id, name }` or `null`; `captures` are `{ id, connectorId, sourceKind, url, offsetMs,
hasRecording }`, first first. `GET .../captures/{captureId}/transcript` answers `{ basedOn, lines }` (`source`, or
`whisper` when the source sent no lines), each line `{ startMs, endMs, speaker, text }` moved by the capture's offset,
or `null` while nothing was heard. An edit takes `{ startWordId, endWordId, text }`: `400` with `meeting_edit_empty`
for text with no words, `meeting_edit_too_long` past 20,000 characters, or a range that ends before it starts. An edit
and an undo answer `entry`, the history entry, and `change`, what it did to the transcript: `{ removedWordIds, words,
marks }`, the words it took out, those it stood in their place, and every mark as it now stands, for the page to apply
without reading the transcript again. A history entry is `{ id, kind, createdBy, agent, data, undid, createdAt }`,
`undid` naming the entry an undo undid. See `docs/meetings.md`, The final transcript and History.

A `TimelineRecord` has `id`, `sourceKind`, `connectorId`, `externalId`, `occurredAt`, `actor`, `title`,
`body`, `url`, `media`, `metadata`, `actionItemId` (the item it raised), `projects` (each `{ projectId, confidence,
attributedBy, createdAt }`), `createdBy`, and `createdAt`. `GET /timeline` takes `projectId`, `actionItemId` (the record
that raised that item), `meetingId` (that meeting's segments), `before` (an `occurredAt`, for the next page), and
`limit` (1 to 200, default 50).
`GET /timeline/{id}/media` streams the clip in `media.clip`, privately cached for good. A change to a record's
attributions answers the record and `entryId`, the history entry it recorded (`null` when nothing changed); a
`HistoryEntry` has `id`, `timelineRecordId`, `projectId`, `kind` (`attributed` or `unattributed`), `createdBy` (the
owner, or Elysia's machine user), `before` and `after` (each `{ confidence, attributedBy }` or `null`), `undid`, and
`createdAt`. Every route answers the record's owner alone, except that `GET /timeline` and `GET /timeline/{id}` also
answer a member the workspace's records, which nobody changes: a Project Manager's turns and batches (`createdBy` its
machine user) and a session's turn summaries (`sourceKind` `turn_summary`, `createdBy` the coding agent's machine user,
`metadata` naming `sessionId`, `turnId`, and `studioItemId`). See `docs/connectors.md`.

### `/api/v1/drafts`

A `Draft` has `id`, `replyToRecordId`, `connectorId` or `mailAccountId` (what sends it), `target` (where it goes, in its
sender's terms; a mail's holds `to` and `subject`), `place` (`thread` or `conversation`, for a chat reply), `body`,
`state` (`draft`, `sending`, `sent`, or `failed`), `lastError`, `attemptedAt`, `sentAt`, `sentRecordId`, `promiseId`
(the promise a follow-up keeps), `createdBy`, `createdAt`, and `updatedAt`. `GET ?recordId=` lists the caller's own
replies alone, never a follow-up drafted for a promise. `POST` takes `{ kind: "reply", recordId, place?, mailAccountId?,
body }`, answering one of the caller's records through what its kind declares (`replies`), or `{ kind: "mail",
mailAccountId, to, subject, body }`. A reply by mail without `mailAccountId` goes from the caller's only active mailbox,
and answers `400` with `no_mailbox` when they have none and `mailbox_required` when they have several. A body is 1 to
20,000 characters. `POST /{id}/send` answers the draft as the send left it: a send the provider refused is `200` with
the draft `failed`, its reason in `lastError` and its text kept. Every route answers the draft's writer alone. See
`docs/connectors.md`, Sending.

### `/api/v1/mail`

A `MailAccount` has `id`, `kind` (`gmail`, `outlook`, `self-hosted`), `address`, `displayName`, `isActive`,
`lastCheckedAt`, `lastError`, `createdAt`, `updatedAt`, and `mailDomainId` (null for OAuth accounts). The
credential is never returned.

`capabilities` is `{ brokerConfigured, brokerError, oauthKinds }`, asking the broker live.

A mail server is `{ state, hostname, step, error }`. `state` is `not-created`, `creating` (with `step`: `preparing`,
`pulling-image`, `starting`, `configuring`, `restarting`, `adding-domain`), `failed` (with `error`), `ready`, or
`unreachable` (with `error`). `POST /server` requires `hostname` and `domain`, both domain names, and answers `409`
when a server exists or is being created; progress arrives as `mailServer.updated` events.

A `MailDomain` has `id`, `name`, `isDefault`, and `createdAt`. `POST /domains` requires `name` and answers `409` when
it exists. `DELETE` answers `409` for the default domain or one with mailboxes. `GET /domains/{id}/dns` answers
`records`, each `{ purpose, recordType, name, value, status, found, error }` as described in `docs/mail.md`, and the
full `zoneFile`. Every domain route answers `503` when no mail server exists.

`POST /accounts` creates a self-hosted mailbox. It requires `localPart` and `domainId` and accepts `displayName`; it
answers `400` for an unknown domain, `503` when no mail server exists, and `409` when the address exists. `PATCH`
accepts `displayName` and `isActive`. `test` answers the account with `lastError` set or cleared; a failing mailbox
is not an error status. `test-message` answers `502` with the server's message when sending fails. The OAuth routes
are browser navigations; see `docs/mail.md`.

### `/api/v1/projects`

A `Project` has `id`, `name`, `description`, `createdAt`, `updatedAt`, `coverUpdatedAt` (null without a cover), and
`coverFit` (`fit` or `fill`, how clients frame the cover), and `github`: `{ access, credentialId }`, how the project
picks its sessions' GitHub token. `access` is `default` (the workspace default), `none`, or `specific` with the token's
`credentialId`; `specific` with a null id means the chosen token was deleted and the project follows the default.
`autonomy` is its trust dial (`docs/autonomy.md`, The dial): `approve_every_batch`, the default, `approve_found_work`,
or `full_autonomy`. `POST` requires `name` (1 to 120 characters, unique) and accepts `description` (up to 2000,
default empty), and creates the project's brief with it, announced as `page.changed` (`docs/autonomy.md`, The project
brief). `PATCH` accepts any of `name`, `description`, `coverFit`, and `github`; an empty body is rejected, and a new
`name` renames the brief too. A `github` whose `credentialId` is missing for `specific`, present for another access,
or names no token answers `400`. `DELETE` answers `409` while any coding session belongs to the project.

`PUT /{id}/cover` takes the image file as the raw body: PNG, JPEG, WebP, or GIF (first frame), up to 10,000,000
bytes; this route alone raises the API's 1 MiB body limit, and nginx's, to allow it. The format is read from the bytes,
not the content type. Decoding refuses images over 8192 pixels on a side or 256 MiB of pixels. The image is scaled down
to fit 2400 by 1350 when larger, keeping its shape, and stored as lossy WebP at quality 80, keeping transparency and
dropping metadata (`api/src/images.rs`). Anything refused answers `400`; a larger body answers `413`.
`GET /{id}/cover` serves it with `Cache-Control: private, max-age=31536000, immutable`: clients add `coverUpdatedAt` to
the URL, so each cover version has its own. Cover changes publish `project.upserted`.

`GET /{id}/autonomy` answers `{ autonomy: { autonomy, setBy, setAt } }`: the project's level, with the person who set
it and when, both `null` while it is at the default it started with. `PUT /{id}/autonomy` takes `{ autonomy }` and
nothing else, records the change as the caller, and answers `{ project, autonomy }`; choosing the level the project has
changes nothing. It takes a company member, so a guest is refused `403` and no agent's tool reaches it (`members_only`).
A change publishes `project.upserted` and `projectManager.changed`, and applies to what the Project Manager submits and
changes from then on.

`GET /{id}/worker-setup` answers `{ workerSetup: { toolsSetupScript, repositories } }`: how the project readies its
workers' containers (`docs/engine.md`, Workers). `toolsSetupScript` is bash run as root in every worker's container
before anything is cloned, or `null`; `repositories` lists what the project declares, in order, each `{ id, url,
baseBranch, setupScript, mergePolicy }`, `setupScript` being bash run as the agent in the repository's directory once
it is cloned and `mergePolicy` who merges its workers' pull requests, `person` or `auto` (`docs/engine.md`, Merging).
`PUT` takes the same shape without ids and replaces the whole setup: at most 16 repositories, each held to a coding
session's rules (`docs/coding.md`, Repositories), none listed twice and no two cloning into one directory ignoring
case, a base branch of at most 255 characters, scripts of at most 20,000 characters, a script of only whitespace
being none, and `mergePolicy` `person` unless given and `auto` only for a github.com repository; anything refused
answers `400` naming the rule. Anyone who may edit the project reads and changes it, and
a change publishes `projectWorkerSetup.changed`; the next worker that starts uses it.

`GET /{id}/project-manager` answers the project's Project Manager (`docs/engine.md`, Project Managers): `status`
(`running`, `held`, `errored`, `waiting`, or `idle`), `waiting` (how many wakes wait for its next turn),
`usagePausedUntil` (while a usage limit holds it), its latest 30 `turns`, newest first, its latest 100 `batches`, newest
first, every batch of its `proposed`, newest first, up to 200, and its latest 100 `promises`, newest first. A turn has `id`, `status` (`running`, `succeeded`, `failed`),
`summary`, `reply`, `error`, `timelineRecordId`, `startedAt`, `finishedAt`, and `wakes`, what woke it: each `{ id, kind,
timelineRecordId, workBatchId, promiseId, pageVersion, body, createdBy, createdAt }`, `kind` being `record`, `batch`,
`message`, `promise`, or `brief` (someone else published the project's brief, as version `pageVersion`). A batch has
`id`, `title`, `goal`, `serves` (the goal in the project's brief it serves, `null` for a batch from before briefs or
one a person made by accepting a finding), `repositories` (`[{ url, baseBranch? }]`), `priority` (`asap`, `normal`,
`scheduled`), `scheduledFor`, `state` (`proposed`, `queued`, `paused`, `assigned`, `running`, `succeeded`, `failed`,
`cancelled`, `rejected`),
`result`, `turnId`, `sourceRecordIds`, `actionItemIds`, `sourceMessageIds` (the messages to its Project Manager that
called for it), `origin` (`human_rooted` or `found`), `generation` (how many batches led to it), `parentBatchIds` (the
batches it was planned from), `timelineRecordId`, `projectId`, `pinnedAt`, `assignedAt`, `createdAt`, `startedAt`, and
`finishedAt`, as `/api/v1/work-batches` answers it (`docs/engine.md`, Origins and generations). A `Promise` has
`id`, `projectId`, `state` (`waiting`, `due`, `drafted`, `kept`, `dismissed`), `recipient`, `summary` (what was promised), `madeIn` (`{
recordId, sourceKind, title, url, occurredAt, personId }`, the message it was made in and whose timeline it is on, or
null once that message is gone), `workBatchId` or `workItemId` (the work it waits on), `dueAt`, `followUpItemId`,
`draftId` (its follow-up's draft, which only its person may read), and `createdAt`; `/api/v1/promises` answers the same
shape (`docs/engine.md`, Follow-ups). `writes` says how its changes reach the work (`docs/engine.md`, Its writes):
`direct`, whether they apply at once, true at the project's Full autonomy; `tracker`, where its issues file, `{ source,
provider, credentialId, credentialName, destination }` with `source` `named` or `precedent`, or null with
`trackerRefusal` saying why. A project nothing woke yet answers `idle` with none. `PUT
/{id}/project-manager/tracker` names the tracker: `provider`, `destination` as the destinations listing names it, and
`credentialId`, a Jira credential or a YouTrack account the caller owns, left out for GitHub, which files with the
project's token; the place is read through that credential before it is stored, and a GitHub tracker on a project
whose sessions get no token answers `400`. `DELETE` follows the precedent again. `GET
/{id}/project-manager/destinations` lists the places a tracker on `provider` could file, through the same credential.
`POST
/{id}/project-manager/messages` takes `{ body }` (1 to 20,000 characters, not blank) as the person's message, which
wakes it at once, and answers it as `GET` does. `GET /{id}/project-manager/turns/{turnId}` answers one turn with its
`log`. Its turns quote what was ingested, so all three answer the company's members alone (`403` for a guest).

`DELETE /{id}/cover` is soft: the cover stops showing (`coverUpdatedAt` is null and `GET /{id}/cover` answers `404`),
but its image is kept until a new cover replaces it, so a project keeps at most one removed cover.
`POST /{id}/cover/restore` shows it again with its old `coverUpdatedAt`, so the cached image still serves. Both answer
the project as it is when there is nothing to remove or restore, so an Undo after a new upload keeps the new cover.

### `/api/v1/work-queue` and `/api/v1/work-batches`

The worker pool and the batches it runs (`docs/engine.md`, Workers and the pool). A batch quotes what was ingested, so
every route answers admins and members, and a guest `403 members_only`.

`GET /work-queue` answers `{ workQueue: { pool, inSlots, next, scheduled, paused, proposed } }`. `pool` is `{ size,
configuredSize, defaultSize, cpuCount, inUse, usageHeldUntil }`: `size` is what is in force, `configuredSize` an
admin's choice (`null` follows the default), `defaultSize` and `cpuCount` what the engine last saw (`null` before it
has looked, when `size` is `null` too unless an admin chose one), and `usageHeldUntil` set while every Claude account is
at its usage limit. `inSlots` lists the batches holding slots, in the order they got them; `next` every batch the pool
may hand a slot now, first first; `scheduled` the scheduled batches not yet due, soonest first; `paused` the paused
ones; and `proposed` those waiting for a person's approval, oldest first, which the pool hands nothing. Each entry is
`{ id, projectId, title, priority, scheduledFor, state, pinnedAt, assignedAt, createdAt, startedAt, origin, generation,
cause }`, without its goal, `cause` being the head of its chain (the first link of `why.chain` below that is neither the
batch nor a pull request), or `null` for a batch that names nothing;
at most the 1,000 oldest waiting batches are read, and apart from them at most the 1,000 oldest proposed.

`GET /work-batches/{id}` answers `{ batch, why, history }`: the batch whole, as `GET /projects/{id}/project-manager`
lists it; `why`, `{ chain, parents }`, its chain from what asked for it to its pull requests (`docs/engine.md`, Why a
batch exists), each link tagged by `kind`: `finding` `{ id, title, acceptedById, acceptedBy, acceptedAt }`, `message`
`{ id, authorId, author, body, sentAt }`, `record` `{ id, sourceKind, title, occurredAt, excerpt, meetingTitle,
documentId }` with `excerpt` its first 600 characters and `documentId` the knowledge base's document a member opens it
by (`null` before it is indexed), `issue` `{ actionItemId, provider, key, url, title }`, `item` `{ id, title, state }`,
`batch` `{ id, projectId, title }`, and `pullRequest` `{ id, repository, number, url, title, state }`, leaving out
anything gone since, and the batches it was planned from, each `{ id, title, state, generation }`; and its
history oldest first, each entry `{ id, kind, data, createdBy, createdAt }`
(`docs/engine.md`, Work batches). A running batch whose worker waits, keeping its container, also carries `waitingOn` (`review`, `ci`, or
`question`), `waitingDetail`, and `waitingUntil`, all `null` otherwise. `GET /work-batches/{id}/worker-log` answers `{
log }`: what its worker said and which tools it called, one line each, scrubbed of its secrets, the newest 64 KiB,
empty before it started. `GET /work-batches/{id}/pull-requests` answers `{ pullRequests }`: the pull requests its
worker recorded, in order, each `{ id, workBatchId, repository, number, url, title, state, mergedAt, mergedBy,
mergedByElysium, mergeMethod, heldBecause, lookedAt, timelineRecordId, createdBy, createdAt }`, `state` being `open`,
`merged`, or `closed` and `heldBecause` why the last look left it open (`docs/engine.md`, Merging). `PATCH` takes any of `priority` (`asap`, `normal`, or `scheduled` with `scheduledFor`, with
its offset, in the future and within a year), `paused` (true holds back a batch no worker started on, giving back a
slot it held; false lets a paused one wait again), and `runNext` (true runs a due, queued batch before everything
else; false puts it back in its place), applied in that order in one transaction, each recorded as the caller; a
change that moves nothing records nothing. `POST /cancel` takes an optional `reason` (up to 20,000 characters), which
its Project Manager is told. A proposed batch takes neither a priority, a pause, nor run next: `POST /approve` queues
it, first changing any of `title` (1 to 200 characters), `goal` (1 to 20,000), `repositories` (replacing them all, at
most 16, each held to a coding session's rules and named once), and `priority` (with `scheduledFor` for a scheduled
one), each trimmed as a submission is, recording an `edited` entry with only what changed and then `approved`; and
`POST /reject` takes a required `reason` (1 to 20,000 characters), which ends it `rejected` and wakes its Project
Manager with it. Both record the person who decided, and answer `409` `work_batch_moved_on` once it is no longer
proposed; no machine user can call either, since each takes a member's session. Both answer as `GET` does; a change the batch's state no longer allows answers `409` with
`code` `work_batch_moved_on`, and one that breaks a rule whatever the state (a time in the past, pausing and running
next at once) `400`.

### `/api/v1/findings`

Work a worker or a Project Manager noticed that nobody asked for, waiting for a person (`docs/engine.md`, Findings). A
finding quotes agents' work on the project, so every route answers admins and members, a guest `403 members_only`, and
only a member decides, which no machine user is.

`GET /projects/{id}/findings` answers `{ findings, total }`: the project's open findings, newest first, at most 200, and
how many are open in all, which its Findings tab counts; `404` for a project that does not exist. Each finding is `{
id, projectId, title, detail, repositoryUrl, location, cost, state, workBatchId, projectManagerTurnId, reports,
lastReportedAt, decidedBy, decidedAt, dismissalReason, actionItemId, acceptedBatchId, createdBy, createdAt }`, `cost`
being `small`, `medium`, or `large` and `state` `open`, `accepted`, or `dismissed`.

`POST /findings/{id}/accept` takes no body. In one transaction it makes an open action item in the finding's project as
the person, owes its issue in the project's tracker, filed as them, when the project has one, submits a batch for the
item, human-rooted and generation 0, proposed or queued as the project's level of autonomy says, and records the
finding accepted. It answers the finding, the item's and the batch's ids, `batchState` (`proposed` or `queued`), and
`issue`, `{ provider, destination }` where the issue files, or `null` with `issueRefusal` saying why it
files nowhere. `POST /findings/{id}/dismiss` takes an optional `reason`, up to 2,000 characters, which agents that
report the finding again are told, and answers the finding. Both answer `404` for an unknown finding and `409` `code`
`finding_decided` once someone decided it.

### `/api/v1/storage-locations`

A `StorageLocation` has `id`, `name`, `provider`, `pathPrefix`, `storageLimitBytes`, `projects`, `createdAt`, and
`updatedAt`. `projects` is `"*"` for every project, or an array of project ids.
The access key is never returned. `provider` is one of:

- `{ kind: "bunny", zone, region }`, with `region` one of `frankfurt`, `london`, `new-york`, `los-angeles`,
  `singapore`, `stockholm`, `sao-paulo`, `johannesburg`, or `sydney`.
- `{ kind: "s3", service, bucket, region, accessKeyId }`, with `service` either `aws` (`region` required, such as
  `us-east-1`) or `google-cloud` (`region` null).

`POST` requires `name` (1 to 120 characters, unique), `provider`, and `accessKey`, and accepts `pathPrefix` (default
the root; surrounding slashes are dropped), `storageLimitBytes` (1 to 2^53 - 1; absent or `null` for no limit), and
`projects` (default none). An unknown project id answers `400`.
`PATCH` accepts any subset; `provider` replaces all of the provider's settings, `projects` replaces the projects,
`storageLimitBytes: null` removes the limit, and a new `accessKey` is re-sealed. A `provider` with another Bunny zone,
S3 service, or access key id answers `400` unless it comes with its `accessKey`.

`test` answers `{ entries, hasMore }`: the files and directories directly inside the location's directory, up to one
page, and whether it holds more. It answers `502` with the provider's error. See `docs/storage.md`.

### `/api/v1/github-credentials`

A `GithubCredential` has `id`, `name`, `kind` (`classic` or `fine-grained`), `login`, `scopes`, `tokenExpiresAt`,
`checkedAt`, `createdAt`, and `updatedAt`. Everything from `login` on is what GitHub answered when the token was last
checked. The token is never returned.

`POST` requires `name` (1 to 120 characters, unique), `kind`, and `token` (up to 255 characters, surrounding
whitespace dropped). `PATCH` accepts any subset of `name`, `kind`, and `token`; a new token is re-sealed. A token
written the wrong way for its kind answers `400` without a call to GitHub, and a `kind` that differs from the stored
one without a `token` answers `400`.

Every write checks the token with `GET https://api.github.com/user` and stores it only if GitHub accepts it: a
rejected token answers `400` with what to check, and an unreachable GitHub `502`.

`isDefault` marks the workspace default. `PATCH` accepts `isDefault`: `true` replaces the previous default in the same
transaction, and both credentials go out on the event stream; `false` leaves no default.

`test` answers `{ login, scopes, tokenExpiresAt }` from asking GitHub now, and records it on the credential.
`repository-access` takes `{ repositoryUrl }` for a github.com remote and answers
`{ repository, canRead, canPush, isPrivate }`, with `canPush` null when unknown. `repositories` answers
`{ repositories, truncated }`, each repository `{ fullName, owner, name, private, archived, defaultBranch, cloneUrl,
pushedAt, canPush }`, most recently pushed first, up to 1,000. For linking, a repository's `issues` answers its open
issues, or its open pull requests with `kind=pull-request`, up to a hundred most recently updated, each
`{ reference, number, title, url, assignee }`; `milestones` answers its open milestones `{ reference, number, title,
url }` and `labels` its labels `{ reference, name, url }`, up to 500 of each. `reference` is what a link request
names. See `docs/github.md`.

### `/api/v1/issue-reports`

Both take `{ subject, note? }`. `subject` is tagged by `kind`: `connectorSync` (`connectorId`, optional `runId`),
`connectorCheck` (`connectorId`, `checkId`), `checkRun` (`runId`, `checkId`), or `error` (`area`, lower-case words
joined by dots or underscores, and `message`, up to 4,000 characters). `note` holds up to 2,000 characters.

`preview` answers the `draft` (`title`, `body`, `fingerprint`) exactly as it would be posted, the repository, the
caller's own GitHub tokens each with its `readiness` (`ready`, `fine_grained`, `missing_scope`, `expired`), the one to
preselect, the caller's last report of this problem, and how many reports they have left today. Filing also takes
`credentialId`, one of the caller's own tokens, and answers `{ report: { filedAs, issueNumber, issueUrl, url } }`,
`filedAs` being `issue` or `comment`.

A subject with nothing to report answers `400`; one the caller cannot see, or a check run that expired, `404`. A token
that cannot report answers `400` with the code `github_token_cannot_report`, a person who already reported the problem
to its open issue `409` `already_reported`, and one past the daily limit `429` `report_limit`. See
`docs/issue-reporting.md`.

### `/api/v1/jira-credentials`

A `JiraCredential` has `id`, `name`, `siteUrl`, `accountEmail`, `accountId`, `displayName`, `projects`, `boards`,
`checkedAt`, `createdAt`, and `updatedAt`. `projects` is `"*"` or an array of `{ id, key, name }`, and `boards` is
`"*"` or an array of `{ id, name, projectKey }`. The token is never returned.

Adding one is two steps: `POST /discover` with `{ siteUrl, accountEmail, token }` answers the account, projects,
and boards that token can reach without storing anything, and `POST /` then creates the credential with the picks
made from it. Every write checks the token with Jira and matches the picks against what Jira reports; a pick it
does not report answers `400` naming it.

Issue routes are bounded by the credential's projects: a search's JQL is rewritten rather than its results
filtered, and a project outside the list answers `403` naming the project and the credential. Paging is by
`nextPageToken`, not an offset. `filters` lists the saved filters the token can see, for linking one to an initiative,
and `done-transition` reads, chooses, or forgets the `done` status a project's issues move into when their item
resolves. Every shape, rule, and page cap is in `docs/jira.md`.

### `/api/v1/environment-variables`

An `EnvironmentVariable` has `id`, `key`, `isSecret`, `description`, `value`, `createdAt`, and `updatedAt`. `value` is
the plaintext of a non-secret variable and `null` for a secret one, whose value is never returned.

`POST` requires `key`, `value` (up to 32 KiB, kept exactly as sent), and `isSecret`, and accepts `description` (up to
500 characters, default empty). `PATCH` accepts any subset; an absent `value` keeps the stored one. A key that breaks
a rule answers `400` naming the key and the rule, a secret with an empty value answers `400`, and making a secret
visible without sending its `value` answers `400`. A duplicate key answers `409`. See `docs/environment.md`.

### `/api/v1/company-doc`

Every route answers admins and members, and a guest `403 members_only`. See `docs/knowledge.md`, The company doc.

| Method and path                                   | Does                                                         |
|---------------------------------------------------|--------------------------------------------------------------|
| `GET /company-doc`                                | Every section in order, each with its pending proposals      |
| `PATCH /company-doc`                              | Writes `sections`, a map of section keys to Markdown, as one change |
| `GET /company-doc/history`                        | Every change, oldest first                                   |
| `POST /company-doc/history/{entryId}/undo`        | Reverses one change, section by section                      |
| `POST /company-doc/proposals/{id}/approve`        | Writes a proposal into its section, merged with what changed since |
| `POST /company-doc/proposals/{id}/reject`         | Turns a proposal down; the doc is left as it is              |
| `POST /company-doc/review`                        | Starts the review now, or answers the one under way          |

The doc answers `sections`, each with `section`, `purpose` (what belongs there, in English, for agents), `content`,
`updatedAt`, and `proposals`. A `CompanyDocProposal` has `id`, `section`, `baseContent`, `content`, `reason`, `quote`,
`source`, `timelineRecordId`, `workflowRunId`, `state`, `decidedBy`, `decidedAt`, `createdBy`, `agent` (`{ id, name }`
or `null`), and `createdAt`; a history entry has `id`, `kind`, `createdBy`, `proposalId`, `data`, and `createdAt`.

The doc's sections are pages (`docs/knowledge.md`, Pages, Learnings and the company doc), which these routes read and
write in the shapes above. `PATCH` trims each section and answers `400` for one past 12,000 characters or a key that is
not a section; sections left out keep their text. Approving and rejecting answer `{ proposal, doc }`, and `409` for a proposal already decided;
approving one whose lines changed since answers `409 proposal_overlaps` and leaves it pending. Undo answers the doc,
and `409` when the change has nothing left to reverse. Review answers `{ run: { id, state }, started }`: `201` with a
new run, `200` with the one already live.

Nothing creates a proposal over HTTP: the review, the MCP server's `company_doc_propose`, and agents' tool of the same
name call `crate::company_doc::propose` in-process.

### `/api/v1/agent-guidance`

The guidance every connected agent reads as its session starts. `GET` answers admins and members, so anyone can copy
its `CLAUDE.md` block; the writes answer admins alone (`403 admin_only` otherwise); a guest gets `403 members_only`.
See `docs/knowledge.md`, Agent guidance.

| Method and path                                   | Does                                                         |
|---------------------------------------------------|--------------------------------------------------------------|
| `GET /agent-guidance`                             | The current version, every version, the rules, and the `CLAUDE.md` block |
| `PUT /agent-guidance`                             | Saves `content` as the next version, made against `baseVersion` |
| `POST /agent-guidance/versions/{version}/restore` | Makes an earlier version's text current again, as the next version |

Every route answers the same: `current` and `versions` (newest first), each `{ version, content, restoredFrom,
createdBy, createdAt }`; `rules`, the text Elysium adds after the guidance for every agent; `claudeMd`, the current
version as a block for a repository's `CLAUDE.md`; and `maxCharacters`, the longest guidance this deployment takes.

`PUT` trims `content`, answers `400` past `maxCharacters`, and `409 guidance_changed` when `baseVersion` is no longer
the current version or another save took the next number meanwhile. Text the current version already holds records
nothing, and neither does restoring a version whose text is current. Restoring a version that does not exist answers
`404`. A write that adds a version announces `agentGuidance.changed`.

### `/api/v1/learnings`

Every route answers admins and members, and a guest `403 members_only`; the feed answers admins alone.

A learning is a page, and its `id` is the page's (`docs/knowledge.md`, Pages, Learnings and the company doc), which these
routes read and write in the shapes below. A `Learning` has `id`, `title`, `body`, `category`, `evidence`, `agent`
(`{ id, name }` or `null`), `createdBy`, `createdAt`, `updatedAt`, and `deletedAt`. A `LearningHistoryEntry` has `id`, `learningId`, `kind`, `createdBy`,
`agent`, `data`, and `createdAt`.

`POST` requires `title` (up to 300 characters, trimmed and its spacing collapsed), `body` and `evidence` (each up to
20,000 characters, trimmed, not blank), and `category`, the key of a learning category (below); a key no category has
answers `400` naming every key there is. A title a learning already has, in any case, revises
that learning, bringing it back if it was deleted. `PATCH` takes any of the four. A title another learning has answers
`409`, as does correcting a deleted learning. Undo answers `409` when the change has nothing left to reverse. The feed
takes `limit` (1 to 200, default 50) and `before`, an entry id, to read on. See `docs/knowledge.md`, Learnings.

### `/api/v1/pages`

The knowledge base's pages, one tree for the whole workspace (`docs/knowledge.md`, Pages). Every route answers admins
and members, and a guest `403 members_only`. `page.changed` carries an id alone, once for each page a change touched;
load it with `GET /{id}`, which answers a deleted page too.

A `Page` has `id`, `parentId` (`null` at the top of the tree), `position`, `title`, `body` (Markdown), `category` and
`evidence` (a learning's, `null` on any other page), `currentVersion`, `proposalOnly`, `fixed`, `briefProjectId` (the
project whose brief it is, `null` on any other page), `labels` (in order),
`attachments` (the oldest first), `publishedBy`, `publishedAt`, `agent` (`{ id, name }` or `null`, for the newest
version), `createdBy`, `createdAt`, `updatedAt`, `deletedAt`, and `projectIds`, the projects it is linked to, oldest link
first. A `PageSummary` has `id`, `parentId`, `position`, `title`, `currentVersion`, `fixed`, `briefProjectId`,
`labels`, `updatedAt`, and `deletedAt`: the list builds the tree by `parentId`, only the order of `position` among
siblings means anything, and its `labels` are all a client needs to filter the tree, list a label's pages, and suggest
the labels in use. `updatedAt` moves when a page is published, deleted, or restored, and not with a move, a label, or a
link. A `ProjectPage` is a `PageSummary` with `publishedBy`, `publishedAt`, `linkedBy`, and `linkedAt`. A
`PageVersionSummary` has `pageId`,
`version`, `title`, `restoredFrom`, `createdBy`, `agent`, and `createdAt`; a `PageVersion` adds `body`, `category`, and
`evidence`. A `PageHistoryEntry` has `id`, `pageId`, `kind` (`created`, `published`, `moved`, `deleted`, `restored`,
`labeled`, `unlabeled`, `attached`, `detached`, `project_linked`, or `project_unlinked`), `createdBy`, `agent`, `data`,
and `createdAt`; `data` by kind is in `docs/knowledge.md`, Pages, History.

`POST /` takes `title` (1 to 300 characters, trimmed and its spacing collapsed), and optionally `parentId` and `body`
(up to 100,000 characters, its line endings normalized and its trailing whitespace dropped), and appends the page to
its parent's children. `POST /{id}/publish` takes `baseVersion`, the `currentVersion` the edit was made from, `title`,
and `body`, the page whole, and on a learning `category` and `evidence`, both or neither: left out, it keeps its own,
and one alone answers `422`. A category no one has answers `400` naming every key there is, as does a learning's body
outside 1 to 20,000 characters or a company doc section's past 12,000. `POST /{id}/move` takes `parentId` and `after`, both required and either `null`: the page
it goes under, or the top of the tree, and the live sibling it goes right after, or first. `POST /{id}/labels` takes
`label`, stored lowercase with spaces as hyphens; it, and the `{label}` of `DELETE /{id}/labels/{label}`, answer `422`
for anything but 1 to 50 letters, digits, hyphens, underscores, and dots, starting with a letter or digit. Neither makes
a version (`docs/knowledge.md`, Pages, Labels), and neither does a link to a project, which takes any page that is not
deleted; an unknown page or project answers `404`. Moving a page where it already is, publishing or restoring what it
already says, putting on a label it carries, taking off one it does not, or making or removing a link that is already
as asked, records nothing and announces nothing; a publish answers `unchanged`. `DELETE /{id}?keepChildren=true` deletes the page alone: its live children take its place
among its siblings, each move recorded in that child's history as part of the deletion, and `POST /{id}/restore` moves
back those still where it left them.

Refusals are `409` with a `code` the web app explains in its own words:

| `code`                | When                                                                            |
|-----------------------|---------------------------------------------------------------------------------|
| `page_changed`        | A publish named a `baseVersion` that is no longer current; read the page again   |
| `page_deleted`        | A publish, restore of a version, move, or change of a label or project link on a deleted page |
| `page_proposal_only`  | An agent wrote a page agents only propose to, or would delete one with its parent |
| `page_fixed`          | A move, a deletion, or a new title on the Learnings, Company, or Project briefs page, a section, or a project's brief |
| `page_brief_linked`   | Taking a project's brief off that project, which only deleting the project does   |
| `page_title_taken`    | A learning was given a title another learning has, deleted ones included         |
| `page_not_a_learning` | A publish named a category and evidence for a page that is no learning           |
| `page_parent_gone`    | The parent named does not exist or is deleted, or a deleted page's parent still is |
| `page_placement_gone` | `after` is not a live child of the parent named; load the tree again             |
| `page_under_itself`   | A move named the page itself, or a page under it, as its parent                  |
| `page_label_limit`    | The page already carries 50 labels, as many as a page may                        |
| `page_is_a_learning`  | An agent's `page_save` named a learning, or a new page directly under Learnings  |
| `page_attachment_limit` | The page already holds 200 attachments, as many as a page may                  |
| `attachment_no_location` | Nothing can be attached until an admin chooses a storage location           |

An `Attachment` has `id`, `pageId`, `name`, `url` (the address a page's Markdown refers to it by), `sizeBytes`, `isImage`
(whether it displays in a page; anything else is a link that downloads), `createdBy`, `agent` (`{ id, name }` or `null`),
and `createdAt`.

`POST /{id}/attachments?name=<file name>` takes the file as its body, with its `Content-Length`, which it needs before
a byte moves, and whatever `Content-Type` it was sent with: what a file is comes from its name. It streams straight to the
storage location, and has a deadline of ten minutes rather than the usual 30 seconds (`docs/security.md`, Request
hygiene). A refused file answers `400` with a `code`, or `413` for a size, and leaves nothing with the provider:

| `code`                    | When                                                                        |
|---------------------------|-----------------------------------------------------------------------------|
| `attachment_too_large`    | Its length is over 50 MB (`413`)                                            |
| `attachment_empty`        | It holds nothing                                                            |
| `attachment_name`         | Nothing is left of its name once it is cleaned                              |
| `attachment_type`         | Its extension is not one a page takes                                       |
| `attachment_not_an_image` | It is named as an image, and its first bytes are not that image             |

`GET /{id}/attachments/{attachmentId}/{name}` finds the file by its id, whatever `{name}` says, and answers a removed
one `404`. Its `Content-Type` and `Content-Disposition` follow `docs/security.md`, Page attachments. `DELETE` and `restore`
record nothing for a file already removed or already back. All of these answer a deleted page's files as `GET` and
refuse a change with `409 page_deleted`. A `Policy` from `/api/v1/page-attachments` has `destinationId`, `null` until an
admin chooses one, `maxBytes`, and `extensions`, lowercase and without their dots.

### `/api/v1/learning-categories`

The list answers admins and members, and a guest `403 members_only`; changes answer admins alone.

A `LearningCategory` has `key`, `label`, `description`, `position`, `builtIn`, `learningCount` (the learnings filed
under it, deleted ones included), `createdBy`, `createdAt`, and `updatedAt`.

`POST` requires `key` (a lowercase letter, then up to 49 lowercase letters, digits, or underscores), `label` (up to 60
characters), and `description` (up to 500), each trimmed and not blank, and adds it after the workspace's last; a key
any category has answers `409`. `PATCH` takes `label`, `description`, and `position`, its place among the workspace's
own counted from 0, past the end meaning last; the key never changes. A built-in category answers `409` to `PATCH` and
`DELETE`. `DELETE` of a category with learnings answers `409 category_in_use` unless `moveTo` names another category,
which they move to first, each as a revision in its history. See `docs/knowledge.md`, Categories.

### `/api/v1/model-settings`

Every route answers admins alone, and anyone else `403 admin_only`: it decides what every turn costs.

A `KindModel` has `name` (`worker`, `project_manager`, `correction`, `segmenting`, `triage`, `elysia`, or
`summaries`), `default` (its `{ model, effort }`), `setting` (the admin's `{ model, effort, updatedAt }`, or `null`
while it runs on its default), `runsInProjects` (whether a project may override it), and `usage` (what its turns spent
since `usageSince`, 30 days back: `{ turns, costUsdMicros, inputTokens, outputTokens }`, or `null` where its turns
record no cost). The kinds come in the order the page lists them.

An `AgentModel` has `name` (a step's name or an agent's, such as `elysia`), `isStep`, `kind` (its kind, or `null` for a
step that runs no model), `setting` (the admin's choice for it alone, or `null` while it follows its kind), and
`resolved` (what its turns run on outside any project, `{ model, effort, source }`, where `source` is `agent`,
`kind`, or `default`; `null` for a step that runs no model). The list holds every step of every declared workflow,
first to last, then every agent.

`choices` is what every page that sets a model offers: `suggestedModels`, the current Claude models, strongest first;
`modelPrices`, each one's `{ model, inputCentsPerMillionTokens, outputCentsPerMillionTokens }`; and `efforts`, the
CLI's levels, lowest first.

`PUT` takes `model`, trimmed, an id or alias the CLI takes (a letter or digit, then up to 99 letters, digits, dots,
underscores, hyphens, or brackets), and `effort` (`low`, `medium`, `high`, `xhigh`, or `max`); anything else answers
`400`. A choice equal to what it would follow without one (for a kind, its default; for an agent, its kind's setting
or default) removes the setting. A name no kind, step, or agent has answers `404`, and a step that runs no model
`409`. Each change is recorded in the workspace's history and announced, an agent's as `modelSetting.changed` and a
kind's as `kindModelSetting.changed`; one that changes nothing announces nothing. See `docs/engine.md`, Models.

`/api/v1/projects/{id}/model-settings` answers whoever may edit the project. A `ProjectKindModel` has `name`, one of
the kinds that run in projects (`worker`, `project_manager`, `summaries`); `setting`, the project's own choice or
`null`; `workspace`, what the kind follows without it (`{ model, effort, source }`, `source` `kind` or `default`); and
`usage`, as a `KindModel`'s but the project's turns alone. `PUT` and `DELETE` take and answer one kind the same way; a
kind that works for the whole workspace answers `409 kind_not_in_projects`, and a project or kind that does not exist
`404`.

### `/api/v1/runs`

Every route answers admins and members, and a guest `403 members_only`: a run reads what was ingested.

A `Run` has `id`, `workflow`, `sourceKind`, `sourceId`, `state` (`queued`, `running`, `held`, `failed`, or
`succeeded`), `error` (the failed step's reason), `createdBy`, `createdAt`, `startedAt`, and `finishedAt`. A `Step`
has `id`, `runId`, `step` (its name), `state` (`pending`, `queued`, `running`, `held`, `failed`, `skipped`, or
`succeeded`), `summary`, `error`, `attempts`, `availableAt` (when a step backing off is tried again), `startedAt`, and
`finishedAt`; its input and output stay on the server. A `RunHistoryEntry` has `id`, `runId`, `stepJobId`, `kind`,
`createdBy`, `data`, and `createdAt`. A step is reached through its own run: another run's id answers `404`. Each
action records who took it in the run's history and signals the engine. See `docs/engine.md`, Runs.

### `/api/v1/action-items`

An `ActionItem` has `id`, `title`, `notes`, `state` (`inbox`, `open`, `resolved`, `dismissed`), `priority` (`urgent`,
`high`, `normal`, `low`), `dueAt`, `snoozedUntil`, `waitingOn`, `owner`, `resolvedAt`, `dismissedAt`, `deletedAt`,
`projectIds`, `projectAttributions` (each of its projects as `{ projectId, confidence, attributedBy }`: how sure whoever
put it there was, 1 for anyone but the attribution step, and the actor), `leadProjectId` (the one of its projects that
leads it, set exactly while it is in any), `managerAssignment` (`{ assignedAt, assignedBy }` while a person has it
assigned to its lead project's Project Manager, or null), `initiativeIds` (the initiatives it is in now, leaving out
deleted ones),
`createdAt`, `updatedAt`, and
`issueFiling`, the issue it is owed and not yet filed, `{ id, provider, destination, attempts, lastError,
lastAttemptAt, createdAt }`, or null. `owner` is `{ kind: "user" }`, `{ kind: "other", name }`, or `{ kind: "nobody"
}`. See `docs/action-items.md`.

`GET /` takes `state` (one or more, comma separated), `project` (an id, or `none` for items in no project),
`initiative`, `waiting` and `snoozed` (`true` or `false`), and `deleted=true` for deleted items only; without
`deleted`, deleted items are left out.

`POST` requires `title` (1 to 500 characters) and accepts `notes` (up to 20,000), `priority` (default `normal`),
`dueAt`, `owner` (default the user), `projectIds`, and `initiativeIds`. The item starts `open`. An unknown project, or
an initiative that does not exist or is deleted, answers `400`. `PATCH` accepts any of `title`, `notes`, `priority`,
`dueAt` (`null` removes it), and `owner`; an empty body is rejected. `snooze` requires `until`, a future moment or
`null` to end the snooze. `wait` requires `on`, a name or address up to 320 characters or `null` to stop waiting.
The transitions answer `409` when the item's state does not allow them. Every write to a deleted item but `restore`
answers `409`, as do deleting a deleted item and restoring one that is not.

`POST /bulk` applies one change to many items, as a multi-select asks: `{ itemIds, change }`, with 1 to 1,000 ids
(repeats count once; `MAX_ITEMS` in `src/models/action_item_bulk.rs`) and `change` an object whose `kind` says what it
does: `state` with `state` (`open`, `resolved`, or `dismissed`; each item takes the transition that leads there from its
own state, and one already there is left as it is), `priority` with `priority`, `snooze` with `until` (as the item's own
route), `join-initiative` or `leave-initiative` with `initiativeId`, `assign-to-manager` or `unassign-from-manager` (for
the company's members alone, as on one item), `delete`, or `restore`. Each item goes through the same write its own
route makes, in one transaction, recording its own history with the person as the actor; there is no changeset, since
the person acts directly. An item the records refuse (unknown, deleted, or in a state no transition leaves for the one
asked) is left as it was and listed in `refused` as `{ itemId, message }`, while the rest apply, so the answer is `200`
with `items`, every item the change applied to as it stands now. An unknown initiative to join answers `404` and a
deleted one `409`, for the whole change. Items are written in the order of their ids, so two changes over the same items
never deadlock. Events go out as for each item's own route, each initiative once.

Membership routes are idempotent: adding an item to a project or initiative it is in, or removing it from one it is
not in, answers the item unchanged. Adding to an unknown project or initiative answers `404`, and joining a deleted
initiative `409`. Deleting a project takes its items and initiatives out of it, each recording the removal. An item's
first project leads it, and when its lead leaves, the surest of those left leads instead
(`docs/action-items.md`, Projects and the lead). `PUT /lead-project` makes one of its projects its lead, and answers
`400` for a project it is not in.

`PUT /manager-assignment` assigns the item to its lead project's Project Manager, which wakes to plan it, as the
person; an item in the inbox is accepted as it is assigned, and one already assigned answers unchanged. An item no
project leads, or one resolved, dismissed, or deleted, answers `409`. `DELETE` takes it back, dropping the wake if no
turn has taken it, and answers unchanged for an item not assigned. `GET` answers `{ plan }`: `{ projectId, assignedAt,
assignedBy, stage, turnId, batches }`, where `stage` is `waiting`, `planning`, `planned`, or `failed`, `turnId` is the
turn that took the assignment once one did, and `batches` are the lead project's latest ten naming the item, newest
first, each `{ id, title, state, createdAt }`; `plan` is null while the item is not assigned. All three answer guests
`403`, since Project Managers and batches are the company's members' (`docs/action-items.md`, Assigning to a Project
Manager). Each write publishes the item and `projectManager.changed` for the lead project.

`POST /history/{entryId}/undo` reverses one `project_added`, `project_removed`, or `lead_project_changed` entry as the
person, recording an entry of its own that names the one it undid (`data.undid`). Only the latest change to that
project, or to the lead, is undone: an entry with a change made since answers `409` with why, and an entry of any other
kind answers `400`.

A `Comment` has `id`, `actionItemId`, `author`, `body` (1 to 20,000 characters), `createdAt`, and `updatedAt`, which
equals `createdAt` until the comment is edited. `POST` and `PATCH` take `{ body }`. Editing or deleting a comment
someone else wrote answers `409`.

A `HistoryEntry` has `id`, `actionItemId`, `initiativeId`, `kind`, `actor`, `data`, `createdAt`, and `changesetId`
(the changeset whose applying or undoing made the change, or null); kinds and their `data` are listed in
`docs/action-items.md`. Every write over HTTP is recorded with the actor `user`; a comment a coding agent writes through
`elysium_work` carries `session:<number>`, a change the watcher reads from a provider `watcher:jira` or
`watcher:github`, and an applied changeset's changes its proposer, `elysia` or `session:<number>`.

Resolving an item owes a close to every linked issue still open, and writing a comment owes it to the item's primary
link; the watcher lands both (`docs/action-items.md`), so the response does not wait on the provider.

#### Links

An `ActionItemLink` has `id`, `actionItemId`, `provider` (`jira`, `github`), `kind` (`issue`, `pull-request`),
`credentialId`, `key` (`ELY-12`, `owner/name#12`), `url`, `title` (as last read), `isPrimary`, `state` (`open`,
`done`, `not-planned`, `merged`, `closed-unmerged`, as last read), `owner` (whose it was when last read, shaped like
an item's), `pendingWrites`, `createdAt`, and `updatedAt`. Each pending write has `id`, `kind` (`close`, `comment`),
`commentId`, `attempts`, `lastError`, `lastAttemptAt`, and `createdAt`; a link with any is pending.

`POST /{id}/links` takes `{ provider, credentialId, kind, reference }`, where `reference` names what a picker listed:
an issue key for Jira, `owner/name#12` for GitHub. The thing is read through the credential first: outside a Jira
credential's allowlist answers `403`, a thing the credential cannot see `404`, the wrong `kind` `400`, and a thing
already linked to another item `409`. Linking the same thing again answers it unchanged. The item's first link becomes
its primary and its owner follows the thing's assignee. `POST /from-link` takes the same fields plus optional
`projectIds` and `initiativeIds`, and creates the item from the thing: title, priority, due date, and owner, starting
`open`.

`GET /{id}/links/remote` reads every link live and answers `{ remotes: [{ linkId, remote, error }] }`, one entry per
link, `remote` null with an `error` for one that could not be read. A `remote` has `key`, `url`, `title`, `state`,
`status` (the provider's own word: a Jira status, or `open`, `closed`, `merged`), `owner`, `assignee` (the name the
provider shows), `priority` (the provider's own), and `dueDate` (a day, Jira only).

`PUT /{id}/links/{linkId}/primary` makes that link primary and answers every link of the item. Removing the primary
promotes the oldest link left. `DELETE .../writes/{writeId}` cancels a write that has not landed.

#### Containers

An `InitiativeLink` has `id`, `initiativeId`, `provider`, `kind` (`epic`, `filter`, `milestone`, `label`),
`credentialId`, `key`, `url`, `title`, `syncedAt`, `syncError`, `truncated`, `createdAt`, and `updatedAt`. `POST
/initiatives/{id}/links` takes `{ provider, credentialId, kind, reference }`: an epic's key or a saved filter's id for
Jira, `owner/name#3` for a milestone, or `owner/name:label`. The container is read through the credential first, and
its children join on the watcher's next pass, which the link wakes at once. Unlinking takes out the items it brought in.

### `/api/v1/changesets`

A `Changeset` has `id`, `proposer` (`elysia`, `session:<number>`, or `project_manager:<project id>`), `projectId` (the
project a coding session or a Project Manager proposed it in, or null), `summary`, `state` (`pending`, `applied`,
`rejected`, `undone`), `decidedAt`, `decidedBy` (the person who applied or rejected it, or the Project Managers'
machine user for changes applied without review), `undoneAt`, `operations`, `createdAt`, and `updatedAt`. An operation
has `id`, `position` (from 1), `operation` (an object whose `kind` says what it does, shaped as `docs/action-items.md`
lists), `reason`, `quote`, `source`, `causeRecordId` (the timeline record that caused it, or null), `dependsOn` (the
positions of the earlier operations it acts on), `decision` (`pending`, `approved`, `rejected`), `outcome` (`pending`,
`applied`, `failed`, `skipped`, `undone`), `error` (why it failed or was skipped), `result` (the ids it acted on or
created, and the values it replaced), and `undo` (null until the changeset is undone; then any of `kept`,
`movedSince`, `stillPosted`, `stillClosed`, `stillFiled`, and `refusal`).

Nothing creates a changeset over HTTP; proposers stage them in-process (`work_propose_changes` and
`manager_make_changes` today). `GET /` takes
`state`, one or more, comma separated.

`decide` takes `{ decision, operationIds }`, where `decision` is `approved`, `rejected`, or `pending`, and
`operationIds` names the operations, or every one when it is left out. Rejecting an operation rejects every operation
that depends on it; approving one whose dependency stays rejected answers `400` naming both, as does an operation that
is not in the changeset. `apply` answers `400` while any operation is undecided, then applies the approved ones as
`docs/action-items.md` describes: one that fails is marked with the reason and the rest apply, so the answer is `200`
with each outcome. `undo` reverses an applied changeset once. `decide` and `apply` on a changeset that is no longer
pending, and `undo` on one that is not applied, answer `409`. Every write publishes `changeset.upserted`, and applying
and undoing publish every item, initiative, comment, link, and history entry they changed.

### `/api/v1/initiatives`

An `Initiative` has `id`, `name`, `description`, `state` (`active`, `achieved`, `abandoned`), `targetAt`,
`projectIds`, `progress` (`{ resolved, total }` now), `deletedAt`, `createdAt`, and `updatedAt`.

`GET /` takes `state`, `project`, and `deleted` like the item list. `POST` requires `name` (1 to 200 characters) and
accepts `description` (up to 20,000), `targetAt`, and `projectIds`; it starts `active`. `PATCH` accepts any of `name`,
`description`, `targetAt` (`null` removes it), and `state`, in any direction. Deleting keeps its items as members;
they stop listing it until it is restored.

`POST /bulk` applies one change to many initiatives, as the item route does: `{ initiativeIds, change }`, with 1 to
1,000 ids and `change` one of `{ kind: "state", state }`, `{ kind: "delete" }`, or `{ kind: "restore" }`. Each goes
through its own route's write in one transaction, and one the records refuse is listed in `refused` as
`{ initiativeId, message }` while the rest apply (`src/models/initiative_bulk.rs`).

`progress` answers `{ resolved, total, burnup }`, with `burnup` a list of `{ at, resolved, total }`: a point at the
initiative's creation, one at every moment either count changed, and one now.

### `/api/v1/coding-sessions`

A `CodingSession` has `id` (the session's number, 1, 2, 3, ...), `projectId`, `title`, `githubCredentialId`,
`actionItemId` (the item it was started from, or null), `studioItemId`, `createdAt`, `createdBy`, `updatedAt`, and
`activity`: what it is doing now, `{ state, queueDepth, currentTurnId, lastActivityAt }`, with `state` one of `idle`,
`queued`, or `running`.

`POST` requires `projectId` and `title` (1 to 200 characters), and accepts `repositories`, up to 16
`{ url, baseBranch? }` cloned in order, and `githubCredentialId` (absent follows the project, `null` asks for no token, and an id names one;
an unknown id answers `400`). Two repositories that would clone into the same directory, ignoring case, answer `400`
naming both URLs, and a clone that fails answers `502` with git's message; see `docs/coding.md`. Sessions carry
`githubCredentialId`, the token their turns use. `prompt` (1 to 100,000 characters) is queued as the first turn. `actionItemId` starts the session from that
item: `prompt` is then required (`400` without it), the project must be one of the item's projects when it has any
(`400` otherwise), an unknown item answers `404`, and a deleted one `409`. The first turn carries the item's context
ahead of the prompt; see `docs/action-items.md`. `turns` requires `prompt` (1 to 100,000 characters)
and answers `{ turnId, status, prompt, queuedAt }`; the turn's progress arrives on the event stream.

A `SessionEvent` has `sessionId` (the session's number), `sequence`, `turnId`, `occurredAt`, `type` (such as
`agent.message`), and `payload`. `payload` is null for event types the API does not render; otherwise its `kind` is
one of `agentMessage`, `agentThinking`, `toolStarted`, `toolCompleted`, `turnStarted`, `turnCompleted`, or
`incident`. Field shapes are in `api/src/sessions/views.rs`.

`events` answers `{ events, truncated, summaries }`. A `TurnSummary` has `sessionId`, `turnId`, `summary` (one line,
at most 400 characters), and `createdAt`; see `docs/coding.md`, Turn summaries. Behavior, policy, and history limits are in `docs/coding.md`.

### `/api/v1/search`

The global search's records (`docs/frontend.md`, Global search). `GET` takes `q` (up to 200 characters) and
`limit` (1 to 50, default 5), and answers the closest `limit` action items, projects, coding sessions, initiatives,
and pages whose titles match, each kind in its own order. A `SearchHit` is `{ kind, id, title }` with `kind` one of
`actionItem`, `project`, `codingSession` (its `id` the session's number), `initiative`, or `page`; items and
initiatives add their `state`. Nothing more is returned: a result is a row, and its page loads the rest.

- **Matching.** Every word typed must begin a word of the title, case ignored; a word is a run of letters and digits,
  so `way 123` finds `WAY-123`, and a query of only punctuation finds nothing. Accents must match as typed.
- **Ranking.** A title that begins with the typed words comes first, then the closest by pg_trgm's `similarity`,
  then alphabetically. The web app ranks every source's results again with one score.
- **Visibility.** Each kind is searched among exactly what its list route shows: items and initiatives not deleted,
  every project, the Coding area's sessions (Studio's are left out), and live pages. The first four lists answer
  everyone with access to the workspace, guests included, so search does too; pages answer admins and members alone,
  so a guest's search holds none, with no refusal. The knowledge base has a search of its own,
  `/api/v1/knowledge/search` (below), which the web app asks beside this one for admins and members alone
  (`docs/knowledge.md`, Who sees what). It finds what pages say; this finds their titles as they are typed.
- **Budget.** Each person may search 20 times at once, refilling 5 a second, on top of the limit per client address;
  past it, `429` with the code `search_rate_limited`. Both are constants (`PER_PERSON` in `src/search/mod.rs`).

The pieces every kind's query shares are in `src/search/`; each model's `search` owns its own query.

### `/api/v1/knowledge`

The knowledge base's search, its documents, its meetings, and asking it (`docs/knowledge.md`, Hybrid search and
Asking). Every route answers admins and members, and a guest `403 members_only`; none is limited by whose record a
document came from.

`GET /search` takes `q` (up to 500 characters; nothing but spaces answers no results), `source` (comma-separated
source names, up to 32, each a lowercase letter then letters, digits, or underscores), `project` and `person` (ids),
`label` (a page label, read as a label is written, which keeps pages alone), `from` and `to` (moments with an offset;
`from` before `to`), and `limit` (1 to 50, default 10). Anything else is `400`, a value out of bounds `422`. It answers the documents found best first, and whether the query was searched by
meaning as well as by words.

A `KnowledgeResult` is `{ citation, passages, alsoRecordedBy, score }`: the `Citation` (`documentId`, `source`,
`title`, `url` or `null`, `pageId`, the page it was indexed from or `null`, `occurredAt`, and `person`, `{ id, name
}`), up to three `{ position, content }` passages
best first, the other documents holding the same text (`{ documentId, person }`, earliest first), and its fused score,
comparable within one answer only.

A `KnowledgeDocument` is `{ citation, origin, passages, passageCount, projects, labels, alsoRecordedBy }`: every passage
in order, or those from `fromPosition` to `toPosition` (both optional and inclusive, from 0; `fromPosition` after
`toPosition` is `422`), how many passages it has in all, the projects its timeline record is attributed to
(`{ id, name }`), the labels its page carries, and `origin`, what it was made from: `{ kind: "timelineRecord" }`, whose record stays its owner's and
is not named, `{ kind: "learning", learningId }`, `{ kind: "companyDoc", section, pageId }`, or `{ kind: "page", pageId }`. An id
the index does not hold is `404`.

`GET /meetings` takes `from` and `to` (moments with an offset, when a meeting started; `from` before `to`), `person`
and `project` (ids: meetings recorded by that person's sources, or with a topic attributed to that project), and
`limit` (1 to 50, default 20). A `KnowledgeMeeting` is `{ title, source, startedAt, person, topics }`: the meeting's
title and the source it was read from (`fireflies`), when its first topic began, whose source recorded it, and its
topics as `Citation`s in the order they were said, each opened with `GET /documents/{id}`. It is gathered from the
index's meeting topics alone; the meeting itself stays its owner's (`docs/knowledge.md`, MCP).

- **Budget.** Each person may search 20 times at once, refilling 2 a second, on top of the limit per client address,
  shared with their MCP clients' and their sessions' searches; past it, `429` with the code
  `knowledge_search_rate_limited` (`PER_PERSON` in `src/knowledge/search.rs`). Opening a document and listing meetings
  embed nothing and spend none of it.

`POST /asks` asks for a written answer (`docs/knowledge.md`, Asking): `{ question, sources, project, person, from, to }`,
the question 1 to 500 characters and the filters as `GET /search` takes them, in JSON. It searches as the asker,
spending one of their searches, and answers `201` with the ask: `queued` for the engine, or `answered` with nothing
to say when the search found nothing. The answer is written afterwards, and `knowledgeAsk.changed` says when the ask
moves on. A `KnowledgeAsk` is `{ id, question, filters, state, answer, error, searchedByMeaning, createdAt,
startedAt, finishedAt }`: `state` is `queued`, `running`, `answered`, or `failed`; `filters` holds those asked for;
`answer`, once answered, is `{ claims, sources, unsupportedClaims }`, each claim `{ text, sources }` citing sources by
number, each source `{ number, citation, position }`, a `Citation` and the position of the passage it cites, and
`unsupportedClaims` how many statements were left out for citing nothing offered; `error`, once failed, says why.

- **Asks.** Each person may ask 20 questions in any hour, with 3 waiting for an answer at once, counted from the asks
  recorded under a lock of the asker's, so asks made at once count each other; past either, `429` with the code
  `knowledge_ask_rate_limited` and a message saying when to ask again (`ASKS_PER_WINDOW` and `WAITING_PER_PERSON` in
  `src/knowledge/ask.rs`).

### `/api/v1/coding-sessions/{id}/desktop`

A WebSocket upgrade carrying the running turn's desktop as VNC (RFB) bytes in binary messages, view-only. It requires
Elysium's own `Origin` (`403` otherwise), answers `404` for an unknown session or when no turn is running, and `502`
when the workspace cannot be reached. See `docs/coding.md`, The desktop.

### Errors

Every error is JSON with a `message`.

| Status | Cause                                                                                 |
|--------|---------------------------------------------------------------------------------------|
| `400`  | Malformed JSON or query, unknown field, unknown enum value, malformed id in the path, blank or oversized token, refused environment variable key, a call an upstream reads as malformed such as invalid JQL |
| `403`  | The credential the request was made with may not touch what it named; see `docs/jira.md` |
| `404`  | No row with that id                                                                   |
| `409`  | Unique constraint, such as a duplicate LLM name, a project that still has sessions, or a change the record's state refuses |
| `422`  | Field validation failed; `fields` lists each failure                                  |
| `429`  | Past a limit the code sets, such as the problem reports a person may file a day; `code` names it |
| `502`  | The workspace, a mail server, a storage provider, or the OAuth broker failed; `message` is its own error |
| `503`  | The mail service a request needs is not configured, or the mail server does not exist |
| `500`  | Internal fault; details are logged, never returned                                    |

## Commands

With no subcommand the binary serves HTTP.

| Command                               | Purpose                                   |
|---------------------------------------|-------------------------------------------|
| `elysium-api serve`                   | Serve HTTP (the default)                  |
| `elysium-api migrate <action>`        | Manage migrations, see `docs/database.md` |
| `elysium-api generate-encryption-key` | Print a new `ELYSIUM_ENCRYPTION_KEY`      |

## Startup and shutdown

On start, the server loads an environment file if one is present, installs tracing, and parses `Config` from the
environment. It then validates the encryption key, connects to Postgres, refuses to continue if any migration is
pending, connects to Redis, starts readying the agent workspace (see `docs/coding.md`) and the link watcher (see
`docs/action-items.md`), binds, and serves. Both store connections retry with exponential backoff, 8 attempts over
roughly a minute, and prove themselves with `SELECT 1` and `PING`.

On `SIGTERM` or `SIGINT`, a shutdown token is cancelled first. That ends every open event stream, agent turn, and
the link watcher, which would otherwise keep the drain waiting forever. Running turns are killed and recorded as
`interrupted`. The listener stops accepting, in-flight
requests drain, the watchers are awaited, then the Postgres pool closes and the Redis connection drops. Compose allows
30 seconds for this before `SIGKILL`.

## Configuration

| Variable                   | Default          | Purpose                                    |
|----------------------------|------------------|--------------------------------------------|
| `DATABASE_URL`             | required         | Postgres connection string                 |
| `REDIS_URL`                | required         | Redis connection string                    |
| `ELYSIUM_ENCRYPTION_KEY`   | required         | Base64 32-byte key sealing stored secrets  |
| `ELYSIUM_PUBLIC_URL`       | required         | The origin browsers use: https, or http on localhost |
| `KRATOS_PUBLIC_URL`        | required         | Kratos's public API, where sessions are checked |
| `KRATOS_ADMIN_URL`         | required         | Kratos's admin API, where identities are managed |
| `HYDRA_ADMIN_URL`          | required         | Hydra's admin API: OAuth consent and token introspection |
| `HYDRA_PUBLIC_URL`         | required         | Hydra's public API, where MCP clients' registrations are relayed |
| `HOST` / `PORT`            | `0.0.0.0` / 8080 | Bind address                               |
| `DATABASE_MAX_CONNECTIONS` | 10               | Pool ceiling                               |
| `CORS_ALLOWED_ORIGINS`     | empty            | Comma-separated origins; empty allows none |
| `REQUEST_TIMEOUT_SECONDS`  | 30               | Handler deadline, answers `408`            |
| `MAX_REQUEST_BODY_BYTES`   | 1048576          | Body cap, answers `413`                    |
| `RUST_LOG`                 | `info,...`       | tracing filter                             |
| `LOG_FORMAT`               | compact          | `json` for one JSON object per line        |
| `OAUTH_BROKER_URL`         | empty            | OAuth broker for browsers; empty disables Gmail and Outlook |
| `OAUTH_BROKER_INTERNAL_URL`| `OAUTH_BROKER_URL` | OAuth broker as the API reaches it       |
| `DOCKER_URL`               | `http://docker-proxy:2375` | Docker API the mail server runs through |
| `MAIL_INGRESS_ADDRESS`     | empty            | nginx's address on the mail network; Stalwart trusts its PROXY headers |

Connection strings and the key are held as `SecretString`, so they never appear in debug output.

Rate limits are deliberately not configurable. The limit is 10 requests per second per client IP, with a burst
of 30. Both are constants in `src/middleware/rate_limit.rs`. The search and the knowledge search have a budget per
person besides (`/api/v1/search` and `/api/v1/knowledge`, Budget).

## Code layout

| Path                     | Holds                                            |
|--------------------------|--------------------------------------------------|
| `src/server.rs`          | Startup, middleware stack, shutdown              |
| `src/routes/`            | One file per route                               |
| `src/models/`            | Diesel records and the queries for each table    |
| `src/database/`          | Pool type, migration runner, generated schema    |
| `src/crypto.rs`          | Secret sealing                                   |
| `src/errors.rs`          | `ApiError` and its mapping to HTTP responses     |
| `src/state.rs`           | `AppState`: browsers, pool, Redis, cipher, version, bus, runner, sessions, GitHub, Jira, links, mail, storage, shutdown token |
| `src/realtime/`          | Event bus over Redis, and the `ServerEvent` envelope |
| `src/runner/`            | Agent turns: the workspace container, the Claude Code CLI, credentials, and workspace files |
| `src/sessions/`          | Coding and Studio sessions on the runner: directories, turns, recorded events, activity |
| `src/jira/`              | Jira Cloud's REST API and Atlassian Document Format |
| `src/mail/`              | IMAP and SMTP transport, OAuth broker client, mail server hosting and administration, DNS checks |
| `src/browser/`           | The browsers the host streams: their containers, the reconciler, Neko's API, the stream relay, and the DevTools client |
| `src/connectors/`        | Accounts, kinds and their checks, sync activity, the webmail reader and its page scripts, and the ingest loop |
| `src/meetings/`          | Meeting sources, one per kind that provides meetings (`sources.rs`), the listing and processing loop, the meeting workflow's steps (fetching, transcription, aligning, correction), segmentation, merging a meeting recorded twice (`duplicates.rs`), and clips cut with `ffmpeg` |
| `src/storage/`           | Storage provider clients, reached through `Storage` |
| `src/issue_reports/`     | Problem reports filed on Elysium's GitHub repository: subjects, redaction, fingerprints, rendering, and filing, see `docs/issue-reporting.md` |
| `src/environment/`       | The rules for which environment variable keys are refused |
| `src/action_items/`      | Action item rules: actors, state transitions, Next's order, initiative progress, a session's first turn from an item, link providers (`links/`), the link watcher, and changesets' operations, dependencies, and decisions |
| `src/workflows/`         | Workflow runs: the step registry, starting a run, announcing what moved, and the engine's step workers |
| `src/tools/`             | The MCP servers agents call at `/api/agent-tools`: `elysium_storage`, `elysium_work`, and Elysia's `elysia`, see `docs/coding.md` |

## Logging

Logs are structured `tracing` events with dotted names such as `connection.open.success` and
`migration.change.success`, using OpenTelemetry attribute names. Every request gets an `x-request-id`, either
client-supplied or a generated UUID. The id is echoed on the response and attached to the request span.

## Testing

`cargo test` runs the hermetic unit tests: encryption, request parsing and validation, refused environment variable
keys, event envelopes, the runner's stream parsing and failover rules, the action item rules (transitions, Next's order, progress, a
session's first turn from an item, and what a provider's change does to a linked item), GitHub's client against a
local fake, and the agent tools' schemas.
`api/scripts/verify-migrations.sh` also runs the store-backed tests against a disposable Postgres and Redis.

## Roadmap

- Authentication. The LLM and coding routes write credentials and drive agents, so nginx publishes on loopback
  by default until auth exists.
- TLS to Postgres for deployments where the database is not on a private network.
