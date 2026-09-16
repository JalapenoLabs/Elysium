# GitHub

Elysium reaches GitHub with personal access tokens, managed on the GitHub settings page (`/settings/github`). Any
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

`api/src/github/mod.rs` is the only place that calls GitHub. It holds Elysium's shared HTTP client and sends every call
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
the default leaves the workspace without one. A session records the token its thread started with, since a thread
takes its environment once; a deleted token leaves that record empty.

`POST /api/v1/coding-sessions` takes `githubCredentialId`: absent follows the project, `null` asks for no token, and
an id names one. `api/src/routes/v1/coding_sessions/github_token.rs` resolves the choice and opens the token. A token
that cannot be decrypted refuses the session rather than starting the agent without the access it was promised.

## How the agent gets the token

Only one token per session, set in the thread's environment. No satellite change is involved:

- `GH_TOKEN` holds the token, declared secret, so the satellite scrubs it from events, incidents, and output, and
  scans pushes for it. `gh` reads it on its own.
- `git` does not read `GH_TOKEN`, so the thread also declares git config through the environment, which git 2.31 and
  later reads (the satellite image ships Ubuntu 24.04's git 2.43):

  | Variable             | Value                                  |
  |----------------------|----------------------------------------|
  | `GIT_CONFIG_COUNT`   | `2`                                    |
  | `GIT_CONFIG_KEY_0`   | `credential.https://github.com.helper` |
  | `GIT_CONFIG_VALUE_0` | empty, clearing any helper set elsewhere |
  | `GIT_CONFIG_KEY_1`   | `credential.https://github.com.helper` |
  | `GIT_CONFIG_VALUE_1` | `!gh auth git-credential`              |

  These are declared not secret, since an unset flag means secret and would scrub the helper from logs. Nothing is
  written into the workspace. The satellite's pre-push hook lives in repository config, so the two never meet.
- The satellite's own clone of the session's repository runs before the agent and does not see its environment, so
  an `https://github.com/` repository also gets the token as `repos[].auth`. SSH remotes get nothing; they
  authenticate with keys.
- The token also goes in `ThreadSettings.github`, which the satellite scrubs and its planned `gh` broker will use.

An agent can read every variable in its environment, so it holds the token. Scope each token to what its sessions
need.

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
checked against GitHub's alphabets before they become part of the path (`Repository::from_url`). The new session
modal runs the check as the repository field settles (`REPOSITORY_ACCESS_CHECK_DELAY_MS`) and says what the chosen
token can do, and warns about a token that has expired or expires within a week. A warning never blocks the session.

## Which permissions a token needs

The setup checklist beside the form describes what a coding session needs: `repo` on a classic token, and Contents,
Pull requests, and Metadata on a fine-grained one. Add `workflow` to a classic token, or Actions to a fine-grained one,
for agents that read or change GitHub Actions.

## Roadmap

- GitHub App installation tokens, issued per turn for an hour, so a session never holds a long-lived token. Needs the
  satellite to accept a fresh token per turn.
- More than one token per session, chosen per repository owner, if sessions spanning owners with fine-grained tokens
  become common.
- Repository, pull request, and issue views built on the stored tokens.
- GitHub Enterprise Server, which needs a host per credential and a validated allowlist.
