# OAuth client (linked services)

Elysium is the OAuth **client** for linked external services (GitHub, Google, Microsoft).
This is the mirror image of the OIDC provider (`docs/oidc.md`): there Elysium is the identity
authority a managed service logs in against; here Elysium **holds a user's grant** to an
external provider so a member can link their own account, and Elysium (and Elysia) act on their
behalf, silently refreshing the grant so the user is never re-prompted for API access. See
`docs/integrations.md` ("Linked services invert the trust direction").

The implementation lives in `backend/src/oauth_client/`, tables in migration `0029_oauth_client.sql`.

## Flow

Standard Authorization Code flow, with PKCE (S256) for the providers that support it:

1. An admin registers a provider's OAuth-app credentials (`client_id` + `client_secret`) and
   enables it. That is what flips the provider `configured` for members.
2. A member starts a connect; Elysium mints a single-use `state` (and a PKCE verifier where
   supported) bound to that member, persists it, and returns the provider's authorize URL for
   a full-page redirect.
3. The provider redirects back to Elysium's callback with `code` + `state`. Elysium consumes
   the state single-use (this is what authenticates the callback, which carries no session
   cookie guarantee), exchanges the code for tokens, fetches the external profile, and links
   the account with **sealed** tokens.
4. The callback 302s back to the SPA settings route (`/settings`) with `?connected=<provider>`
   on success or `?oauth_error=<reason>` on failure.

## Providers (v1)

Provider **endpoints** are hardcoded (stable well-known URLs); only the app **credentials** are
per-deployment. The per-provider table lives in `backend/src/oauth_client/providers.rs`.

| Provider | Kind | PKCE | Default scopes | Refresh token via | Profile source |
|---|---|---|---|---|---|
| `github` | OAuth2 | no | `read:user user:email` | (none; tokens don't expire) | `/user` + `/user/emails` (primary verified) |
| `google` | OIDC | yes | `openid email profile` | `access_type=offline` + `prompt=consent` | OIDC `userinfo` (`sub`/`email`/`name`/`picture`) |
| `microsoft` | OIDC | yes | `openid email profile offline_access` | `offline_access` scope | OIDC `userinfo` (`sub`/`email`\|`preferred_username`) |

## Endpoints

| Method + path | Auth | Purpose |
|---|---|---|
| `GET /api/v1/oauth/providers` | session cookie | List providers + `configured` (credentials set **and** enabled). |
| `GET /api/v1/me/connected-accounts` | session cookie | The caller's linked accounts. **Never** tokens. |
| `POST /api/v1/oauth/connect/{provider}` | session cookie | Mint state/PKCE, return `{ authorize_url }`. |
| `GET /api/v1/oauth/callback/{provider}` | single-use `state` | Exchange code, link account, 302 to settings. |
| `DELETE /api/v1/me/connected-accounts/{id}` | session cookie | Best-effort upstream revoke, then forget the row (`404` if not the caller's). |
| `GET /api/v1/oauth/providers/{provider}` | `manage_integrations` | Admin echo of `client_id` + `enabled` (never the secret). |
| `PUT /api/v1/oauth/providers/{provider}` | `manage_integrations` | Set the OAuth-app credentials (secret sealed) and the enabled flag. |

The OAuth **redirect URI** each provider's app must register is
`<app_origin>/api/v1/oauth/callback/<provider>` (the SPA proxies `/api` to the backend).

## Tokens at rest, and silent refresh

Access and refresh tokens are **sealed** with `crypto.rs` (recoverable, since Elysium must
present them to the provider), bound to the connected account's identity
(`user_id:provider:external_id`) so a dump can neither open a token nor shuffle one between
accounts or users. This inverts the transport-token trade-off (`docs/transport.md`), where a
bearer token is only ever compared and so is hashed.

`resolve_connected_token(pool, crypto, connected_account_id) -> Option<access_token>` is the
seam the integration surfaces (and Elysia) call. It returns the current token when fresh,
silently refreshes an expiring one (within a 60s skew) via the refresh token and re-seals the
rotated tokens, and sets `needs_reconsent` (returning `None`) only when a refresh genuinely
fails or the grant already needs re-consent. `None` therefore means exactly "the user must
re-connect this account", which the connected-accounts list surfaces via `needs_reconsent`.

## Owner scope

A linked account is a **personal** grant (the member's own account). Whether members may link a
given provider kind is gated by the org's per-kind integration policy (`docs/integrations.md`);
admin provider configuration is gated by `manage_integrations`.
