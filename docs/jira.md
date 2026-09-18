# Jira

Elysium reaches Jira Cloud with API tokens, managed on the Jira settings page (`/settings/jira`). Any number of
credentials are held at once, each named, so one Elysium can reach a work site and a personal one. A credential
names the projects and boards it may touch, and every call is bounded by that list.

Jira Cloud only. Authentication is HTTP basic with the account's email address and an API token, against the
credential's own site, `https://<site>.atlassian.net`. Jira Data Center, which lives on a customer's own host, is
not supported.

## Credentials

A credential is a name, a site, an account email, the token itself, what Jira answered the last time the token was
checked, and the projects and boards the token may reach.

| Field          | Meaning                                                                                  |
|----------------|------------------------------------------------------------------------------------------|
| `siteUrl`      | The site's origin, `https://<site>.atlassian.net`; validated and normalized               |
| `accountEmail` | The Atlassian account the token belongs to, the username half of basic auth               |
| token          | Sealed in `jira_credentials.token_encrypted`; write-only over HTTP                        |
| `accountId`    | The account Jira reports for the token, its stable id                                     |
| `displayName`  | That account's name, as Jira reports it                                                   |
| `projects`     | `"*"` for every project, including ones added later, or the list the credential may touch |
| `boards`       | `"*"` for every board, or the list the credential may touch                               |
| `checkedAt`    | When Jira last confirmed the token, which is every save and every test                    |

A site URL is refused unless it is `https`, its host ends in `.atlassian.net` and is longer than that suffix, it
carries no userinfo, port, query, or fragment, and no path beyond `/`. What is stored is the origin alone, so two
spellings of one site are one site.

Selections are stored with their display names (`jira_credential_projects`, `jira_credential_boards`), so the
settings page renders a credential without calling Jira. `"*"` is stored as `all_projects` or `all_boards` and
removes the link rows, the same shape storage locations use for their projects.

## The allowlist

The allowlist is the whole point of a credential: a token that can reach thirty projects is given to Elysium for
two of them.

- Every read and every write passes through one check (`api/src/routes/v1/jira_credentials/allowlist.rs`).
- Where the issue key implies a project, the project is checked **before** the call: `ELY-12` is in project `ELY`.
  A key is split at its last hyphen, and keys are compared case insensitively.
- The project Jira reports on the answer is checked **after** the call, so an issue moved to another project since
  it was written cannot slip through a stale key.
- A search is **bounded**, never filtered afterwards: the caller's JQL is rewritten as
  `(<their where clause>) AND project IN ("ABC", "DEF") ORDER BY <their ordering>`, so Jira itself never looks
  outside the allowed projects. An `ORDER BY` in the caller's JQL is hoisted out before the `AND` is added, since
  `(... ORDER BY x) AND ...` is not valid JQL. Quotes in the caller's JQL are tracked, so an `order by` inside a
  quoted string is left alone. With no ordering of their own, a search is ordered `updated DESC`.
- A credential for every project (`"*"`) adds no `project IN` clause, and an empty JQL means every allowed project
  ordered by `updated DESC`.
- Anything outside the list answers `403` naming the project and the credential. Nothing is silently dropped.

Boards are allowlisted the same way and are read only today: they are listed for picking, and the roadmap below
builds on them.

## No freeform entry

Nothing anywhere takes a typed project key or board id. The user picks from what Jira reports their token can
reach, which is why creating a credential is two steps:

1. `POST /api/v1/jira-credentials/discover` with `{ siteUrl, accountEmail, token }` checks the token against
   `GET /rest/api/3/myself` and answers the account, the projects, and the boards that token can see. **Nothing is
   stored**, and the credential does not exist yet.
2. `POST /api/v1/jira-credentials` creates it with the selections the user made. The token is checked with Jira
   again, the selections are checked against what Jira reports now, and only then is anything written. A selection
   Jira does not report answers `400` naming the offender.

`GET /{id}/projects` and `GET /{id}/boards` do the same for the edit form, with the stored token, marking which
entries are currently selected.

## Routes

`/api/v1/jira-credentials`. Every shape below is exactly what the API sends and accepts; fields are camelCase and
every timestamp is UTC with a `Z` suffix.

| Method | Path                             | Answers                                                   |
|--------|----------------------------------|-----------------------------------------------------------|
| GET    | `/`                              | `{ credentials: JiraCredential[] }`, by name              |
| POST   | `/`                              | `201 { credential }`; checked with Jira first             |
| POST   | `/discover`                      | `{ account, projects, boards, projectsTruncated, boardsTruncated }`; stores nothing |
| GET    | `/{id}`                          | `{ credential }`                                          |
| PATCH  | `/{id}`                          | `{ credential }`                                          |
| DELETE | `/{id}`                          | `204`; the token itself stays valid on Jira               |
| POST   | `/{id}/test`                     | `{ credential, result }` from asking Jira now             |
| GET    | `/{id}/projects`                 | `{ projects, truncated }` the stored token can reach now  |
| GET    | `/{id}/boards`                   | `{ boards, truncated }` the stored token can reach now    |
| GET    | `/{id}/issues`                   | `{ issues, nextPageToken, isLast }`                       |
| POST   | `/{id}/issues`                   | `201 { issue }`                                           |
| GET    | `/{id}/issues/{key}`             | `{ issue }`, with its description and comments            |
| PATCH  | `/{id}/issues/{key}`             | `{ issue }`                                               |
| GET    | `/{id}/issues/{key}/transitions` | `{ transitions }`                                         |
| POST   | `/{id}/issues/{key}/transitions` | `{ issue }` as it is after the transition                 |
| POST   | `/{id}/issues/{key}/comments`    | `201 { comment }`                                         |

### `JiraCredential`

```json
{
  "id": "0199…",
  "name": "Work",
  "siteUrl": "https://acme.atlassian.net",
  "accountEmail": "alex@example.com",
  "accountId": "5b10a2844c20165700ede21g",
  "displayName": "Alex Navarro",
  "projects": [{ "id": "10001", "key": "ELY", "name": "Elysium" }],
  "boards": [{ "id": 12, "name": "ELY board", "projectKey": "ELY" }],
  "checkedAt": "2026-09-17T12:00:00Z",
  "createdAt": "2026-09-17T12:00:00Z",
  "updatedAt": "2026-09-17T12:00:00Z"
}
```

`projects` is either the string `"*"` or an array of `{ id, key, name }`; `boards` is either `"*"` or an array of
`{ id, name, projectKey }`, where `projectKey` is null for a board spanning no single project. The token is never
returned, sealed or not.

### Creating and changing

`POST /` takes `{ name, siteUrl, accountEmail, token, projects, boards }`.

| Field          | Rule                                                                                       |
|----------------|--------------------------------------------------------------------------------------------|
| `name`         | 1 to 120 characters, unique; a duplicate answers `409`                                     |
| `siteUrl`      | As above; anything else answers `400` saying what a site URL looks like                    |
| `accountEmail` | An address with one `@`, up to 320 characters                                              |
| `token`        | 1 to 512 characters, surrounding whitespace dropped; never returned                        |
| `projects`     | `"*"`, or an array of strings, each a project's `id` or its `key` as Jira reports them     |
| `boards`       | `"*"`, or an array of board ids as numbers                                                 |

A selection Jira does not report for that token answers `400` naming the offending id or key. Selections are stored
with the id, key, and name Jira reports, never with what the client sent.

`PATCH /{id}` accepts any subset of `name`, `siteUrl`, `accountEmail`, `token`, `projects`, and `boards`. An empty
body is refused.

- A different `siteUrl` or `accountEmail` needs `token` in the same request: a token belongs to one account on one
  site. Renaming needs neither.
- A different `siteUrl` also needs `projects` and `boards`, since selections made on the old site mean nothing on
  the new one.
- Any request carrying `projects` or `boards` is re-checked against Jira before it is stored, exactly as a create
  is. A `token` on its own is checked the same way, and the row records the check.
- A body that names only values the credential already holds changes nothing and answers `400`, rather than
  writing an empty update.

A rename calls Jira not at all.

### `POST /discover`

```json
{
  "account": { "accountId": "5b10…", "displayName": "Alex Navarro", "email": "alex@example.com" },
  "projects": [{ "id": "10001", "key": "ELY", "name": "Elysium" }],
  "boards": [{ "id": 12, "name": "ELY board", "projectKey": "ELY" }],
  "projectsTruncated": false,
  "boardsTruncated": false
}
```

`account.email` is null when the account hides its address, which Atlassian allows; the address the client sent is
still what signs in. A `truncated` flag is true when Jira had more pages than Elysium reads (see
[Page caps](#page-caps)).

### `POST /{id}/test`

Asks Jira about the stored token now, records the account and `checkedAt` on the credential, publishes the refreshed
credential on the event stream, and reports which stored selections the token can still reach.

```json
{
  "credential": { "…": "…" },
  "result": {
    "account": { "accountId": "5b10…", "displayName": "Alex Navarro", "email": "alex@example.com" },
    "projects": [{ "id": "10001", "key": "ELY", "name": "Elysium", "reachable": true }],
    "boards": [{ "id": 12, "name": "ELY board", "projectKey": "ELY", "reachable": false }]
  }
}
```

A selection the token has lost access to comes back `reachable: false` rather than being dropped, so the page can
say so. For a credential with `"*"` there is nothing stored to check and the lists are empty.

### `GET /{id}/projects` and `GET /{id}/boards`

```json
{ "projects": [{ "id": "10001", "key": "ELY", "name": "Elysium", "selected": true }], "truncated": false }
{ "boards": [{ "id": 12, "name": "ELY board", "projectKey": "ELY", "selected": false }], "truncated": false }
```

`selected` is true for every entry when the credential holds `"*"`.

### Issues

`GET /{id}/issues` takes `jql`, `maxResults`, and `nextPageToken`, all optional.

**Paging is by token, not by offset.** Jira Cloud's current search endpoint is `POST /rest/api/3/search/jql`, which
pages with an opaque `nextPageToken` and reports `isLast`; the older offset endpoint with `startAt` and a `total`
is deprecated, and neither an offset nor a total count is available. Send no token for the first page, then send
back the `nextPageToken` from the previous answer. `nextPageToken` is null on the last page.

`maxResults` is 1 to 100, defaulting to 50. The ceiling is Elysium's own; Jira may return fewer. Any other query
parameter answers `400` rather than being ignored, so a `startAt` left over from the old endpoint is reported
instead of quietly returning page one forever.

A credential whose project list is empty can match nothing, so it answers an empty page with `isLast: true`
without calling Jira at all.

```json
{
  "issues": [{ "…": "…" }],
  "nextPageToken": "CAEaAggD",
  "isLast": false
}
```

An `Issue` is the same shape everywhere it appears:

```json
{
  "id": "10042",
  "key": "ELY-12",
  "projectKey": "ELY",
  "summary": "Bound every search to the allowlist",
  "status": { "name": "In Progress", "category": "indeterminate" },
  "issueType": "Task",
  "priority": "High",
  "assignee": { "accountId": "5b10…", "displayName": "Alex Navarro" },
  "reporter": { "accountId": "5b10…", "displayName": "Alex Navarro" },
  "labels": ["backend"],
  "createdAt": "2026-09-17T12:00:00Z",
  "updatedAt": "2026-09-17T12:30:00Z",
  "url": "https://acme.atlassian.net/browse/ELY-12",
  "description": null,
  "comments": []
}
```

Every field but `id`, `key`, `projectKey`, `summary`, `labels`, and `url` can be null: Jira reports a status, a
type, a priority, or a person only when the project has one, and Elysium passes that through rather than inventing
a value. `description` and `comments` are filled in only by `GET /{id}/issues/{key}`; a search leaves them null and
empty, since a listing has no use for them and they are the largest part of an issue.

`projectKey` is what Jira reports the issue is in now, not the prefix of the key that was asked for, which is what
makes the check after a call meaningful.

`POST /{id}/issues` takes `{ projectKey, issueType, summary, description?, labels?, priority?, assigneeAccountId?,
parentKey? }` and answers the created issue, read back from Jira. `PATCH /{id}/issues/{key}` takes any subset of
`{ summary, description, labels, priority, assigneeAccountId }` and answers the issue as it is afterwards;
`assigneeAccountId: null` unassigns it, and an empty body is refused.

`GET /{id}/issues/{key}/transitions` answers `{ transitions: [{ id, name, to: { name, category } }] }`, the moves
this issue can make right now, for whoever the token is. `POST` to the same path takes `{ transitionId, comment? }`
and answers the issue after the move.

`POST /{id}/issues/{key}/comments` takes `{ body }` and answers
`201 { comment: { id, author, body, createdAt, updatedAt } }`.

### Text is Atlassian Document Format

Descriptions and comments on Jira Cloud are ADF, a JSON document tree, not a string. Elysium accepts plain text
from its callers and wraps it: every blank-line-separated block becomes a paragraph, and single newlines inside a
block become hard breaks. That covers what a form and an agent write; it is not a Markdown renderer, so `**bold**`
arrives as those characters.

Reading goes the other way. Any rich text Elysium returns is both halves:

```json
{ "text": "One paragraph.\n\nAnd another.", "adf": { "type": "doc", "version": 1, "content": [] } }
```

`adf` is exactly what Jira stored, for a client that renders it properly. `text` is a plain rendering that walks
the tree and keeps paragraphs, headings, list items, and code blocks as lines, so a list or a table is readable
even though its structure is flattened. A document Elysium cannot read at all renders as an empty string, never an
error.

### Errors

| Status | Cause                                                                                            |
|--------|--------------------------------------------------------------------------------------------------|
| `400`  | A malformed request, a site URL that is not a Jira Cloud site, a selection Jira does not report, a token Jira refuses, or a call Jira refused as invalid (bad JQL, an unknown field, a transition that does not apply), carrying Jira's own messages |
| `403`  | A project outside the credential's allowlist, naming the project and the credential                |
| `404`  | No credential with that id, or an issue the token cannot see; Jira answers `404` for both a missing issue and one the token may not read |
| `409`  | A credential name that is already taken                                                           |
| `422`  | Field validation failed; `fields` lists each failure                                               |
| `502`  | Jira could not be reached, or refused for a reason that is not the caller's to fix                 |

A token Jira rejects is the client's mistake, not an upstream fault, so it answers `400` with what to check, the
way a GitHub token does.

## The client

`api/src/jira/mod.rs` is the only place that calls Jira. It holds Elysium's shared HTTP client, whose `User-Agent`
is already set, and builds basic auth from the credential's email and token for every call. The site comes from the
credential, so the host is not fixed in code the way GitHub's is; it is validated and normalized instead, and every
request is built from the stored origin.

| Call                                              | Used for                                  |
|---------------------------------------------------|-------------------------------------------|
| `GET /rest/api/3/myself`                          | Proving a token works and whose it is     |
| `GET /rest/api/3/project/search`                  | The projects a token can reach, paged     |
| `GET /rest/agile/1.0/board`                       | The boards a token can reach, paged       |
| `POST /rest/api/3/search/jql`                     | A bounded JQL search                      |
| `GET /rest/api/3/issue/{key}`                     | One issue, with description and comments  |
| `POST /rest/api/3/issue`                          | Creating an issue                         |
| `PUT /rest/api/3/issue/{key}`                     | Changing an issue's fields                |
| `GET /rest/api/3/issue/{key}/transitions`         | The transitions that apply now            |
| `POST /rest/api/3/issue/{key}/transitions`        | Applying one                              |
| `POST /rest/api/3/issue/{key}/comment`            | Adding a comment                          |

Searches always name the fields they want, so the answer never depends on what Jira returns by default, and they
always ask for `project` so the allowlist can be checked against the answer.

### Page caps

Listings follow Jira's pages up to a cap, since a listing answers one HTTP request under the API's 30 second
handler deadline:

| Listing  | Page size | Pages | Ceiling |
|----------|-----------|-------|---------|
| Projects | 50        | 10    | 500     |
| Boards   | 50        | 10    | 500     |

Fifty is the most Jira sends per page for both. Past the cap the answer carries `truncated: true`, which the
picker shows; there is no way to widen a selection past it today, which is the roadmap item below.

## Realtime

| Event                    | `data`           | Sent when                                        |
|--------------------------|------------------|--------------------------------------------------|
| `jiraCredential.upserted` | `JiraCredential` | A credential was added, changed, or checked      |
| `jiraCredential.deleted`  | `{ id }`         | A credential was deleted                         |

Issues are not pushed. Jira has no stream Elysium listens to, so an issue view refetches.

## Roadmap

- Action items: work Elysium tracks itself, created from and synced to Jira issues, built on these credentials and
  bounded by the same allowlist.
- Board contents: sprints and their issues, which the stored boards already name.
- A remembered reachability per selection, so the credentials list can mark a project the token has lost access to
  without anyone pressing Test. A `JiraCredential` is built from stored rows and costs no Jira call today, and the
  two ways to change that are both worse than the gap: calling Jira once per credential would make the settings
  page as slow as the slowest site, and storing what the last check found would show a boolean that is only as
  current as the last time someone looked. Today `POST /{id}/test` reports it live and publishes the refreshed
  credential on the event stream, and the edit form's listings mark what is selected.
- A searchable picker for sites with more projects or boards than one page cap holds.
- Webhooks, so an issue changed in Jira reaches the event stream instead of waiting for a refetch.
