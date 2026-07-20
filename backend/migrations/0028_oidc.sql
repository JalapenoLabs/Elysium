-- Identity: the OIDC/OAuth2 provider core (Authorization Code + PKCE, JWKS).
--
-- Elysium already owns identity deeply (OPAQUE, passkeys, TOTP, orgs, sessions),
-- so rather than bolt on a separate IdP it is extended to be an OIDC provider that
-- every managed service (Forgejo, Harbor, ...) SSOs against. This is the
-- interactive/web half; transport auth (SSH keys, scoped tokens) is migration 0027.
-- See docs/oidc.md and docs/integrations.md (Identity: one login).
--
-- Three concerns, three-plus tables:
--   * signing keys      -- the RS256 keypairs that sign tokens; published via JWKS
--   * clients           -- the registered relying parties (managed services)
--   * authorization codes / refresh tokens -- the short-lived grant artifacts

-- The asymmetric keys that sign ID and access tokens. RS256 today; the algorithm
-- column leaves room for ES256 later without a migration. Exactly one key is
-- active and signs; JWKS publishes every non-retired public key so a token signed
-- by a just-rotated-out key still verifies. Rotation makes a new key active, marks
-- the prior one inactive-but-not-retired (still verifies), and retires it later.
CREATE TABLE oidc_signing_keys (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The JWK "kid"; the token header carries it so a verifier picks the right key.
    kid         TEXT NOT NULL UNIQUE,
    algorithm   TEXT NOT NULL DEFAULT 'RS256' CHECK (algorithm IN ('RS256')),
    -- crypto.rs-sealed PKCS#8 private key PEM. Sealed, not hashed: signing needs the
    -- recoverable private half, so a database dump alone (without the master key)
    -- yields no usable signing key.
    private_key BYTEA NOT NULL,
    -- The public half as a JWK object (kty/use/alg/kid/n/e), served verbatim by JWKS.
    public_jwk  JSONB NOT NULL,
    -- The one key that signs new tokens. The partial unique index below enforces
    -- "at most one active" at the database level.
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- When set, the key drops out of JWKS and can no longer verify; NULL while live.
    retired_at  TIMESTAMPTZ
);

-- At most one active signing key. A partial unique index over the constant `true`
-- rejects a second active row; rotation flips the old key inactive before inserting
-- the new active one, all in one transaction.
CREATE UNIQUE INDEX oidc_signing_keys_single_active_idx
    ON oidc_signing_keys (is_active) WHERE is_active;

-- A registered OIDC relying party. Managed services are registered as clients at
-- provisioning time (#38), their credentials injected into the container config.
CREATE TABLE oidc_clients (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The public client identifier, presented on every authorize/token request.
    client_id      TEXT NOT NULL UNIQUE,
    -- crypto.rs-sealed client secret for confidential clients; NULL for public
    -- (PKCE-only) clients. Sealed, not hashed: the managed service needs the
    -- plaintext injected into its config (#31 templating), so it must be
    -- recoverable, unlike a bearer token which is only ever compared.
    client_secret  BYTEA,
    name           TEXT NOT NULL,
    -- The exact redirect URIs a code may be returned to; matched exactly, never
    -- by prefix, so a code never rides to an unvalidated destination.
    redirect_uris  TEXT[] NOT NULL,
    -- The scopes and grant types the client may request.
    scopes         TEXT[] NOT NULL,
    grant_types    TEXT[] NOT NULL,
    -- The managed integration this client fronts, when provisioned for one; the
    -- client is torn down with its service.
    integration_id UUID REFERENCES integrations (id) ON DELETE CASCADE,
    -- First-party (managed) clients auto-approve: the user already holds an Elysium
    -- session, so opening the service is a pre-authenticated view with no consent
    -- screen. A consent screen for third-party clients is a later addition.
    is_first_party BOOLEAN NOT NULL DEFAULT FALSE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A single-use authorization code minted by the authorize endpoint and redeemed at
-- the token endpoint. Only a hash of the code is stored (like a transport token):
-- the code is a bearer credential, only ever compared, so a dump yields nothing.
-- The PKCE challenge binds the code to the client instance that requested it.
CREATE TABLE oidc_authorization_codes (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Lowercase-hex SHA-256 of the code; the token endpoint hashes-then-finds.
    code_hash             TEXT NOT NULL UNIQUE,
    client_id             TEXT NOT NULL REFERENCES oidc_clients (client_id) ON DELETE CASCADE,
    -- The account the code authenticates, and the org context it was issued in.
    user_id               UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    org_id                UUID REFERENCES orgs (id) ON DELETE SET NULL,
    -- The redirect URI the code was bound to; the token request must present the same.
    redirect_uri          TEXT NOT NULL,
    scopes                TEXT[] NOT NULL,
    -- PKCE: the token request must present a verifier hashing to this challenge.
    code_challenge        TEXT NOT NULL,
    code_challenge_method TEXT NOT NULL DEFAULT 'S256' CHECK (code_challenge_method IN ('S256')),
    -- The OIDC nonce to echo into the ID token, if the client sent one.
    nonce                 TEXT,
    -- Short expiry (~60s): a code is redeemed immediately after the redirect.
    expires_at            TIMESTAMPTZ NOT NULL,
    -- Stamped on redemption; a second redemption of the same code is refused.
    consumed_at           TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Reap expired/consumed codes efficiently; the hash lookup rides the UNIQUE index.
CREATE INDEX oidc_authorization_codes_expires_at_idx
    ON oidc_authorization_codes (expires_at);

-- A refresh token issued alongside an access token when `offline_access` is granted.
-- Opaque and hashed at rest (only ever compared), it re-issues access tokens without
-- another authorize round-trip. Rotated on use: redemption consumes the presented
-- token and issues a fresh one, so a captured refresh token is single-use.
CREATE TABLE oidc_refresh_tokens (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token_hash  TEXT NOT NULL UNIQUE,
    client_id   TEXT NOT NULL REFERENCES oidc_clients (client_id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    org_id      UUID REFERENCES orgs (id) ON DELETE SET NULL,
    scopes      TEXT[] NOT NULL,
    nonce       TEXT,
    -- Absolute expiry; NULL means it does not expire on its own (still revocable).
    expires_at  TIMESTAMPTZ,
    -- Stamped when rotated out; a consumed token never re-issues.
    consumed_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX oidc_refresh_tokens_user_id_idx ON oidc_refresh_tokens (user_id);
