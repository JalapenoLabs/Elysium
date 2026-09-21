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
| GET    | `/api/v1/action-items`                | `200` `{ items: ActionItem[] }`, newest first       |
| POST   | `/api/v1/action-items`                | `201` `{ item }`                                    |
| GET    | `/api/v1/action-items/next`           | `200` `{ items: ActionItem[], inboxCount }`, in Next's order |
| GET    | `/api/v1/action-items/{id}`           | `200` `{ item }`, deleted or not                    |
| PATCH  | `/api/v1/action-items/{id}`           | `200` `{ item }`                                    |
| DELETE | `/api/v1/action-items/{id}`           | `204`; soft, restorable                             |
| POST   | `/api/v1/action-items/{id}/restore`   | `200` `{ item }`                                    |
| POST   | `/api/v1/action-items/{id}/accept`    | `200` `{ item }`; also `resolve`, `dismiss`, `reopen` |
| POST   | `/api/v1/action-items/{id}/snooze`    | `200` `{ item }`                                    |
| POST   | `/api/v1/action-items/{id}/wait`      | `200` `{ item }`                                    |
| GET    | `/api/v1/action-items/{id}/history`   | `200` `{ history: HistoryEntry[] }`, oldest first   |
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
| GET    | `/api/v1/initiatives`                 | `200` `{ initiatives: Initiative[] }`, by name      |
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
| GET    | `/api/v1/changesets`                  | `200` `{ changesets: Changeset[] }`, newest first    |
| GET    | `/api/v1/changesets/{id}`             | `200` `{ changeset }`                               |
| POST   | `/api/v1/changesets/{id}/decide`      | `200` `{ changeset }`; approves or rejects operations |
| POST   | `/api/v1/changesets/{id}/apply`       | `200` `{ changeset }` with each operation's outcome |
| POST   | `/api/v1/changesets/{id}/undo`        | `200` `{ changeset }` with what each undo did       |
| GET    | `/api/v1/llms`                        | `200` `{ llms: Llm[] }`, in priority order          |
| POST   | `/api/v1/llms`                        | `201` `{ llm }`                                     |
| GET    | `/api/v1/llms/{id}`                   | `200` `{ llm }`                                     |
| PATCH  | `/api/v1/llms/{id}`                   | `200` `{ llm }`                                     |
| DELETE | `/api/v1/llms/{id}`                   | `204`                                               |
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
| DELETE | `/api/v1/environment-variables/{id}`  | `204`; running threads keep it                      |
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
| DELETE | `/api/v1/projects/{id}/cover`         | `200` `{ project }`                                 |
| GET    | `/api/v1/satellites`                  | `200` `{ satellites: Satellite[] }`, by name        |
| POST   | `/api/v1/satellites`                  | `201` `{ satellite }`                               |
| GET    | `/api/v1/satellites/{id}`             | `200` `{ satellite }`                               |
| PATCH  | `/api/v1/satellites/{id}`             | `200` `{ satellite }`                               |
| DELETE | `/api/v1/satellites/{id}`             | `204`; forgets its sessions                         |
| POST   | `/api/v1/satellites/{id}/test`        | `200` `{ result }` from a fresh connection          |
| GET    | `/api/v1/storage-locations`           | `200` `{ locations: StorageLocation[] }`, by name   |
| POST   | `/api/v1/storage-locations`           | `201` `{ location }`                                |
| GET    | `/api/v1/storage-locations/{id}`      | `200` `{ location }`                                |
| PATCH  | `/api/v1/storage-locations/{id}`      | `200` `{ location }`                                |
| DELETE | `/api/v1/storage-locations/{id}`      | `204`; files already saved stay with the provider   |
| POST   | `/api/v1/storage-locations/{id}/test` | `200` `{ result }` from listing its directory       |
| GET    | `/api/v1/coding-sessions`             | `200` `{ sessions: CodingSession[] }`, newest first |
| POST   | `/api/v1/coding-sessions`             | `201` `{ session }`; opens a thread                 |
| PATCH  | `/api/v1/coding-sessions/{id}`        | `200` `{ session }`; renames                        |
| DELETE | `/api/v1/coding-sessions/{id}`        | `204`; destroys the thread                          |
| GET    | `/api/v1/coding-sessions/{id}/events` | `200` `{ events: SessionEvent[], truncated }`       |
| POST   | `/api/v1/coding-sessions/{id}/turns`  | `201` `{ turn }`; queues a prompt                   |

Each route lives in its own file under `api/src/routes/`, and the directory mirrors the URL.

### `/api/version`

Returns the crate name and version, build profile, `rustc` version, build timestamp, and git metadata. The git
metadata is the current branch, `HEAD`, and the last 20 commits, each with hash, short hash, author, ISO 8601
date, and subject. `api/build.rs` captures all of it at compile time. A build with no git history reports an empty
history rather than failing.

### `/api/v1/llms`

An `Llm` has `id`, `name`, `description`, `type`, `priority`, `isActive`, `expiresAt`, `createdAt`, and
`updatedAt`. The secret token is never returned.

`type` is one of `chatgpt-oauth`, `chatgpt-api-token`, `claude-api-token`, or `claude-code-oauth`.

`POST` requires `name`, `type`, and `secretToken`. It accepts optional `description` (default empty), `priority`
(default 0, lower is tried first), `isActive` (default true), and `expiresAt`.

`PATCH` accepts any subset of the same fields. Omitted fields are unchanged. `expiresAt: null` clears the expiry.
A new `secretToken` is re-sealed. An empty body is rejected.

Timestamps sent to the API must include an offset. Responses are always UTC with a `Z` suffix.

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
`POST` requires `name` (1 to 120 characters, unique) and accepts `description` (up to 2000, default empty). `PATCH`
accepts any of `name`, `description`, `coverFit`, and `github`; an empty body is rejected. A `github` whose
`credentialId` is missing for `specific`, present for another access, or names no token answers `400`. `DELETE` answers `409` while any coding session belongs to the project.

`PUT /{id}/cover` takes the image file as the raw body: PNG, JPEG, WebP, or GIF (first frame), up to 10,000,000
bytes; this route alone raises the API's 1 MiB body limit, and nginx's, to allow it. The format is read from the bytes,
not the content type. Decoding refuses images over 8192 pixels on a side or 256 MiB of pixels. The image is scaled down
to fit 2400 by 1350 when larger, keeping its shape, and stored as lossy WebP at quality 80, keeping transparency and
dropping metadata (`api/src/images.rs`). Anything refused answers `400`; a larger body answers `413`.
`GET /{id}/cover` serves it with `Cache-Control: private, max-age=31536000, immutable`: clients add `coverUpdatedAt` to
the URL, so each cover version has its own. Cover changes publish `project.upserted`.

### `/api/v1/satellites`

A `Satellite` has `id`, `name`, `description`, `url`, `isActive`, `createdAt`, `updatedAt`, and `status`. The
secret is never returned. `status` is the fleet's latest poll, or null for an inactive satellite and until the
first poll after a change: `{ satelliteId, reachable, version, runningThreads, maxConcurrentThreads, error }`.

`POST` requires `name`, `url` (http or https), and `secret`, and accepts `description` and `isActive` (default
true). `PATCH` accepts any subset; a new `secret` is re-sealed. Saving restarts the satellite's watchers.

`test` answers `{ version, protoMajor, protoMinor, runningThreads, maxConcurrentThreads }`, or `502` with the
satellite's error.

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

### `/api/v1/action-items`

An `ActionItem` has `id`, `title`, `notes`, `state` (`inbox`, `open`, `resolved`, `dismissed`), `priority` (`urgent`,
`high`, `normal`, `low`), `dueAt`, `snoozedUntil`, `waitingOn`, `owner`, `resolvedAt`, `dismissedAt`, `deletedAt`,
`projectIds`, `initiativeIds` (the initiatives it is in now, leaving out deleted ones), `createdAt`, and `updatedAt`.
`owner` is `{ kind: "user" }`, `{ kind: "other", name }`, or `{ kind: "nobody" }`. See `docs/action-items.md`.

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

Membership routes are idempotent: adding an item to a project or initiative it is in, or removing it from one it is
not in, answers the item unchanged. Adding to an unknown project or initiative answers `404`, and joining a deleted
initiative `409`. Deleting a project takes its items and initiatives out of it, each recording the removal.

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

A `Changeset` has `id`, `proposer` (`elysia` or `session:<number>`), `projectId` (the project a coding session proposed
it in, or null), `summary`, `state` (`pending`, `applied`, `rejected`, `undone`), `decidedAt`, `undoneAt`,
`operations`, `createdAt`, and `updatedAt`. An operation has `id`, `position` (from 1), `operation` (an object whose
`kind` says what it does, shaped as `docs/action-items.md` lists), `reason`, `quote`, `source`, `dependsOn` (the
positions of the earlier operations it acts on), `decision` (`pending`, `approved`, `rejected`), `outcome` (`pending`,
`applied`, `failed`, `skipped`, `undone`), `error` (why it failed or was skipped), `result` (the ids it acted on or
created, and the values it replaced), and `undo` (null until the changeset is undone; then any of `kept`,
`movedSince`, `stillPosted`, `stillClosed`, and `refusal`).

Nothing creates a changeset over HTTP; proposers stage them in-process (`work_propose_changes` today). `GET /` takes
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

`progress` answers `{ resolved, total, burnup }`, with `burnup` a list of `{ at, resolved, total }`: a point at the
initiative's creation, one at every moment either count changed, and one now.

### `/api/v1/coding-sessions`

A `CodingSession` has `id` (the session's number, 1, 2, 3, ...), `projectId`, `satelliteId`, `threadId`, `title`,
`actionItemId` (the item it was started from, or null), `createdAt`, `updatedAt`, and
`thread`: the thread as of the latest poll, or null until the first poll sees it. `thread` holds `state`,
`queueDepth`, `currentTurnId`, `latestSequence`, `lastActivityAt`, and `expiresAt`. `state` is one of `unknown`,
`provisioning`, `idle`, `running`, `awaiting-input`, `watching`, `paused`, `expired`, or `destroyed`.

`POST` requires `projectId`, `satelliteId`, and `title` (1 to 200 characters), and accepts `repositories`, up to 16
`{ url, baseBranch? }` cloned in order, and `githubCredentialId` (absent follows the project, `null` asks for no token, and an id names one;
an unknown id answers `400`). Two repositories that would clone into the same directory, ignoring case, answer `400`
naming both URLs; see `docs/coding.md`. Sessions carry `githubCredentialId`, the token their thread started with.
`prompt` (1 to 100,000 characters) is queued as the thread's first turn. `actionItemId` starts the session from that
item: `prompt` is then required (`400` without it), the project must be one of the item's projects when it has any
(`400` otherwise), an unknown item answers `404`, and a deleted one `409`. The first turn carries the item's context
ahead of the prompt; see `docs/action-items.md`. The satellite must be active (`409` otherwise). `turns` requires `prompt` (1 to 100,000 characters)
and answers `{ turnId, status, prompt, queuedAt }`; the turn's progress arrives on the event stream.

A `SessionEvent` has `sessionId` (the session's number), `sequence`, `turnId`, `occurredAt`, `type` (the satellite's wire name, such as
`agent.message`), `memberId`, and `payload`. `payload` is null for event types the API does not render; otherwise
its `kind` is one of `agentMessage`, `agentThinking`, `toolStarted`, `toolCompleted`, `turnStarted`,
`turnCompleted`, `planProposed`, `questionAsked`, `budgetWarning`, or `incident`. Field shapes are in
`api/src/fleet/views.rs`. Behavior, policy, and history limits are in `docs/coding.md`.

### Errors

Every error is JSON with a `message`.

| Status | Cause                                                                                 |
|--------|---------------------------------------------------------------------------------------|
| `400`  | Malformed JSON or query, unknown field, unknown enum value, malformed id in the path, blank or oversized token, refused environment variable key, a call an upstream reads as malformed such as invalid JQL |
| `403`  | The credential the request was made with may not touch what it named; see `docs/jira.md` |
| `404`  | No row with that id                                                                   |
| `409`  | Unique constraint, such as a duplicate LLM name, a project that still has sessions, or a change the record's state refuses |
| `422`  | Field validation failed; `fields` lists each failure                                  |
| `502`  | A satellite, mail server, storage provider, or the OAuth broker refused; `message` is its own error |
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
pending, connects to Redis, starts the fleet watchers (see `docs/coding.md`) and the link watcher (see
`docs/action-items.md`), binds, and serves. Both store connections retry with exponential backoff, 8 attempts over
roughly a minute, and prove themselves with `SELECT 1` and `PING`.

On `SIGTERM` or `SIGINT`, a shutdown token is cancelled first. That ends every open event stream, fleet watcher, and
the link watcher, which would otherwise keep the drain waiting forever. The listener stops accepting, in-flight
requests drain, the watchers are awaited, then the Postgres pool closes and the Redis connection drops. Compose allows
30 seconds for this before `SIGKILL`.

## Configuration

| Variable                   | Default          | Purpose                                    |
|----------------------------|------------------|--------------------------------------------|
| `DATABASE_URL`             | required         | Postgres connection string                 |
| `REDIS_URL`                | required         | Redis connection string                    |
| `ELYSIUM_ENCRYPTION_KEY`   | required         | Base64 32-byte key sealing stored secrets  |
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
of 30. Both are constants in `src/middleware/rate_limit.rs`.

## Code layout

| Path                     | Holds                                            |
|--------------------------|--------------------------------------------------|
| `src/server.rs`          | Startup, middleware stack, shutdown              |
| `src/routes/`            | One file per route                               |
| `src/models/`            | Diesel records and the queries for each table    |
| `src/database/`          | Pool type, migration runner, generated schema    |
| `src/crypto.rs`          | Secret sealing                                   |
| `src/errors.rs`          | `ApiError` and its mapping to HTTP responses     |
| `src/state.rs`           | `AppState`: pool, Redis, cipher, version, bus, fleet, GitHub, Jira, links, mail, storage, shutdown token |
| `src/realtime.rs`        | Event bus and the `ServerEvent` envelope          |
| `src/fleet/`             | Satellite clients and watchers; JSON views of Arsox types |
| `src/jira/`              | Jira Cloud's REST API and Atlassian Document Format |
| `src/mail/`              | IMAP and SMTP transport, OAuth broker client, mail server hosting and administration, DNS checks |
| `src/storage/`           | Storage provider clients, reached through `Storage` |
| `src/environment/`       | The rules for which environment variable keys are refused |
| `src/action_items/`      | Action item rules: actors, state transitions, Next's order, initiative progress, a session's first turn from an item, link providers (`links/`), the link watcher, and changesets' operations, dependencies, and decisions |
| `src/tools/`             | The relayed MCP servers agents call: `elysium_storage` and `elysium_work`, see `docs/coding.md` |

## Logging

Logs are structured `tracing` events with dotted names such as `connection.open.success` and
`migration.change.success`, using OpenTelemetry attribute names. Every request gets an `x-request-id`, either
client-supplied or a generated UUID. The id is echoed on the response and attached to the request span.

## Testing

`cargo test` runs the hermetic unit tests: encryption, request parsing and validation, refused environment variable
keys, event envelopes, the Arsox view conversions, the action item rules (transitions, Next's order, progress, a
session's first turn from an item, and what a provider's change does to a linked item), GitHub's client against a
local fake, and the agent tools' schemas.
`api/scripts/verify-migrations.sh` also runs the database-backed tests against a disposable Postgres.

## Roadmap

- Authentication. The LLM and satellite routes write credentials and drive agents, so nginx publishes on loopback
  by default until auth exists.
- TLS to Postgres for deployments where the database is not on a private network.
