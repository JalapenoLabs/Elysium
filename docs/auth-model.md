# Authentication model (authoritative)

This document states Elysium's authentication architecture. Auth is first-party:
the API owns every credential flow and Postgres is the system of record. There is
no external identity provider in the deployment.

## The one rule

**The browser only ever talks to Elysium's own SPA and API, and the API brokers
all authentication.** Our React renders every input: the email field, the passkey
button, the (future) password and TOTP fields, recovery. No auth redirect ever
leaves the app's origin.

## Roles

- **SPA (React).** Renders the entire auth UI and drives the browser-native passkey
  ceremony via `navigator.credentials`. Holds only the session cookie.
- **API (Rust).** The **WebAuthn Relying Party** (`webauthn-rs`) and the broker for
  every other method as it ships: it issues challenges, verifies assertions, and
  owns the passkey / password / TOTP / recovery logic and the sessions.
- **Postgres (app DB).** The **system of record** for auth: users, their passkeys,
  login history, per-org auth policy, (future) OPAQUE password records, TOTP
  secrets, and recovery codes all live here.

## Sign-in methods

Elysium is deliberately **not passkey-only**. Passkeys are the flagship path;
passwords with a mandated second factor are the compatible path orgs can rely on.
Which methods an org's members may use is org policy (see below).

### Passkeys (built)

The primary, phishing-resistant method. The API is authenticator-agnostic: platform
authenticators (Windows Hello, Touch ID), hardware keys, and credential-manager
passkey providers (Bitwarden, 1Password, ...) all work through the same WebAuthn
ceremony. In the browser build, extension providers intercept
`navigator.credentials`; in the Electron shell the OS passkey-provider APIs carry
the same role (verify per platform when the shell lands).

### Passwords via OPAQUE (built)

Password sign-in uses the **OPAQUE aPAKE ([RFC 9807](https://www.rfc-editor.org/info/rfc9807/))**,
not password-over-TLS: the API never sees the password, any hash of it, or anything
replayable. Plain client-side hashing (e.g. SHA-256 before submit) is explicitly
rejected as a pass-the-hash anti-pattern; OPAQUE is the correct construction for
"the password never leaves the client".

- **Server**: `opaque-ke` 4.x (Meta's audited, RFC-compliant Rust implementation),
  suite ristretto255 OPRF + triple-DH over SHA-512 + Argon2 KSF. The server setup
  (its OPRF seed and keypair) is a singleton persisted in `opaque_server_setup` and
  loaded on boot; one registration record per user in `user_password_credentials`,
  with a `scheme_version` so a future cipher-suite bump can force re-enrollment on
  next login instead of a breaking migration. Pure crypto lives in `backend/src/opaque.rs`.
  The server setup and every TOTP secret are sealed at rest with XChaCha20-Poly1305
  under a required `ELYSIUM_SECRET_KEY` (`backend/src/crypto.rs`), so a database dump
  alone exposes neither; a row written before encryption reseals on first read.
- **Client**: `@serenity-kit/opaque` (a WASM build of the same crate), so both ends
  share one implementation and one pinned suite. Wire messages are **base64url
  without padding** (the encoding that library emits); the server matches it. Cross-
  language interop is verified end to end.
- **Identifier**: the OPAQUE credential identifier is the **lowercased, trimmed
  email**, used identically at registration and login so the OPRF keys agree.
- **Flow**: registration is one round trip; login is the KE1/KE2/KE3 exchange (two
  round trips). A wrong password fails client-side (no valid finalization), and an
  unknown user gets an indistinguishable dummy response (`ServerLogin::start(None)`),
  so the endpoint never reveals which accounts exist. Success mints the same opaque
  session cookie as a passkey sign-in.
- **Rate limiting**: `password/authenticate/begin` returns `429` once an identifier
  has 5 failures in 15 minutes, counted from `login_events`. This blunts online
  guessing (offline attack is already out of reach by construction).
- **Accepted tradeoffs**: password-strength rules and breach checks (HIBP
  k-anonymity) run client-side, because the server never sees the password.
- **Reserved bonus**: OPAQUE's `export_key` (a stable client-side key the server
  never learns) is a foundation for future end-to-end-encrypted features.

### Passwordless email sign-in (designed, not built)

Sign-in by proving control of an email inbox, in two interchangeable shapes: a
single-use **magic link** and a typed **one-time code** (the code is the friendlier
path when the link would open on a different device). It is a *sign-in* method, not a
stored credential: unlike a passkey or an OPAQUE record there is nothing per user to
enroll, so it is available to any account whose email is verified and whose org policy
permits it.

**Prerequisite.** Delivery rides Elysium's own (planned) SMTP service; this method
lands only after outbound mail exists. First-run **claim stays passkey / password**
(neither needs an external service), which avoids the chicken-and-egg of emailing a
link from an instance whose mail is not yet configured. Email sign-in becomes
available once an operator configures SMTP and an org enables the method. Invite
delivery stays copyable-link regardless, so air-gapped installs never need email.

**Challenge store.** `email_login_challenges` holds one row per outstanding request:
the lowercased email, the SHA-256 **hash** of the link token and of the code (never
the plaintext, so a database dump cannot be replayed to sign in), a purpose, the
resolved `user_id` when known, a short expiry (~15 min), a single-use `consumed_at`,
and an attempt counter. Verifying compares hashes, checks liveness and single-use,
then consumes the row.

**Flow and endpoints.**

- `POST /api/v1/auth/email/begin` `{ identifier }` -> always `204` (non-enumerating,
  like `password/authenticate/begin`): a challenge is minted and mailed only when the
  email resolves to a user whose org permits the method, but the response never
  reveals that. Issuance is rate-limited per email and per IP (`429` on abuse) so the
  endpoint cannot be used to flood an inbox.
- `POST /api/v1/auth/email/finish` `{ token }` (link) or `{ identifier, code }`
  (code) -> verifies and consumes the challenge, then mints the same session cookie as
  every other method, or answers `401 { reason: "totp_required" }` when the org
  mandates TOTP. The high-entropy link needs no attempt cap; the short code locks
  after a few wrong tries (counted in `login_events`, reusing `rate_limit.rs`).

**Security stance.** Inbox possession is a genuine factor but a weaker one than a
passkey, so a `require_totp` org still forces the second factor after an email
sign-in, exactly as the password path does. The method is gated in `org_auth_policies`
(a new `magic_link` allowed-method flag, default **off**) and by a per-user
`email_verified_at`: claiming or accepting an invite verifies the address inherently,
and a passkey / password account verifies on its first successful email challenge.

**Client contract (handed to the FE once SMTP lands).** An "Email me a link or code"
card in the method chooser, shown only when `/auth/methods` reports the method
enabled, plus a screen to type the code and a landing route that posts the link token
to `finish`. The `magic_link` kind and its copy already exist in the frontend method
registry, unused, waiting on this.

### TOTP second factor (built)

TOTP is a second factor verified by the API (`totp-rs`, SHA-1 / 6 digits / 30s, one
step of skew) against a secret stored in `user_totp_secrets`; enrollment returns an
`otpauth://` provisioning URI the SPA renders as a QR (any RFC 6238 app works: Google
Authenticator, Bitwarden, ...). It ships together with password login: claiming or
signing in with a password requires a confirmed TOTP code, so a password without a
second factor is never a complete method. After a correct password,
`password/authenticate/finish` answers `401 { reason: "totp_required" }` and the flow
completes at `totp/verify`. Recovery codes (one-time codes issued at enrollment) are
the next roadmap item.

### SSO / custom OAuth providers (roadmap)

Per-org federation is API-brokered: the org configures its provider (issuer URL,
client id/secret) in Postgres, and the API runs the OIDC/OAuth client flow
(`openidconnect`) server-side. The browser still only ever holds the Elysium
session cookie.

## First-run claim (atomic, method-aware)

A fresh instance has no users; the first (root) user claims it. The claim is a
two-step wizard (name the workspace, then create the owner with a chosen method)
backed by an **atomic** begin/finish contract in `backend/src/bootstrap.rs`:

- **`begin` persists nothing.** It mints a candidate user id and, per the chosen
  method, either starts a passkey registration challenge or answers the OPAQUE
  registration request and issues a TOTP provisioning URI. All of it (candidate id,
  entered details, method-specific ceremony state) rides a short-lived HttpOnly
  cookie.
- **`finish` verifies the credential, then writes everything in one transaction**:
  the org, its seeded `org_auth_policies` row, the user, the credential
  (passkey, or OPAQUE record + confirmed TOTP secret), and a session. The chosen
  method seeds the policy (passkey -> passkeys allowed; password -> passwords
  allowed and TOTP mandatory).

Because nothing is written until the credential verifies, abandoning `begin` (for
example closing the passkey dialog) leaves the instance **unclaimed** rather than
claimed-with-no-way-in. This closes the earlier lockout hole where the org and user
were created before the ceremony completed.

## Org auth policy (built; enforcement expanding)

`org_auth_policies` (one row per org, created with the org) is the control surface
for org-enforced auth rules: which sign-in methods are allowed (at least one must
stay enabled), whether TOTP is mandatory, and the session lifetime. The row is
seeded at claim time from the owner's chosen method, and method discovery
(`/api/v1/auth/methods`) already reads it. Remaining enforcement (rejecting a
disabled method mid-flow, applying `require_totp` to every member, honoring
`session_lifetime_seconds`) lands with the Security Center admin surface.

## Login history (built)

`login_events` is an append-only audit trail: every sign-in attempt (successful or
not) records the user (when resolved), the presented email, method, outcome, client
IP (nearest proxy hop, falling back to the socket peer), and user agent. The trail
outlives account deletion (`ON DELETE SET NULL`). It feeds the Security Center's
account-access view and the sliding-window rate limiting: every verification path
(password, TOTP, recovery) is throttled by a count of recent failures read from
here, which doubles as a temporary, self-expiring lockout (`backend/src/rate_limit.rs`).

## Sessions

An opaque HttpOnly, Secure, SameSite=Lax session cookie backed by the `sessions`
table; the `SessionUser` extractor resolves it to the session's **active account**
and protects every `/api/v1/*` route and the SSE stream. Same-origin (the SPA
proxies `/api` to the API), so the cookie is first-party. Sessions hold no tokens.

A session is **multi-account** (Google-style): `session_accounts` lists every
profile signed into the browser session and `sessions.active_user_id` /
`active_org_id` say which account (and which of its orgs) is active. Adding an
account is a normal sign-in performed while the cookie is present, which
`session::establish` appends instead of replacing. Switching is only allowed to
accounts already in the session, so a caller can never switch to a profile it never
authenticated. See `docs/identity.md` for the profile/org/session model.

## WebAuthn configuration

- **RP ID** = the app's registrable domain (`localhost` in dev, the deployment
  domain in production). **RP origin** = the app origin (`http://localhost:5173` in
  dev). Because the API and SPA share one origin, the RP origin the API validates
  matches the origin the browser signs against.
- Credentials are discoverable (resident keys) with user verification required, so a
  passkey is a true passwordless factor.

## Build notes

- The RP uses `webauthn-rs` 0.5 (`WebauthnBuilder::new(rp_id, rp_origin)`), verified
  to build on Linux. It pulls in native OpenSSL (`openssl-sys`), unlike the rest of
  the backend which is rustls: the backend image already has `libssl-dev`, and CI
  installs `openssl-devel` on the Fedora runner. This also means the backend does
  not compile on a bare Windows host; build inside the dev container.
- The registration/authentication ceremony state (the `webauthn-rs`
  `PasskeyRegistration` / `PasskeyAuthentication`, behind the
  `danger-allow-state-serialisation` feature) is carried between begin and finish
  in a short-lived HttpOnly cookie, so no extra server-side state table is needed.
  Credentials (the `Passkey`) persist in Postgres, linked to the user.
- The SPA uses `@simplewebauthn/browser` (`startRegistration` / `startAuthentication`)
  to convert the challenge JSON to/from the `ArrayBuffer` shapes `navigator.
  credentials` needs, rather than hand-rolling base64url conversion.
- Client IPs reach the audit trail via `X-Forwarded-For` (the SPA proxy / the
  operator's gateway) with the socket peer as fallback (`ConnectInfo`).
- The OPAQUE and TOTP crates pull `sha2`/`argon2` (suite match) and native
  `ring`/`getrandom`; they build in the same dev container as `webauthn-rs`. The
  OPAQUE server setup is loaded from Postgres after migrations run, so `AppState`
  is assembled in `main` (not a bare `from_env`) once the setup exists.
- Auth cookies (session, and the ephemeral passkey / password / bootstrap ceremony
  cookies) are all built through `backend/src/cookies.rs`, so their security
  attributes are defined once. Session establishment is shared via
  `session::establish`, used by every method's success path.

## API surface (implemented)

Discovery and first-run:

- `GET /api/v1/auth/context` -> `{ claimed }`.
- `POST /api/v1/auth/methods` `{ identifier }` -> `{ methods: [...] }`. The permitted
  methods from the applicable org policy; non-enumerating (an unknown identifier gets
  the default org's policy, never an error).
- `POST /api/v1/auth/bootstrap/begin` `{ org_name, email, display_name, method,
  opaque_registration_request? }` -> passkey challenge, or OPAQUE registration
  response + TOTP provisioning URI (first-run only; 409 once claimed).
- `POST /api/v1/auth/bootstrap/finish` (passkey credential, or
  `{ opaque_registration_upload, totp_code }`) -> claims atomically, signs in.

Sign-in:

- `POST /api/v1/auth/passkey/authenticate/begin` `{ email }` -> auth challenge.
- `POST /api/v1/auth/passkey/authenticate/finish` (assertion) -> verifies, signs in.
- `POST /api/v1/auth/password/authenticate/begin` `{ identifier, ke1 }` -> `{ ke2 }`
  (always answers; `429` when rate-limited).
- `POST /api/v1/auth/password/authenticate/finish` `{ ke3 }` -> `204` + session, or
  `401 { reason: "totp_required" }`.
- `POST /api/v1/auth/totp/verify` `{ code }` -> `204` + session.
- `POST /api/v1/auth/recovery/authenticate` `{ email, code }` -> `204` + session
  (single-use; generic `401` on unknown email or wrong code).
- `POST /api/v1/auth/logout` -> ends the session.
- `GET /api/v1/me` -> the session principal (session probe).

Invites and add-credential (see `docs/auth.md` for the full flow):

- `POST /api/v1/auth/invites` (authed) -> mints a link token + acceptance URL.
- `GET /api/v1/auth/invites/:token` -> preview (org/team/role); generic `404`.
- `POST /api/v1/auth/invites/:token/accept/begin` + `/accept/finish` -> enroll a
  credential; atomically provisions the member + recovery codes and consumes the invite.
- `POST /api/v1/credentials/enroll/begin` + `/finish` (authed) -> add a passkey or
  set/change the password for the signed-in user.
- `POST /api/v1/auth/invites/:token/accept/join` (authed) -> an existing profile
  joins the invite's org (multi-org), no enrollment.

Profiles, orgs, and sessions (see `docs/identity.md`):

- `POST /api/v1/orgs` (authed) -> create a new org (creator becomes owner).
- `GET /api/v1/session/accounts` -> the accounts in this session + the active
  account's orgs + `active_org_id`.
- `POST /api/v1/session/switch` `{ user_id }` -> make another signed-in account active.
- `POST /api/v1/session/switch-org` `{ org_id }` -> change the active account's org.
- `POST /api/v1/session/signout` `{ user_id? }` -> sign one account out (default active).

Enrollment (claim, invite acceptance, add-credential) shares `backend/src/enrollment.rs`,
and both claim and invite acceptance issue one-time recovery codes. Every sign-in
attempt is recorded in `login_events`.

## Roadmap

1. TOTP enrollment for the passkey path so a passkey account can add a second factor;
   regenerating recovery codes from the Security Center.
2. Passwordless email sign-in (magic link / one-time code), once the SMTP service
   lands; the design is in "Passwordless email sign-in" above.
3. Per-org SSO / custom OAuth brokering (`openidconnect`), configured in Postgres.
4. Security Center surfaces: login history, listing/removing credentials, active-session
   revocation. Remaining org-policy enforcement: force TOTP enrollment for a member of a
   `require_totp` org who has none, and honor `session_lifetime_seconds`. (Password
   sign-in already refuses when no org the profile belongs to permits passwords, and an
   enrolled TOTP is always required; see `password::authenticate_begin`.)
