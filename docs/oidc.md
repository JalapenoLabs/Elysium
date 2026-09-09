# OIDC / OAuth2 provider

Elysium is its own OpenID Connect provider. Every managed service (Forgejo, Harbor, ...)
SSOs against Elysium as an OIDC client rather than against a bolted-on Keycloak, because
Elysium already owns identity deeply (OPAQUE, passkeys, TOTP, orgs, sessions). This is the
**interactive / web** half of "Identity: one login" ([integrations.md](integrations.md)); transport auth
(SSH keys, scoped tokens) is [transport.md](transport.md).

Anubis owns identity, so this provider is built on the framework's users, sessions, and
organizations rather than beside them.
The identity source of truth is always Elysium's own tables; the provider is a thin,
spec-compliant surface over them.

## Flow

Standard **Authorization Code flow with PKCE (S256 required)**. A client:

1. reads `/.well-known/openid-configuration` and `/.well-known/jwks.json`,
2. redirects the user to `/api/v1/oidc/authorize`,
3. exchanges the returned code at `/api/v1/oidc/token` for an ID token, an access token,
   and (with `offline_access`) a refresh token,
4. optionally reads `/api/v1/oidc/userinfo` with the access token.

## Endpoints

| Method + path | Auth | Purpose |
|---|---|---|
| `GET /.well-known/openid-configuration` | public | Provider metadata; `issuer` = `app_origin`. |
| `GET /.well-known/jwks.json` | public | Public signing keys (every non-retired key). |
| `GET /api/v1/oidc/authorize` | session cookie (bridged) | Validates client + exact redirect URI, then issues a code. |
| `POST /api/v1/oidc/token` | client (Basic or `client_secret_post`) | `authorization_code` and `refresh_token` grants. |
| `GET /api/v1/oidc/userinfo` | Bearer access token | Fresh claims for the token's subject. |
| `GET`/`POST /api/v1/oidc/clients` | session + `manage_integrations` | Admin list / register a client. |

The two discovery documents live at the site root (`/.well-known/*`), **not** under `/api`.

## Silent SSO (session bridge)

`authorize` reads the Elysium session cookie directly (not the `SessionUser` extractor, which
would `401`). A signed-in user whose auth policies pass gets a `302` straight back to the
client with a code, **no second prompt**. An anonymous user is redirected to the SPA login
with `return_to` set to the full authorize URL; after signing in the SPA replays the request
and the second pass finds a live session. The org context is the session's active org.

## Auth policies enforced before issuance

Before any code or token is issued, `authorize` enforces the same checks the rest of the app
uses, never re-derived:

- **Suspension + org membership** via `authz::role_in_org` (a suspended or non-member caller
  has no effective role in the active org).
- **Mandatory 2FA** via `context::mandatory_totp_missing` (a member of a `require_totp` org
  with no confirmed TOTP is refused with `access_denied`).

## Tokens and claims

ID and access tokens are RS256 JWTs signed with the active key; the JWT `kid` header selects
the verifying JWK. The ID token is short-lived (5 min) and echoes `nonce`; the access token
lives 1 hour. Refresh tokens are opaque, hashed at rest, and single-use (rotated on redeem).
Authorization codes are opaque, hashed at rest, single-use, and expire in 60 seconds.

Claim set (gated by requested scope):

| Claim | Source | Notes |
|---|---|---|
| `iss` | `app_origin` | Same for every token. |
| `sub` | `users.id` (UUID string) | Stable per profile. |
| `aud` | client id | |
| `iat` / `exp` | issue time / TTL | |
| `nonce` | request | ID token only. |
| `email`, `email_verified` | `users.email` | `email` scope. `email_verified` is `true`: email is the global profile key and source of auth truth. |
| `name`, `given_name`, `family_name` | `users` | `profile` scope. |
| `org` | active org | `{ id, slug, role }`; `profile` scope. |
| `roles` | org role | e.g. `["admin"]`; the axis a service maps to its admin role. |
| `groups` | team slugs in the active org | the axis a service maps to its teams. |

**Downstream mapping (Forgejo et al.):** map `groups` (team slugs) to service teams and
`roles` / `org.role` (org role) to service admin. `sub` is the stable account key.

## Signing keys and rotation

RSA-2048 / RS256, one row per key in `oidc_signing_keys`. The private PEM is sealed with
`crypto.rs` (a database dump alone yields no signing key); the public JWK is stored and served
verbatim. Exactly one key is **active** (signs new tokens); JWKS publishes every **non-retired**
key. An initial key is generated on boot if none is active (idempotent, mirroring the OPAQUE
setup). Rotation generates a new active key and demotes the prior one to inactive-but-published
(still verifies); retiring it later drops it from JWKS. The rotation function exists; an admin
"rotate now" endpoint is a later addition.

## Clients

Registered in `oidc_clients`. A confidential client's secret is **sealed** (not hashed) with
`crypto.rs`, because the managed service needs the plaintext injected into its own config; a
public client carries no secret and proves itself with PKCE alone. Redirect URIs are matched
**exactly** (never by prefix). First-party (managed) clients auto-approve with no consent
screen; a consent screen for third-party clients is a later addition. `create_client` /
`get_client` are the internal seams provisioning (the git vertical) calls.

## What consumers must do

- **The git vertical (Forgejo provisioning)** registers a first-party confidential client via
  `oidc::clients::create_client` (redirect URIs = the service's callback, scopes
  `openid profile email`, grants `authorization_code refresh_token`), injects the returned
  `client_id` / `client_secret` into the service config, and maps `groups` / `roles` to its
  own teams and admin role.
- **The SPA dev server** must proxy `/.well-known/*` to the backend in addition to `/api` and
  `/auth` (the discovery documents are same-origin at the site root). `/api/v1/oidc/*` is
  already covered by the existing `/api` proxy. No consent UI is needed for first-party
  clients; the existing `/login` route already honors `return_to` for the silent-SSO bounce.
