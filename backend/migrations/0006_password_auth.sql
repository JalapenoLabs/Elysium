-- OPAQUE password auth and TOTP second factor.
--
-- Passwords use the OPAQUE aPAKE (RFC 9807): the server stores an opaque
-- registration record and never sees the password or any replayable hash of it
-- (docs/auth-model.md). The server's OPAQUE setup (its OPRF seed + keypair) is a
-- process-wide singleton that must be stable across restarts, so it is persisted
-- here rather than regenerated on boot.

-- The server-side OPAQUE setup. Exactly one row (id = TRUE), created on first
-- boot and read on every subsequent boot; regenerating it would invalidate every
-- stored password record.
CREATE TABLE opaque_server_setup (
    id         BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    setup      BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One OPAQUE password record per user. The `record` is the serialized
-- ServerRegistration; `scheme_version` lets a future cipher-suite change force
-- re-enrollment on next login instead of a breaking migration.
CREATE TABLE user_password_credentials (
    user_id        UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    record         BYTEA NOT NULL,
    scheme_version INTEGER NOT NULL DEFAULT 1,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER user_password_credentials_set_updated_at
    BEFORE UPDATE ON user_password_credentials
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();

-- One TOTP secret per user. Stored as raw bytes; `confirmed_at` marks a secret
-- the user has proven they can generate codes for (always set for a secret
-- enrolled during bootstrap, since bootstrap verifies a code before persisting).
CREATE TABLE user_totp_secrets (
    user_id      UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    secret       BYTEA NOT NULL,
    confirmed_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
