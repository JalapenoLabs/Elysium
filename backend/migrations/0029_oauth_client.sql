-- Identity: Elysium as an OAuth *client* for linked services (GitHub/Google/MS).
--
-- This is the mirror image of the OIDC provider (migration 0028). There Elysium
-- is the identity authority a managed service logs in against; here Elysium holds
-- a user's grant to an *external* provider so a member can link their own GitHub,
-- Google, or Microsoft account and Elysium (and Elysia) can act on their behalf,
-- silently refreshing the grant so the user is never re-prompted for API access.
-- See docs/integrations.md (Identity: linked services invert the trust direction).
--
-- Three tables, three concerns:
--   * provider configs  -- the admin-supplied OAuth *app* credentials, per provider
--   * states            -- short-lived, single-use CSRF/PKCE state for a connect flow
--   * connected accounts -- the linked accounts, with sealed access/refresh tokens

-- The OAuth application credentials an admin registers for a provider. Provider
-- endpoints themselves are hardcoded (stable well-known URLs); only the per-
-- deployment app credentials live here. Setting a row (with `enabled`) is what
-- flips a provider `configured` and lets members connect it.
CREATE TABLE oauth_provider_configs (
    -- The provider slug: 'github', 'google', or 'microsoft'.
    provider      TEXT PRIMARY KEY,
    -- The OAuth app's public client id, presented on every authorize/token call.
    client_id     TEXT NOT NULL,
    -- crypto.rs-sealed client secret. Sealed, not hashed: the token exchange needs
    -- the plaintext to authenticate to the provider, so a dump alone (without the
    -- master key) yields nothing usable, but Elysium can still recover it.
    client_secret BYTEA NOT NULL,
    -- An admin toggle: a configured-but-disabled provider is not offered to members.
    enabled       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The pending state of an in-flight connect flow, bound to the user who started
-- it. The `state` parameter that rides to the provider and back is stored only as
-- a hash (a bearer value, only ever compared), exactly like an authorization code.
-- Single-use (consumed by an atomic DELETE ... RETURNING) and short-lived, so a
-- captured or replayed state is refused.
CREATE TABLE oauth_states (
    -- Lowercase-hex SHA-256 of the opaque `state`; the callback hashes-then-finds.
    state_hash     TEXT PRIMARY KEY,
    -- The account that initiated the connect; the callback re-associates by this,
    -- since it carries no guaranteed session cookie of its own.
    user_id        UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider       TEXT NOT NULL,
    -- The PKCE code_verifier for providers that support PKCE; NULL for plain OAuth2
    -- (GitHub). The callback presents it at the token exchange.
    pkce_verifier  TEXT,
    -- The app-origin SPA path to return to after the callback (a fixed settings
    -- route, never attacker-controllable); only `?connected=`/`?oauth_error=` differ.
    redirect_after TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Short expiry (~10 min): a connect is completed immediately after consent.
    expires_at     TIMESTAMPTZ NOT NULL
);

-- Reap the initiating user's states on account deletion, and sweep expired ones.
CREATE INDEX oauth_states_user_id_idx ON oauth_states (user_id);
CREATE INDEX oauth_states_expires_at_idx ON oauth_states (expires_at);

-- A linked external account: one row per (user, provider, external identity), so a
-- user may link several accounts of the same provider (distinct external_id). The
-- access and refresh tokens are crypto.rs-sealed at rest (recoverable, since the
-- app must present them to the provider), never returned to the client or logged.
CREATE TABLE connected_accounts (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider          TEXT NOT NULL,
    -- The provider's stable, immutable account id (GitHub numeric id, OIDC `sub`).
    external_id       TEXT NOT NULL,
    -- Display fields refreshed from the provider on every (re)link; may be blank.
    external_username TEXT NOT NULL DEFAULT '',
    external_email    TEXT NOT NULL DEFAULT '',
    avatar_url        TEXT NOT NULL DEFAULT '',
    -- The scopes the grant actually carries (as reported by the token response).
    scopes            TEXT[] NOT NULL DEFAULT '{}',
    -- Sealed access token, and the sealed refresh token when the provider issued
    -- one (GitHub OAuth-app tokens often have neither expiry nor refresh).
    access_token      BYTEA NOT NULL,
    refresh_token     BYTEA,
    -- When the access token expires, when known; NULL means it does not expire.
    token_expires_at  TIMESTAMPTZ,
    -- Set when a silent refresh has genuinely failed, so the surfaces raise a single
    -- clear re-consent prompt rather than retrying a dead grant.
    needs_reconsent   BOOLEAN NOT NULL DEFAULT FALSE,
    connected_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Re-linking the same external account updates the row rather than duplicating.
    UNIQUE (user_id, provider, external_id)
);

CREATE INDEX connected_accounts_user_id_idx ON connected_accounts (user_id);
