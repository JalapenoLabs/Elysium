# Accounts and sign-in

Elysium is one shared workspace. Everyone who signs in sees and changes the same things, and every row records who
created it. Ory Kratos owns identities; Elysium owns roles, approval, and attribution.

## Kratos

[Ory Kratos](https://www.ory.sh/kratos/) v26.2.0 runs beside the API (`kratos` in `compose.yml`) and handles
everything about proving who someone is:

- passwords
- passkeys
- authenticator apps (TOTP) and lookup codes
- sessions
- recovery links

Elysium never stores or sees a password. Kratos has no UI of its own here: the React frontend renders its flows
through Kratos's JSON API.

- **Reaching it.** Browsers reach Kratos's public API on Elysium's own origin at `/api/identity/`, which nginx
  proxies with the prefix stripped, for everything but ending a session, which they ask the API for (Ending a
  session, below). The admin API (port 4434) is reachable on the compose network alone, and only the API calls it
  (`api/src/auth/kratos.rs`).
- **Configuration.** `kratos/kratos.yml` is a template. `kratos/entrypoint.sh` fills it from two variables the stack
  already has, `ELYSIUM_PUBLIC_URL` and `ELYSIUM_ENCRYPTION_KEY`, so the environment file holds nothing for Kratos.
  - Kratos's cookie, cipher, and default secrets, and the key its webhooks carry, are each the hex SHA-256 of
    `elysium/kratos/<purpose>:<encryption key>`. The cipher secret is the first 32 characters of its digest, since
    Kratos requires exactly 32.
  - `api/src/auth/hook_key.rs` derives the hook key the same way. Both sides pin one test vector, so they cannot
    drift apart.
  - Rotating `ELYSIUM_ENCRYPTION_KEY` rotates these secrets too, which signs everyone out and invalidates enrolled
    authenticator apps. The stored-secrets rotation in `docs/secrets.md` must account for that.
- **Database.** Kratos keeps its data in its own database, `kratos`, on the same Postgres server and with the same
  credentials as Elysium's.
  - `elysium-api migrate run` (the `migrate` service) creates it, and Hydra's, when missing. Postgres's init scripts
    run only for a new data volume, so an existing deployment would otherwise never get one.
  - `kratos-migrate` then applies Kratos's own schema.
- **Image.** `kratos/Dockerfile` pins Kratos's image by tag and digest and builds the configuration into it, and
  Compose rebuilds it on every `up`, so a deploy that changes any file in `kratos/` recreates Kratos on the new
  configuration. See `docs/infrastructure.md`, Configuration images.

### The public URL

`ELYSIUM_PUBLIC_URL` is the origin browsers use. Sessions, passkeys, and recovery links are bound to it.

- It must be `https://<host name>`, or `http://localhost[:port]` for development.
- Browsers keep passkeys only for a host name, never an IP address. The API refuses to start on any other value.
- It defaults to `http://localhost:${WEB_PORT}` in `compose.yml`.
- A deployment reached from other machines sets it in the environment file and puts TLS in front of nginx. That TLS
  is the operator's, like everything in front of the host.
- A development host reached over SSH is used through a tunnel (`ssh -L 4000:localhost:4000 <host>`), which keeps
  the localhost default working.

Over plain http, `entrypoint.sh` turns the cookies' `Secure` flag off. Kratos sets that flag on the session cookie
separately from the CSRF cookie, so both are configured.

## Signing up

Sign-up is one form: first and last name side by side, then email, and a password. Passkeys are added from Settings
afterwards, so every account has a password and one way to recover it.

Before Kratos stores a new identity, it asks the API (`POST /internal/kratos/registration`, a pre-persist webhook).
The API refuses in two cases:

- **Sign-up is closed.** An admin closed it, and someone has already signed up. The first person can always sign up,
  so a new workspace can never lock itself out.
- **It is a passkey sign-up.** Every account starts with a password.

A refusal appears on the form under the email field, as Kratos message `4190001` (sign-up closed) or `4190002`
(password required).

Once Kratos has stored the identity, a second webhook (`POST /internal/kratos/registered`, post-persist) creates the
person's account, so accounts are created in the order people finished signing up.

- **The first person** becomes an active admin.
- **Everyone after** is pending, with no role, until an admin approves them.
- **Concurrent first sign-ups.** An advisory lock serializes account creation, so two people signing up at once
  never both become admin.
- **A lost webhook.** The API also creates any missing account on the person's first request
  (`api/src/auth/middleware.rs`), so an identity is never left without one.
- **Names.** Kratos refuses a blank first name; the last name is optional, for people with one name. The account's
  `users.name` is the two joined by a space, the name the whole workspace shows. One that reaches the API blank
  anyway, such as an identity written through Kratos's admin API, takes the email's local part.

Email addresses are stored lowercased. There is no email verification, since there is no mail yet: an admin approving
the sign-up is the check.

## Identity schemas

An identity's traits are its email and its name, `{ first, last }`, described by `kratos/identity-v2.schema.json`
(schema `person-v2`). Kratos validates traits only when an identity is written and never rewrites a stored identity
when a schema changes, so a change to what identities hold is a new schema, never an edit of the old one:

- **Both schemas are registered.** `kratos/kratos.yml` registers the legacy `person` schema
  (`kratos/identity.schema.json`, one `name` string) beside `person-v2`, and new identities take `person-v2`.
- **The API moves identities on.** On every start, `api/src/auth/identity_schemas.rs` lists every identity, in the
  background and retrying until Kratos answers, and moves each one still on `person` to `person-v2` with one JSON Patch
  that names the new schema and replaces only the name. Kratos validates the result against `person-v2` and never
  touches credentials, so passwords, passkeys, and authenticator apps keep working. An identity Kratos refuses to move
  is logged (`auth.identities.migrate.refused`) and keeps the legacy schema, which still signs in, until the next
  start.
- **Splitting a legacy name.** The first word is the first name and the rest the last name, which keeps compound
  surnames ("García Márquez") whole; a one-word name has no last name. People correct a wrong split on My profile.
- **Until an identity moves.** The API reads either shape (`PersonName` in `api/src/auth/kratos.rs`), and My profile
  shows a legacy name split the same way, so nobody is locked out in between.

The move is one way. A release from before `person-v2` neither registers it nor reads a split name, so it must never
run against identities that have moved. The updater's rollback is safe, since it restores Kratos's database from the
backup it took before the update along with Elysium's; pinning an older release by hand needs the same restore.

Removing the legacy schema, and the code that reads it, is on the roadmap once every deployment has started a release
that moves identities.

## Signing in

- **Password.** Email and password.
- **Passkey.** Any WebAuthn authenticator, including password managers such as Bitwarden and the browser's own. The
  login page offers passkeys in the email field's autofill where the browser supports it.
- **Authenticator app.** A person who has enrolled one enters its code, or a lookup code, after their password or
  passkey.

Kratos enforces the second factor itself (`required_aal: highest_available`), and a passkey counts as the first
factor, so a person with an authenticator app is asked for its code after a passkey too. Someone who wants a
one-step sign-in uses a passkey without enrolling an app.

### Authenticator nicknames

A person may name their authenticator app, such as "Bitwarden", when they set it up or later, so they remember which
app holds Elysium's codes. Kratos holds one app per identity and has nowhere for a name, so it lives in Elysium's
`authenticator_nicknames`, one row per account (`api/src/models/authenticator_nickname.rs`):

- **Written by the person alone.** `PUT` and `DELETE /api/v1/me/authenticator-nickname` sit beside `/me`, outside the
  workspace's access rule, since someone the workspace requires to set up an app names it while doing so. `PUT`
  answers `409` unless Kratos holds an app to name.
- **Read through `/me`**, as `authenticatorNickname`. Sign-in & security shows it only while Kratos says the app is
  set up, so a name left behind is never shown beside another app.
- **Removed with the app.** Sign-in & security forgets it when the person removes their app, and when they set one
  up without a name; an admin's Reset authenticator forgets it too.

## Sessions

- **Lifetime.** A session lasts 72 hours from its last use.
  - Kratos extends a session only when asked. The API asks on the first request after a session is an hour old
    (`session.earliest_possible_extend` of 71 hours), which resets it to a full 72 hours.
  - The event stream's own reconnects do not count as use.
  - There is no "remember me": every session is the same.
- **Cookie.** `elysium_session` is first-party and `HttpOnly`, with `SameSite=Lax`, and set without a `Domain`
  attribute.
- **Caching.** The API asks Kratos whose each session is (`whoami`) and caches the answer in Redis for 15 minutes,
  never past the session's own expiry, under a hash of the cookie, filed under the person's Kratos identity
  (`api/src/auth/sessions.rs`). A session that ends is forgotten at once (below), so the cache never decides how long
  a revoked session lasts, and a busy session asks Kratos about four times an hour. With Redis unreachable, every
  request asks Kratos, and nothing is refused for it.
- **Rolling under the cache.** Kratos extends a session only when the API asks, so a cached expiry stays exact until
  then. The first request after a session is an hour old extends it and drops its cached answer, so the next request
  reads the new expiry.
- **Disabling is immediate.** Each request reads the person's `users` row, so a person disabled by an admin is
  refused on their next request.
- **Password changes.** Changing a password signs out every other session.
- **Recent sign-in.** Changing a password, passkey, or authenticator requires having signed in within the last 15
  minutes. Kratos asks for a fresh sign-in otherwise.

### Ending a session

A session that ends is refused on its very next request. Kratos tells nothing about a sign-out or a session ended
through its own routes: its logout flow and session routes take no webhooks. So the API ends sessions itself, and
every way one ends forgets the person's cached sessions in the same step:

| What ends it                              | How                                                                  |
|-------------------------------------------|----------------------------------------------------------------------|
| Signing out                               | `POST /api/v1/auth/sign-out` runs Kratos's logout flow with the browser's cookie and passes Kratos's cleared cookie back |
| Ending one of one's other sessions        | `DELETE /api/v1/me/sessions/{id}`, Kratos's own route asked with the person's cookie, so Kratos checks the session is theirs |
| Ending all of one's other sessions        | `DELETE /api/v1/me/sessions`, the same way                           |
| An admin signing someone out, resetting their authenticator, or disabling them | Kratos's admin API |
| An admin rejecting a sign-up              | Deleting the identity, which would otherwise come back as a new pending account from a cached session |
| A settings change, such as a new password | Kratos's settings webhook, `POST /internal/kratos/settings`         |

- **One way in.** nginx answers browsers Kratos's logout flow (`/api/identity/self-service/logout`) with a `404`, and
  its session routes (`/api/identity/sessions`) for reading alone, so these are the only ways a session ends. Sign-out
  needs no session of its own, so a session still waiting on its second factor signs out too. Ending one's own
  sessions sits beside `/me`, outside the workspace's access rule.
- **The settings webhook.** A new password signs the other sessions out (`revoke_active_sessions`), a new
  authenticator changes what a session must have done, and a new name is in the cached answer. Kratos calls the
  webhook once any settings change is stored and waits for it. Its global hooks run only for methods without hooks of
  their own, so the password method names the webhook too, after the revocation (`kratos/kratos.yml`).
- **Kratos first.** The API tells Kratos, then forgets every cached session of the person, so their other sessions
  ask Kratos once more. Forgetting first would let a request in between cache Kratos's answer from before.
- **The race.** A request that asked Kratos just before the session ended could cache the answer just after the
  person's sessions were forgotten. So forgetting also marks the person for a minute, longer than any call to Kratos
  may take, and nothing is cached for them while the mark stands; their requests still work, each asking Kratos. The
  check and the write are one Lua script (`api/src/auth/revocable_cache.rs`).
- **Redis unreachable.** Forgetting that fails is logged as an error (`auth.cache.revoke_failure`). The session has
  still ended in Kratos, and while Redis is unreachable nothing is read from it; only answers a Redis that comes back
  still holds could let the session through, until they expire.
- **What the API never hears.** A change made in Kratos directly, such as an operator using its admin API, reaches
  requests once the cached answer expires, within 15 minutes. That bound is what the cache's length is chosen for.

## Requests

Every `/api/v1` route needs a session, except `GET /api/v1/auth/status`, which the sign-in and sign-up pages read.
Three layers run in order (`api/src/routes/v1/mod.rs`):

1. **Same origin.** An unsafe request (anything but `GET`, `HEAD`, `OPTIONS`) must carry Elysium's own `Origin`.
   `SameSite=Lax` keeps other sites' requests from carrying the cookie, but a sibling subdomain counts as the same
   site.
2. **Authenticate.** This resolves the session and the person, creating their account the first time. It refuses
   with `401 unauthenticated`, or with `403 second_factor_required` when the session is waiting on a second factor.
3. **Require access.** This refuses pending (`account_pending`) and disabled (`account_disabled`) people. When the
   workspace requires an authenticator app, it also refuses a one-factor session (`mfa_enrollment_required`).
   `GET /api/v1/me` sits outside this layer, so anyone signed in can learn why they are waiting.

Refusals are JSON with a stable `code` beside the `message`. Admin-only routes answer `403 admin_only`. The knowledge
base's routes and MCP tools answer admins and members alone (`MemberUser`, whose `check` the MCP server calls too),
and a guest `403 members_only` (`docs/knowledge.md`, Who sees what).

The event stream checks its session and person every 30 seconds and closes when either loses access. The browser
then reconnects and is refused.

## Roles

`users.role` is one enum:

| Role     | Who                                         | May                                       |
|----------|---------------------------------------------|-------------------------------------------|
| `admin`  | People                                      | Everything, including users and settings  |
| `member` | People                                      | Everything but users and settings         |
| `guest`  | People                                      | A member's access, but no knowledge base  |
| `agent`  | Elysia and the coding agent                 | Never signs in                            |
| `system` | The system user                             | Never signs in                            |

- **Guest.** Guests are meant to see and do less than members. Today the knowledge base is the one thing refused them
  (`is_company_member`); the rest is on the roadmap.
- **Machine roles.** The database ties the two machine roles to having no Kratos identity, so a machine can never
  sign in and a person can never hold a machine's role.
- **Requests.** Requests can assign only `admin`, `member`, or `guest`.
- **Permission checks.** Checks match roles explicitly (`is_admin`, `is_company_member`), never "anything but guest",
  so a new role gains nothing by accident.

## Managing people

Admins manage people under Settings, Users (`/api/v1/users`).

**Account changes:**
- **Approve** a pending person with a role, which is Member unless the admin picks another.
- **Reject** a pending person. This deletes their account and their Kratos identity, so the address can sign up
  again. An approved person is never deleted, because rows they created name them.
- **Change a role.**
- **Disable** or **enable** a person. Disabling stops Kratos from signing them in, signs them out everywhere, and
  refuses their next request.

**Security actions:**
- **Sign out everywhere.** This also disconnects every MCP client they connected (`docs/mcp.md`).
- **Reset authenticator.** This removes a person's authenticator app, its nickname, and their lookup codes and signs
  them out, MCP clients included, for someone who lost both.
- **Create a recovery link.** The admin hands the one-time link over; it works for 15 minutes.

**Workspace settings:**
- Sign-up open or closed.
- Require an authenticator app of everyone.

**Guardrails and audit:**
- The workspace always keeps at least one active admin. Demoting or disabling the last one answers `409` with code
  `last_admin`. Every such change locks the active admins in one order, so two admins demoting each other at once
  queue, and the second is refused. Anything else about oneself is allowed.
- Changing whether someone may sign in, and rejecting a sign-up, tell Kratos inside the same database transaction. If
  Kratos refuses, the change rolls back, so Elysium and Kratos never disagree about an account.
- Every account change is recorded in `user_events` with its actor, including profile changes a person made in
  Kratos and workspace settings changes. A rejected sign-up's events outlive it, with their user cleared.

Every active person may list users, since every row names its creator. Only admins see how each person signs in, read
from Kratos per identity.

## Recovery

There is no mail yet.

- **Forgot password.** Kratos sends its "email" to the API (`POST /internal/kratos/courier`, the courier's HTTP
  delivery). The API logs the recovery link at `WARN` as `auth.recovery.issued`, with the address it was requested
  for. An operator finds it with `docker compose logs api` and passes it on.
- **Admin-issued links.** A link created from Settings, Users needs no log access.
- **Using a link.** A link works for 15 minutes, once. It signs its holder in and opens Settings, Sign-in & security,
  where Change password sets a new one. A person with an authenticator app still enters its code.
- **Privacy.** The page answers the same whether or not the address has an account, and Kratos sends nothing for an
  unknown address.

Both internal routes live under `/internal`, which nginx never forwards (in development it proxies `/api/` alone; in
production, where every other path reaches the API, it answers `/internal` with a `404` itself). Each also checks the
hook key Kratos sends in `X-Elysium-Hook-Key`.

When Elysium sends mail, the courier's delivery switches to SMTP and the log line goes away.

## Brute force

Self-hosted Kratos has no brute-force protection. nginx throttles POSTs to Kratos's sign-in, sign-up, and recovery
endpoints to 5 a minute per client address, after a burst of 5, answering `429`. Loading a form is not throttled.

The client address is the one nginx sees connecting, unless a proxy in front of it is trusted. A deployment behind a
TLS terminator or load balancer must set `TRUSTED_PROXY_ADDRESSES` to that proxy's address or CIDR
(`nginx/real-ip.conf.template`), so nginx takes the client's address from its `X-Forwarded-For`. Without it, every
client arrives as the proxy, and the limit is shared by the whole workspace: a few mistyped passwords lock everyone out
of signing in for a minute. The default trusts nobody, which is right when nginx faces clients directly.

There is no per-account lockout. It would let anyone lock out a known person by guessing at their address, and Kratos
cannot count failed sign-ins per account without Elysium wrapping its API. The per-address limit, together with a
12-character minimum, Kratos's breach check, and optional authenticator apps, is the defense.

## Passwords

Kratos enforces:
- at least 12 characters, with no maximum
- no password found in a known breach (HaveIBeenPwned range queries, which send only the first five hex characters of
  the password's SHA-1)
- no password that resembles the email address

Passwords are hashed with Argon2id. The frontend additionally requires an uppercase letter, a special character, and
a zxcvbn score of at least 3 of 4 before it lets a form submit. Kratos cannot check character classes, and its
webhooks never see the password, so those rules are enforced in the frontend alone.

## Attribution

Every top-level table has `created_by UUID NOT NULL REFERENCES users`:

- action_items
- changesets
- coding_sessions
- environment_variables
- github_credentials
- initiatives
- jira_credentials
- llms
- mail_accounts
- mail_domains
- mail_servers
- projects
- storage_locations

Join and child tables inherit their parent's creator. Every new top-level table gets the column too.

Machines are rows in `users` with fixed ids (`api/src/models/user.rs`):

| Id                                     | Name           | Creates                                                  |
|----------------------------------------|----------------|----------------------------------------------------------|
| `00000000-0000-0000-0000-000000000001` | Elysium        | Rows that predate accounts, and what the watcher imports |
| `00000000-0000-0000-0000-000000000002` | Elysia         | What the assistant proposes and applies                  |
| `00000000-0000-0000-0000-000000000003` | Coding agent   | What coding sessions propose and apply                   |

- **History and comments.** These name a person as `user:<id>`, with finer labels for machines (`elysia`,
  `session:<n>`, `watcher:<provider>`). Rows written before accounts say `user` alone.
- **Agents.** Agents read people by name: the tools and a session's first turn resolve `user:<id>` to the person's
  name.
- **Changesets.** A changeset's `created_by` is the machine that proposed it. Its `decided_by` is the person who
  applied or rejected it.
- **No `updated_by`.** Rows record no `updated_by`. Action items and initiatives keep full history, and other tables
  gain an editor when they gain history.

## Roadmap

- What guests may not see or do.
- Retiring the legacy `person` identity schema and the code that reads it.
- Mail: email verification at sign-up, and recovery links sent rather than logged.
- `users` as the owner of action items, replacing `owner_kind = 'user'`, which predates accounts.
