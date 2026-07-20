# Identity

Elysium owns identity end to end: users, credentials, organizations, teams, and
memberships live in the application's Postgres, and the API brokers every
authentication flow. The authentication architecture itself (methods, ceremonies,
sessions, org auth policy) is specified in `auth-model.md`; this document covers
the identity domain around it.

## Principles

- **Passkey-first, not passkey-only.** A passkey (WebAuthn) is the flagship
  factor; passwords (OPAQUE) plus a mandated TOTP second factor are the
  compatible path. See `auth-model.md` for the method set and decisions.
- **The API is the credential authority.** Credential storage, verification, and
  every ceremony are the API's (`webauthn-rs` today; `opaque-ke` and `totp-rs` on
  the roadmap). No external IdP is required to run Elysium.
- **Our own React UI.** Every auth screen is Elysium's SPA (see `auth.md`).
- **Invite-only, with a first-run root.** A fresh instance is claimed by its first
  (root) user; everyone after that arrives by a copyable invite link. No open
  public registration.
- **A profile is global; membership is many-to-many.** A `users` row is a
  deployment-global profile (identity + credentials + a globally-unique email)
  that can belong to many orgs. A person may hold several distinct profiles and be
  signed into more than one at once (see Profiles, orgs, and sessions below).

## Identity schema (database)

Applied at startup via `sqlx::migrate!` from `backend/migrations/`:

- `orgs` (tenants), `teams` (groups within an org), `users` (a deployment-global
  profile, keyed by a stable opaque `subject`, with a globally-unique email),
  `org_members` (profile-org edges with a role) and `team_members` (profile-team
  edges with a role).
- Names follow the Slack model: `given_name` + `family_name` (both optional) are
  the full name; `display_name` is the single field the UI renders and defaults to
  `given_name` when blank. Never concatenate given + family for display.
- Roles (`member`/`admin`/`owner`) are text guarded by CHECK constraints.
- `manual_status` (`out_of_office`/`do_not_disturb`/`custom`) + `manual_status_text`
  + `manual_status_expires_at`: the user-set status override. **Presence itself
  (`online`/`away`/`offline`) is derived, never stored** (see below).
- `webauthn_credentials`: each user's passkeys (credential id, the serialized
  `Passkey`, signature counters).
- `user_password_credentials`: each user's OPAQUE record + `scheme_version`.
- `user_totp_secrets`: each user's TOTP secret + `confirmed_at`.
- `opaque_server_setup`: the process-wide OPAQUE server setup (singleton).
- `sessions`: browser sessions behind the opaque HttpOnly cookie, each carrying an
  `active_user_id` + `active_org_id`; `session_accounts` lists the profiles signed
  into a session.
- `org_auth_policies`: one row per org; the org-enforced auth controls
  (allowed methods, mandatory TOTP, session lifetime).
- `login_events`: the append-only sign-in audit trail.
- `invites`: single-use, expiring onboarding links (only the token hash is stored).
- `recovery_codes`: one-time codes for regaining access (only hashes stored).
- A trigger keeps `updated_at` columns honest.

## Profiles, orgs, and sessions

Three ideas, kept distinct (`backend/src/{session,accounts,orgs}.rs`):

- **Profile** (`users`): a deployment-global identity with credentials and a
  globally-unique email. Sign-in authenticates a profile by email, regardless of
  which orgs it belongs to. The same person can hold several profiles (e.g.
  `adnavarro001@gmail.com` and `alex@mooreslab.ai`).
- **Org membership** (`org_members`): a profile joins many orgs, each membership
  carrying its own role. First-run claim makes the root org and its owner;
  `POST /api/v1/orgs` creates further orgs (any deployment can host several, e.g.
  NavarroCity + JalapenoLabs + MooresLabAI); invite acceptance adds a membership,
  and an already-signed-in profile can join a second org in one click
  (`accept/join`). Because a profile can be in many orgs, method discovery is the
  union of its orgs' policies (see `auth-model.md`).
- **Session** (`sessions` + `session_accounts`): a browser session holds several
  signed-in profiles with one active (Google-style), and the active profile has one
  active org. **Adding an account is a normal sign-in performed while a session
  cookie is present**, which `session::establish` appends rather than replaces.
  `GET /api/v1/session/accounts` lists them; `POST /api/v1/session/switch`,
  `.../switch-org`, and `.../signout` drive the profile and org switchers.
  `POST /api/v1/auth/logout` signs out every account on the device.

### Project-issued emails (roadmap)

The model above already allows an org to provision a member by email (invites). The
next layer, once the mail service lands, is **project-owned email domains**: a
**project** (the sub-tenant workspace under an org, e.g. NavarroCity.com within the
NavarroCity org) owns its domain and issues addresses under it
(`alex@navarrocity.com`), so a profile may be provisioned by the project and carry a
distinct initial-authentication path (project-provisioned credentials). The org
stays the umbrella (identity, membership); the **project** owns the domain and the
mail server. Email stays the global profile key throughout; domain ownership and
managed provisioning sit on top of it. See `docs/projects.md` and `docs/mail.md`.
This is not built yet.

## Presence and status

Presence (`online` / `away` / `offline`) is **derived, never a stored choice**:
`GET /api/v1/users/me` computes it at read time. Online means the user holds at
least one live SSE connection (`SseRegistry::is_online`); offline means none
(render "last seen" from `last_seen_at`); `away` (connected-but-idle) awaits a
client idle signal. So onboarding never asks the user to pick a presence.

What the user *can* set is a **manual status override** with an expiry
(`PUT /api/v1/users/me/status`): `out_of_office`, `do_not_disturb`, or a free-text
`custom` label, defaulting to a 24-hour lifetime. While it is unexpired it
supersedes derived presence; once `manual_status_expires_at` passes it reads back
as none and presence reverts to automatic. Render precedence: show the override
when set, otherwise the derived presence.

The profile surface lives in `backend/src/profile.rs`: `GET`/`PATCH
/api/v1/users/me` for names, display name, bio, preferred `language`
(`LANGUAGE_UNSPECIFIED` = auto-detect), and the contact/personalization fields
(pronouns, web link, contact phone, backup email), and `PUT
/api/v1/users/me/status` for the override. The `PATCH` is a full replace: every
editable field is sent on save, an empty value clears it, so callers send the
current value for fields they are not changing. All speak protobuf
(`elysium.user.v1.User` / `UpdateProfileRequest` / `SetStatusRequest`).

## Provisioning

- **First run**: the atomic bootstrap (`backend/src/bootstrap.rs`) creates the org
  (slug `root`), its seeded auth policy, the root user, the chosen credential, and a
  session in one transaction, only after the credential verifies. The subject is a
  locally minted UUID. See `auth-model.md` for the method-aware begin/finish contract.
- **Steady state**: invite links (org/team/role/expiry) minted by any member;
  acceptance (`backend/src/invites.rs`) atomically provisions the user, their
  membership, their first credential, and recovery codes, and consumes the invite.
  Delivery is copy/paste, never SMTP-dependent. See `docs/auth.md`.

## Roadmap

- Signed invite links and invite acceptance (mint, verify, accept endpoints).
- A permission model over teams.
- User directory surfaces: profile, avatar, presence (`last_seen_at` feeds
  "last seen").
- Per-org SSO / custom OAuth federation, API-brokered (see `auth-model.md`).
