# GitHub

Elysium reaches GitHub with personal access tokens, added and listed in the connector hub (Settings, Connectors, `docs/connectors.md`) and edited on their own page
(`/settings/github/:credentialId/edit`). Any
number of tokens are held at once, each named, so one Elysium can act as a work account and a personal one. A coding
session's agent works with one of them through `gh` and `git`.

## Tokens

A credential is a name, a kind, the token itself, and what GitHub answered the last time the token was checked.

| Field            | Meaning                                                                             |
|------------------|--------------------------------------------------------------------------------------|
| `kind`           | `classic` or `fine-grained`                                                          |
| token            | Sealed in `github_credentials.token_encrypted`; write-only over HTTP                 |
| `login`          | The account the token acts as, as GitHub reports it                                  |
| `scopes`         | A classic token's scopes; empty for a fine-grained token                             |
| `tokenExpiresAt` | When GitHub stops accepting the token; `null` for one that does not expire           |
| `checkedAt`      | When GitHub last confirmed the token, which is every save and every test             |

Both kinds are sent as a bearer token, and GitHub tells them apart only in what it answers: a classic token's scopes
come back in `X-OAuth-Scopes`, while a fine-grained token's permissions are granted per repository and are not
reported at all.

A token is written a particular way, and the API checks that before it spends a call:

| Kind           | Shape                                                                       |
|----------------|------------------------------------------------------------------------------|
| `classic`      | `ghp_` followed by letters and digits, or the 40 characters issued before 2021 |
| `fine-grained` | `github_pat_` followed by letters, digits, and underscores                   |

Lengths are GitHub's to change, so only the prefix and the alphabet are checked. The form checks the same shapes
(`matchesGithubTokenKind`) before it sends anything.

## Every write is checked with GitHub

`api/src/github/` is the only place that calls GitHub. It holds Elysium's shared HTTP client and sends every call
with the token, `Accept: application/vnd.github+json`, and `X-GitHub-Api-Version: 2022-11-28`. `verify` is
`GET https://api.github.com/user`. GitHub refuses a request without a `User-Agent`, and the shared client sets
Elysium's.

Creating or replacing a token calls it first, and stores the token only if GitHub accepts it. That is why every row
names an account and an expiry rather than holding a token nobody has tried. A token GitHub rejects answers `400`
with what to check; a GitHub that cannot be reached answers `502`.

The host is fixed in code, so a credential carries no endpoint. GitHub Enterprise Server, which lives on a customer's
own host, is not supported.

### Changing a token

A stored token is one kind of token, so changing the kind means bringing the token that goes with it: `PATCH` with a
different `kind` and no `token` answers `400`. Renaming needs neither. The form mirrors this: the token field turns
required the moment the kind changes, and is otherwise optional, keeping what is stored.

## Testing a token

`POST /api/v1/github-credentials/{id}/test` asks GitHub about the stored token right now and answers
`{ result: { login, scopes, tokenExpiresAt } }`. What comes back replaces what the row remembered, since a token can
be revoked, rotated, or given new scopes on GitHub, and the refreshed credential goes out on the event stream. A
token GitHub refuses answers `400`, and an unreachable GitHub `502`.

Deleting a credential only makes Elysium forget the token. It keeps working until it is revoked on GitHub.

## The configuration repository's token

The configuration repository agents' `~/.claude` comes from (`docs/coding.md`, `~/.claude`) is read with a token of
its own, chosen beside it under Settings, LLMs, Agent configuration
(`workspace_settings.agent_configuration_github_credential_id`), never the workspace's default: a company's default
token is usually a fine-grained token of a work account, which can never reach a repository another account owns, even
as a collaborator. Its owner adds a fine-grained token scoped to that repository with read-only Contents, under
Connectors like any other, and chooses it there. The token is sent to github.com alone, so the settings refuse one for
a repository on another host. Replacing a token's value or deleting a token signals the engine to fetch the
repository again, so the settings page shows at once whether it still reads.

## Which token a session gets

A session starts with one token, or none, chosen at three levels:

| Level     | Choices                                                     | Stored as                                         |
|-----------|-------------------------------------------------------------|---------------------------------------------------|
| Workspace | One default token, or none                                  | `github_credentials.is_default`, at most one true |
| Project   | Follow the workspace default, no token, or a specific token | `projects.github_access`, `github_credential_id`  |
| Session   | Follow the project, no token, or a specific token           | `coding_sessions.github_credential_id`            |

Following the level above and choosing no token are different choices: a project that must never reach GitHub stays
that way when a workspace default is added later. Deleting a token a project chose leaves that project with
`specific` access and no token; it follows the workspace default from then on, and the project page says so. Deleting
the default leaves the workspace without one. A session records the token it was started with and opens it again
for every turn, so a token edited under Settings reaches the next turn; a deleted token leaves that record empty and
the session's later turns without one.

`POST /api/v1/coding-sessions` takes `githubCredentialId`: absent follows the project, `null` asks for no token, and
an id names one. `api/src/routes/v1/coding_sessions/github_token.rs` resolves the choice and opens the token. A token
that cannot be decrypted refuses the session rather than starting the agent without the access it was promised.

A worker of the pool has no session level: it takes its batch's project's choice as it starts, the project's token or
the workspace default or none, and gets it the same way, clones and every turn alike (`docs/engine.md`, Workers).

## How the agent gets the token

Only one token per session, set in each turn's environment and in the environment of the session's clones:

- `GH_TOKEN` holds the token, and Elysium scrubs it from the session's events. `gh` reads it on its own.
- `git` does not read `GH_TOKEN`, so the turn also carries git config through the environment, which git 2.31 and
  later reads (the workspace image ships Ubuntu 24.04's git 2.43):

  | Variable             | Value                                  |
  |----------------------|----------------------------------------|
  | `GIT_CONFIG_COUNT`   | `2`                                    |
  | `GIT_CONFIG_KEY_0`   | `credential.https://github.com.helper` |
  | `GIT_CONFIG_VALUE_0` | empty, clearing any helper set elsewhere |
  | `GIT_CONFIG_KEY_1`   | `credential.https://github.com.helper` |
  | `GIT_CONFIG_VALUE_1` | `!gh auth git-credential`              |

  Nothing is written into the workspace.
- The session's clones run with the same environment, so a private github.com repository clones with the token.
  Elysium holds no SSH keys, so a github.com SSH remote (`git@github.com:` or `ssh://git@github.com/`) is cloned from
  its `https://github.com/` URL instead (`github_token::clone_url`), and the workspace's `origin` is HTTPS, the remote
  the `gh` credential helper serves.

An agent can read every variable in its environment, so it holds the token. Scope each token to what its sessions
need.

## Listing a token's repositories

`GET /api/v1/github-credentials/{id}/repositories` lists what the stored token can see, for picking a session's
repositories instead of pasting their URLs. It asks `GET https://api.github.com/user/repos?per_page=100&sort=pushed`
and follows the `Link` header's `rel="next"`, only ever on `https://api.github.com/`, for up to 10 pages
(`REPOSITORY_PAGE_LIMIT`), so 1,000 repositories. It answers `{ repositories, truncated }`, most recently pushed first:

| Field           | Meaning                                                                               |
|-----------------|---------------------------------------------------------------------------------------|
| `fullName`      | `owner/name`, as GitHub spells it; also `owner` and `name` on their own                |
| `private`       | Whether the repository is private                                                     |
| `archived`      | Whether it is archived; the picker still lists it, marked                              |
| `defaultBranch` | The branch a session starts from when it names none                                    |
| `cloneUrl`      | The `https://github.com/` URL a session clones                                         |
| `pushedAt`      | When anything was last pushed; `null` for a repository nothing was pushed to          |
| `canPush`       | As for `repository-access`: the account's role and a classic token's scopes; `null` for a fine-grained token |

`truncated` is true when GitHub had more pages; the rest can still be added by URL. A token GitHub refuses answers
`400`, an unreachable GitHub `502`, and an unknown credential `404`. Nothing is cached: one page answers in about a
second (0.7 to 2.5 seconds measured), and the list is asked for once when the New session form opens on a token.

With no `type`, GitHub already includes the account's own repositories, the ones it collaborates on, and its
organizations' repositories. An `affiliation` naming all three changed nothing when measured with a classic token.

### What a fine-grained token lists

Measured with a fine-grained token whose resource owner is the JalapenoLabs organization, granted all of its
repositories:

- Every repository of the resource owner was listed, private ones included (45 of 45).
- The account's own public repositories were listed too (44), but none of its private ones, which the token cannot
  read either (`repository-access` answers `canRead: false`).
- Other organizations' repositories were not listed, public or private. An organization can refuse the token outright,
  such as one that forbids fine-grained tokens living longer than 366 days.

So the list holds what the token can clone, except other owners' public repositories, which clone without a token and
are added by URL. `canPush` is `null` for every entry, since GitHub reports only the account's role.

## Checking a token against a repository

`POST /api/v1/github-credentials/{id}/repository-access` with `{ repositoryUrl }` asks
`GET https://api.github.com/repos/{owner}/{repo}` with the token, and answers
`{ result: { repository, canRead, canPush, isPrivate } }`:

| GitHub answers | Result                                                                                        |
|----------------|-----------------------------------------------------------------------------------------------|
| 200            | `canRead: true`. For a classic token, `canPush` is the account's push permission and `repo` (or `public_repo` on a public repository); for a fine-grained token it is `null`, since GitHub does not report a token's own permissions |
| 404            | `canRead: false`. GitHub answers 404, not 403, for a private repository a token cannot see    |
| 401            | `400` with what to check                                                                      |

Only `https://github.com/`, `ssh://git@github.com/`, and `git@github.com:` remotes are checked; owner and name are
checked against GitHub's alphabets before they become part of the path (`Repository::from_url`). The New session
modal runs the check for each github.com repository added by URL that the token's list does not hold, and says what
the chosen token can do. It also warns about a token that has expired or expires within a week. A warning never blocks
the session.

## Issues, pull requests, milestones, and labels

Action items link to GitHub issues and pull requests, and initiatives to a repository's milestones and labels
(`docs/action-items.md`). The client reads and writes them in `api/src/github/issues.rs`:

| Call                                                | Used for                                              |
|-----------------------------------------------------|-------------------------------------------------------|
| `GET /repos/{owner}/{repo}/issues/{number}`         | One issue or pull request: its state, reason, assignee, and whether it merged |
| `GET /repos/{owner}/{repo}/issues`                  | A repository's issues, by `state`, `since`, `milestone`, `labels`, `creator`, or `assignee`, paged |
| `GET /issues?filter=assigned&state=open`            | The open issues assigned to the token's account, in every repository it sees, most recently updated first |
| `GET /repos/{owner}/{repo}/pulls/{number}`          | Whether a closed pull request merged                  |
| `PATCH /repos/{owner}/{repo}/issues/{number}`       | Closing an issue as completed                         |
| `POST /repos/{owner}/{repo}/issues/{number}/comments` | Posting an item's comment                          |
| `GET /repos/{owner}/{repo}/milestones`, `.../milestones/{number}` | Picking and reading a milestone          |
| `GET /repos/{owner}/{repo}/labels`, `.../labels/{name}` | Picking and reading a label                       |
| `POST /repos/{owner}/{repo}/issues`                 | Opening a problem report's issue on `JalapenoLabs/Elysium`, and an issue a Project Manager files for an item |
| `GET /search/issues`                                | Finding the open issue that already carries a report's fingerprint |

A pull request is an issue that carries a `pull_request` object, so one listing reads a repository's changes of both
kinds. GitHub does not promise `merged_at` in that object, so a closed pull request that does not say it merged is read
from the pull request itself before it counts as closed without merging. An issue closed with `state_reason: not_planned` dismisses its item; any other close resolves it. Path segments
are percent-encoded, so a label with a space or a slash stays one segment, and owner and name are checked against
GitHub's alphabets before they reach a path.

An issue is the token's account's when that account is any of its assignees, since GitHub lets several people share
one; otherwise it is its first assignee's.

For picking, `GET /api/v1/github-credentials/{id}/repositories/{owner}/{name}/issues` answers a repository's open
issues, or its open pull requests with `kind=pull-request`, from one page of the hundred most recently updated;
`.../milestones` and `.../labels` answer up to 500 of each. Each entry carries the `reference` a link request names:
`owner/name#12`, `owner/name#3` for a milestone, `owner/name:label`.

Closing an issue and commenting need write access to issues: `repo` on a classic token (`public_repo` for public
repositories only), or Issues read and write on a fine-grained one. A token without it leaves the close or the comment
pending on its link with GitHub's answer. Reading whether a linked pull request merged needs Pull requests read on a
fine-grained token; without it, the watcher leaves that pull request's link as it was and logs why, and the rest of the
token's links are read as usual.

An issue a Project Manager files for an item (`docs/engine.md`, Its writes) is opened with the token the project's
sessions get (Which token a session gets, above), in the repository its project's tracker names, then read back and
linked as the item's primary. A filing tried before first lists the issues that token's account opened in the
repository since its first attempt and links the one with exactly its title, so a retry never opens two. A repository
is named as a tracker from the token's listing, archived ones left out, and checked with GitHub before it is stored.

### Assigned issues

A GitHub connector's content is the repositories whose issues assigned to the token's account come in as inbox items
(`docs/action-items.md`, Assigned issues): `{"repositories": "*"}`, every one the token reaches, or a list picked from
what the token lists (Listing a token's repositories, below), stored as GitHub spells each name. A token added under
Settings, GitHub reads every repository; the connector's page narrows it. The content bounds only what comes in: a
link can still be made to anything the token reaches.

The watcher bounds the read in what it asks GitHub, as Jira's and YouTrack's reads are bounded in their queries. A
connector reading every repository takes one listing for the account, `GET /issues?filter=assigned&state=open`, which
costs one request for most people whatever number of repositories it spans. One that chose repositories lists each of
them, `GET /repos/{owner}/{repo}/issues?assignee=<login>&state=open`, a request per chosen repository, so issues
assigned elsewhere never use up the read before a chosen repository is reached. Either listing takes at most ten pages
of a hundred, and pull requests are left out. A chosen repository the token cannot read fails the read, naming it, so
the connector shows which pick to change. A fine-grained token answers for the repositories it was granted; that the
account listing works with one has not yet been checked against GitHub itself.

## Merging a worker's pull requests

Elysium merges a pool worker's own pull request in a repository its project set to auto-merge (`docs/engine.md`,
Merging), through the project's token, the one its workers get. The calls are in `api/src/github/pulls.rs`:

| Call                                                | Used for                                              |
|-----------------------------------------------------|-------------------------------------------------------|
| `POST /graphql`                                     | One pull request, read whole for a merge decision     |
| `GET /repos/{owner}/{repo}/rules/branches/{branch}` | The rules its base branch's rulesets enforce          |
| `PUT /repos/{owner}/{repo}/pulls/{number}/merge`    | The merge, with `merge_method` and the head `sha` read |

- **One read.** Review threads, and whether each is resolved, exist only in GraphQL, so one query reads everything a
  decision needs: the token's account (`viewer`), the repository's allowed merge methods, and the pull request's
  state, author, fork, draft, `mergeable`, `mergeStateStatus`, `reviewDecision`, latest reviews, the first hundred
  review threads with their count, the classic protection a non-admin sees on its base branch (`refUpdateRule`), and
  its head commit's first hundred checks, check runs and commit statuses both. The field names were checked against a
  live answer, and need no preview header. A pull request or repository the token cannot see answers a `NOT_FOUND`
  error beside a null, read as "cannot see"; any other error, or an answer that does not read, is a failure, which
  holds the pull request.
- **Rules.** Rulesets are read from REST, which anyone who can read the repository may: required status checks by
  name, allowed merge methods (every ruleset that names some narrows the rest), a required linear history, and a merge
  queue. A branch with more rules than one page is refused, since a rule not read could be one that forbids the merge.
- **The merge** names the head commit the read saw, so GitHub answers `409` rather than merge a later push. It enforces
  the branch's protection on the call itself (`405` for a pull request its rules keep from merging), and Elysium asks
  it to bypass nothing. Any answer but `200` with `merged: true` leaves the pull request open with GitHub's message.
- **Permissions.** Per GitHub's documentation, not yet observed with a fine-grained token: reading needs Pull requests
  read and Commit statuses read on a fine-grained token (Checks read too for check runs), or `repo` on a classic one;
  merging needs Contents and Pull requests write, or `repo`. A token without them leaves the pull request open saying
  what GitHub answered, so a missing permission is never silent.

## Problem reports

Report, beside an error in the app, files the problem on `JalapenoLabs/Elysium` with one of the reporter's own tokens,
so it is posted as them; see `docs/issue-reporting.md`. Opening an issue or commenting there needs `public_repo` (or
`repo`) on a classic token. A fine-grained token reaches only its resource owner's repositories, so it reports only when
it was made for JalapenoLabs. GitHub refusing a write answers `403`, or `404` for a repository the token cannot see; the
client reports both as `GithubError::NoAccess`, which the report turns into a refusal saying what token would work.

## Which permissions a token needs

The setup checklist beside the form describes what a coding session needs: `repo` on a classic token, and Contents,
Pull requests, and Metadata on a fine-grained one. A project whose workers' pull requests Elysium merges needs
Commit statuses and Checks read as well (Merging a worker's pull requests, above). Add `workflow` to a classic token, or Actions to a fine-grained one,
for agents that read or change GitHub Actions.

## Roadmap

- GitHub App installation tokens, issued per turn for an hour, so a session never holds a long-lived token. Turns
  already take their token afresh.
- More than one token per session, chosen per repository owner, if sessions spanning owners with fine-grained tokens
  become common.
- Repository, pull request, and issue views built on the stored tokens, beyond the pickers and links above.
- GitHub Enterprise Server, which needs a host per credential and a validated allowlist.
