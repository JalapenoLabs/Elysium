-- Identity: transport credentials, the SSH keys and scoped access tokens an
-- account authenticates transport with (git over SSH/HTTPS, `docker login`, SMTP
-- AUTH). See docs/integrations.md (Identity, transport auth) and src/transport.rs.
--
-- These are not OIDC: a client presents a credential, and Elysium resolves it to
-- the owning account. Both credential kinds are secrets shown exactly once at
-- creation and never retrievable again: storage keeps only the SSH public half
-- and a one-way hash of each token. Revocation deletes the row, so transport auth,
-- which checks live state, fails at once.

-- One SSH public key mapped to an account. Elysium resolves an inbound key to its
-- owner from this registry (M2-05 / #29 / #38). The private half is either the
-- user's own (never sent to us) or generated once by Elysium and returned once.
CREATE TABLE ssh_keys (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The owning account. A key belongs to one profile and cascades with it.
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- SHA256 fingerprint ("SHA256:..."), always derived from the public key by the
    -- server, never trusted from the client. It is what transport auth matches on.
    fingerprint  TEXT NOT NULL,
    -- The OpenSSH public key line ("ssh-ed25519 AAAA... comment").
    public_key   TEXT NOT NULL,
    -- Key algorithm string, e.g. "ssh-ed25519".
    algorithm    TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Stamped by the transport-auth path when this key authenticates; NULL until then.
    last_used_at TIMESTAMPTZ,
    -- A key registers once per account; the fingerprint identifies it.
    UNIQUE (user_id, fingerprint)
);

-- Fingerprint -> user resolution for transport auth: an inbound key's SHA256
-- fingerprint looks up its owner here.
CREATE INDEX ssh_keys_fingerprint_idx ON ssh_keys (fingerprint);

-- One scoped access token: a bearer credential for HTTPS git, `docker login`,
-- SMTP AUTH, or a service API. Only a hash is stored (a bearer token is only ever
-- compared, so a one-way hash is sufficient and strictly safer than a recoverable
-- seal: a database dump yields no usable token even with the master key). The
-- plaintext ("ely_pat_<random>") is shown once at mint.
CREATE TABLE access_tokens (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    -- Lowercase-hex SHA-256 of the plaintext secret. UNIQUE both prevents a
    -- collision and, being backed by a btree index, is the lookup path the
    -- transport-auth verifier hashes-then-finds against.
    token_hash     TEXT NOT NULL UNIQUE,
    -- The elysium.security.v1.TokenScope discriminants this token grants (see
    -- src/transport.rs); the 0 UNSPECIFIED sentinel is never stored.
    scopes         SMALLINT[] NOT NULL,
    -- Optional bind to a single integration instance; NULL grants every instance
    -- of the scoped kinds within the account's access. Cascades on integration
    -- delete so a token never outlives (and cannot broaden past) its bound service.
    integration_id UUID REFERENCES integrations (id) ON DELETE CASCADE,
    -- Absolute expiry; NULL means the token never expires (ttl_seconds = 0).
    expires_at     TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Stamped by the transport-auth path when this token authenticates; NULL until then.
    last_used_at   TIMESTAMPTZ
);

-- List an account's tokens efficiently; token_hash lookups ride the UNIQUE index.
CREATE INDEX access_tokens_user_id_idx ON access_tokens (user_id);
