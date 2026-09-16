# GitHub

Elysium reaches GitHub with personal access tokens, managed on the GitHub settings page (`/settings/github`). Any
number of tokens are held at once, each named, so one Elysium can act as a work account and a personal one. Nothing
uses a token yet; this doc covers how tokens are stored, checked, and shown.

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

`api/src/github/mod.rs` is the only place that calls GitHub. It holds Elysium's shared HTTP client and has one call,
`verify`, which is `GET https://api.github.com/user` with the token, `Accept: application/vnd.github+json`, and
`X-GitHub-Api-Version: 2022-11-28`. GitHub refuses a request without a `User-Agent`, and the shared client sets
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

## Which permissions a token needs

Nothing in Elysium uses a token yet, so nothing is required. The setup checklist beside the form describes what the
first consumer, cloning repositories into coding sessions, will need: `repo` on a classic token, and Contents, Pull
requests, and Metadata on a fine-grained one.

## Roadmap

- Cloning private repositories into coding sessions, which needs the satellite SDK to carry credentials.
- Linking a credential to the projects that use it, as storage locations are linked.
- Repository, pull request, and issue views built on the stored tokens.
- GitHub Enterprise Server, which needs a host per credential and a validated allowlist.
