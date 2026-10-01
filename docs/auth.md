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
  proxies with the prefix stripped. The admin API (port 4434) is reachable on the compose network alone, and only
  the API calls it (`api/src/auth/kratos.rs`).
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
    run only
    for a new data volume, so an existing deployment would otherwise never get one.
  - `kratos-migrate` then applies Kratos's own schema.
- **Image.** The image is pinned by tag and digest in `compose.yml`, and the configuration is mounted read-only
  rather than baked into it.

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

Sign-up is one form: name, email, and a password. Passkeys are added from Settings afterwards, so every account has
a password and one way to recover it.

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
- **Names.** Kratos refuses a blank name. One that reaches the API blank anyway, such as an identity written through
  Kratos's admin API, takes the email's local part.

Email addresses are stored lowercased. There is no email verification, since there is no mail yet: an admin approving
the sign-up is the check.

## Signing in

- **Password.** Email and password.
- **Passkey.** Any WebAuthn authenticator, including password managers such as Bitwarden and the browser's own. The
  login page offers passkeys in the email field's autofill where the browser supports it.
- **Authenticator app.** A person who has enrolled one enters its code, or a lookup code, after their password or
  passkey.

Kratos enforces the second factor itself (`required_aal: highest_available`), and a passkey counts as the first
factor, so a person with an authenticator app is asked for its code after a passkey too. Someone who wants a
one-step sign-in uses a passkey without enrolling an app.

## Sessions

- **Lifetime.** A session lasts 72 hours from its last use.
  - Kratos extends a session only when asked. The API asks on the first request after a session is an hour old
    (`session.earliest_possible_extend` of 71 hours), which resets it to a full 72 hours.
  - The event stream's own reconnects do not count as use.
  - There is no "remember me": every session is the same.
- **Cookie.** `elysium_session` is first-party and `HttpOnly`, with `SameSite=Lax`, and set without a `Domain`
  attribute.
- **Caching.** The API asks Kratos whose each session is (`whoami`) and caches the answer in Redis for 30 seconds,
  under a hash of the cookie (`api/src/auth/sessions.rs`). A session Kratos revokes keeps working for at most those
  30 seconds.
- **Disabling is immediate.** Each request reads the person's `users` row, so a person disabled by an admin is
  refused on their next request.
- **Password changes.** Changing a password signs out every other session.
- **Recent sign-in.** Changing a password, passkey, or authenticator requires having signed in within the last 15
  minutes. Kratos asks for a fresh sign-in otherwise.

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

Refusals are JSON with a stable `code` beside the `message`. Admin-only routes answer `403 admin_only`.

The event stream checks its session and person every 30 seconds and closes when either loses access. The browser
then reconnects and is refused.

## Roles

`users.role` is one enum:

| Role     | Who                                         | May                                       |
|----------|---------------------------------------------|-------------------------------------------|
| `admin`  | People                                      | Everything, including users and settings  |
| `member` | People                                      | Everything but users and settings         |
| `guest`  | People                                      | The same as a member, for now             |
| `agent`  | Elysia and the coding agent                 | Never signs in                            |
| `system` | The system user                             | Never signs in                            |

- **Guest.** Guests are meant to see and do less than members. That is on the roadmap; today they have a member's
  access.
- **Machine roles.** The database ties the two machine roles to having no Kratos identity, so a machine can never
  sign in and a person can never hold a machine's role.
- **Requests.** Requests can assign only `admin`, `member`, or `guest`.
- **Permission checks.** Checks match roles explicitly (`is_admin`), never "anything but guest", so a new role gains
  nothing by accident.

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
- **Reset authenticator.** This removes a person's authenticator app and lookup codes and signs them out, MCP clients
  included, for someone who lost both.
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
- **Using a link.** A link works for 15 minutes, once. It signs its holder in and opens Settings, Sign-in & security
  to set a new password. A person with an authenticator app still enters its code.
- **Privacy.** The page answers the same whether or not the address has an account, and Kratos sends nothing for an
  unknown address.

Both internal routes live under `/internal`, which nginx never forwards (it proxies `/api/` alone). Each also checks
the hook key Kratos sends in `X-Elysium-Hook-Key`.

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
- satellites
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
- Mail: email verification at sign-up, and recovery links sent rather than logged.
- `users` as the owner of action items, replacing `owner_kind = 'user'`, which predates accounts.
