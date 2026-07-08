# Authentication model (authoritative)

This document states Elysium's authentication architecture. Where it conflicts with
`auth.md` (the earlier Keycloakify / redirect-BFF design), **this document wins**:
the browser never contacts Keycloak, and the API brokers everything.

## The one rule

**The browser only ever talks to Elysium's own SPA and API. Only the API ever
touches Keycloak, server-side.** There is no redirect to Keycloak, no Keycloak-served
page (themed or not), and no Keycloak origin in the browser. Our React renders every
input: the email field, the passkey button, the TOTP field, recovery.

## Roles

- **SPA (React).** Renders the entire auth UI and drives the browser-native passkey
  ceremony via `navigator.credentials`. Holds only the session cookie.
- **API (Rust).** Is the **WebAuthn Relying Party** (`webauthn-rs`): it issues
  challenges, verifies assertions, and owns the passkey / TOTP / recovery logic and
  the sessions. It is the only component that would ever talk to Keycloak.
- **Postgres (app DB).** The **system of record** for auth: users, their passkeys,
  (future) TOTP secrets and recovery codes, and sessions all live here.
- **Keycloak.** Retained in the deployment but **outside the auth path** (dormant
  for now): kept for future federation / IdP brokering (SSO) and OIDC token issuance
  to the service/worker layer, which only the API would use, server-side. It is not
  the WebAuthn Relying Party and never runs the browser login. (If you would rather
  Keycloak be the canonical user directory synced from the API, say so; it is a
  small additive layer on top of this.)

## Passwordless sign-in (passkey)

1. SPA: user enters their email, taps **Sign in with a passkey**.
2. SPA → `POST /api/v1/auth/passkey/authenticate/begin { email }`. The API looks the
   user up (Postgres), builds a WebAuthn authentication challenge for that user's
   registered credentials, stashes the challenge in a short-lived cookie, and returns
   the public-key request options.
3. SPA → `navigator.credentials.get({ publicKey })`. The browser shows the native
   passkey prompt (biometric / PIN). No webpage, no redirect.
4. SPA → `POST /api/v1/auth/passkey/authenticate/finish { assertion }`. The API
   verifies the assertion against the stored credential and challenge, and on success
   creates a session and sets the HttpOnly session cookie.
5. SPA re-resolves the session (`GET /api/v1/me`) and routes into the app.

## Passkey enrollment (onboarding / recovery)

Enrollment (root claim, invite acceptance, or adding a passkey) is the same shape
with `register/begin` + `register/finish` and `navigator.credentials.create()`. The
API stores the new credential in Postgres. Enrollment always issues recovery codes.

## TOTP and recovery

TOTP is a fallback factor verified by the API (a vetted TOTP library) against a
secret stored in our DB; enrollment shows a QR the SPA renders. Recovery codes are
one-time codes the API issues at enrollment and consumes on a lost-device sign-in,
which then forces a fresh passkey enrollment.

## Sessions

Unchanged from the current implementation: an opaque HttpOnly, Secure, SameSite=Lax
session cookie backed by the `sessions` table; the `SessionUser` extractor protects
every `/api/v1/*` route and the SSE stream. Same-origin (the SPA proxies `/api` to
the API), so the cookie is first-party.

## WebAuthn configuration

- **RP ID** = the app's registrable domain (`localhost` in dev, the deployment
  domain in production). **RP origin** = the app origin (`http://localhost:5173` in
  dev). Because the API and SPA share one origin, the RP origin the API validates
  matches the origin the browser signs against.
- Credentials are discoverable (resident keys) with user verification required, so a
  passkey is a true passwordless factor.

## What is retired

The redirect-based BFF is removed entirely: `oidc.rs` (the OIDC Authorization Code
+ PKCE code exchange and the `/auth/login` + `/auth/callback` redirects) and `auth.rs`
(Keycloak JWKS / JWT validation, which only served that flow) are deleted, along with
their dependencies (`jsonwebtoken`, `reqwest`). Keycloak stays in the deployment but
its realm login flow is dormant; the API talks to it only if and when the federation
or service-token features are built.

## Build notes

- The RP uses `webauthn-rs` 0.5 (`WebauthnBuilder::new(rp_id, rp_origin)`), verified
  to build on Linux. It pulls in native OpenSSL (`openssl-sys`), unlike the rest of
  the backend which is rustls: the backend image already has `libssl-dev`, and CI
  installs `openssl-devel` on the Fedora runner. RP id = the app host (`localhost`
  in dev), RP origin = the app origin (`http://localhost:5173`).
- The registration/authentication ceremony state (the `webauthn-rs`
  `PasskeyRegistration` / `PasskeyAuthentication`, behind the
  `danger-allow-state-serialisation` feature) is carried between begin and finish
  in a short-lived HttpOnly cookie, so no extra server-side state table is needed.
  Credentials (the `Passkey`) persist in Postgres, linked to the user.
- The SPA uses `@simplewebauthn/browser` (`startRegistration` / `startAuthentication`)
  to convert the challenge JSON to/from the `ArrayBuffer` shapes `navigator.
  credentials` needs, rather than hand-rolling base64url conversion.

## API surface (implemented)

- `GET /api/v1/auth/context` -> `{ claimed }`.
- `POST /api/v1/auth/bootstrap/begin` `{ email, display_name }` -> registration
  challenge (first-run only; 409 once claimed).
- `POST /api/v1/auth/bootstrap/finish` (credential) -> stores the passkey, signs in.
- `POST /api/v1/auth/passkey/authenticate/begin` `{ email }` -> auth challenge.
- `POST /api/v1/auth/passkey/authenticate/finish` (assertion) -> verifies, signs in.
- `POST /api/v1/auth/logout` -> ends the session.
- `GET /api/v1/me` -> the session principal (session probe).

## Roadmap

1. WebAuthn RP in the API (`webauthn-rs`), credential + challenge storage, the
   authenticate begin/finish endpoints, and the SPA passkey sign-in against them.
2. Enrollment for additional passkeys + recovery codes, wired to onboarding and
   invite acceptance (the first-run root claim already enrolls the first passkey).
3. TOTP fallback (enroll + verify) in the API and SPA.
4. (Optional) Keycloak for federation / IdP brokering (SSO) and service/worker token
   issuance, synced from the Postgres user store, all server-side.
