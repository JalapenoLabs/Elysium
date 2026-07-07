# Identity & Authentication

Elysium does not roll its own authentication. **Keycloak** owns every credential and runs every
login ceremony; the application never sees or stores a password. Elysium owns only profile and
membership data, linked back to Keycloak by the user's subject (`sub`).

## Principles

- **Passwordless from day 0.** No password is ever stored. Users authenticate with a passkey
  (WebAuthn passwordless) or a TOTP code.
- **2FA from day 0.** WebAuthn passkeys and TOTP (authenticator app / QR) are first-class.
- **Keycloak owns security.** All credential storage, MFA enrollment, and login flows live in
  Keycloak. We never reimplement them.
- **Our own React UI.** The login/register/2FA screens are Elysium's own React, delivered as a
  Keycloak theme via Keycloakify (Phase C), not Keycloak's stock pages and not a headless flow.

## Deployment

Two services in `compose.yaml`:

- **`keycloak`** (`quay.io/keycloak/keycloak:26.3.2`) on port `8080`. Runs `start-dev
  --import-realm` in development; production runs `start` behind the operator's TLS and
  hostname (their responsibility). Health and metrics are exposed on the management port
  `9000` (`KC_HEALTH_ENABLED=true`).
- **`keycloak-database`** a dedicated Postgres, isolated from the application database so
  identity data and app data never share a store.

Configuration is driven by `.env` (`KEYCLOAK_*`). The bootstrap admin (`KEYCLOAK_ADMIN`) governs
the Keycloak master realm and admin console only; it is not an application user.

## The `elysium` realm

Defined in `keycloak/realm-export.json`, imported on first boot:

- **Clients**: `elysium-frontend`, a public SPA client using Authorization Code + PKCE (S256),
  redirecting to `http://localhost:5173/*` in development. (The backend validates tokens via the
  realm's JWKS and does not need its own client yet; an audience mapper arrives in Phase C.)
- **WebAuthn passwordless policy**: resident key required and user verification required, so
  registered credentials are true passkeys (discoverable + user-verifying).
- **OTP policy**: standard TOTP (SHA1, 6 digits, 30s), compatible with common authenticator apps.
- **Passwordless browser flow** (`browser-passwordless`, set as the realm `browserFlow`):
  1. `auth-cookie` / `identity-provider-redirector` (alternatives, for existing sessions / IdP)
  2. otherwise the forms subflow: `auth-username-form` (required), then a passwordless factor
     subflow offering **passkey** (`webauthn-authenticator-passwordless`) or **TOTP**
     (`auth-otp-form`) as alternatives.

There is no password authenticator anywhere in the flow.

## Token validation (backend)

The API issues no tokens of its own. Clients present a Keycloak bearer token, and the Axum
middleware in `backend/src/auth.rs` validates it: RS256 signature against the realm's JWKS,
issuer against the configured realm URL, and expiry. The JWKS is fetched lazily and cached,
refetched when a token references an unknown key id (rotation), so the API can start before
Keycloak is reachable. Any handler that takes the `AuthenticatedUser` extractor is protected;
unauthenticated requests get `401` before the handler runs. `GET /api/v1/me` is the first such
route, returning the principal (`elysium.auth.v1.MeResponse`) from the token claims.

Issuer consistency is handled by pinning Keycloak's base URL (`KC_HOSTNAME`) so every token
declares the same `iss` (the browser-facing `http://localhost:8080/realms/elysium`) regardless
of whether it was requested from the browser or another container. The backend trusts that
issuer (`KEYCLOAK_ISSUER`) while fetching the JWKS over the internal network
(`KEYCLOAK_JWKS_URL`), which is why the two URLs differ.

Audience is not yet enforced; a resource-server audience mapper is a planned hardening step.

## Identity schema (database)

`backend/migrations/0001_identity.sql` creates the application's view of identity, applied at
startup via `sqlx::migrate!`:

- `orgs` (tenants), `teams` (groups within an org), `users` (keyed by `keycloak_sub`, one org
  per user), and `team_members` (user-team edges with a role).
- Roles (`member`/`admin`/`owner`) and status (`online`/`offline`/`away`/`out_of_office`) are
  text guarded by CHECK constraints, mirroring the `elysium.user.v1` proto enums.
- A trigger keeps `users.updated_at` current.

Just-in-time provisioning (upserting a `users` row on a user's first authenticated request,
including org assignment) is deferred until the onboarding flow is designed.

## First-login enrollment (open item)

Passwordless onboarding has a bootstrapping nuance: a brand-new user has no passkey or TOTP yet,
so they cannot pass the passwordless factor on their very first login. This is resolved out of
band, by an admin-initiated "set up credential" action email or a dedicated onboarding path. That
onboarding UX is deferred to the Keycloakify work (Phase C) and the account/admin surfaces; it is
intentionally not solved by the realm import alone.

## Roadmap

- **Phase C (remaining)**: Keycloakify React theme for login/register/2FA, so the login screens
  are Elysium's own React rather than Keycloak's stock pages.
- **Phase D**: frontend OIDC login (PKCE) wired to the protected API, and route guards.
- Just-in-time user provisioning and admin-initiated passwordless enrollment emails.
- A permission model over teams, and audience enforcement on the API.
- Organization-, team-, and user-provided Keycloak identity providers / brokering.
