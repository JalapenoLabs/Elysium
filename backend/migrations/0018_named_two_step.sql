-- Multiple named TOTP authenticators per user (with one default), and names on
-- passkeys, so a user with several of either can tell them apart and manage each.

-- TOTP moves from one-per-user to many. Backfill: each existing secret becomes a
-- named, default authenticator owned by its user.
ALTER TABLE user_totp_secrets
    ADD COLUMN id           UUID        NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN name         TEXT        NOT NULL DEFAULT 'Authenticator',
    ADD COLUMN is_default   BOOLEAN     NOT NULL DEFAULT TRUE,
    ADD COLUMN last_used_at TIMESTAMPTZ;

-- New rows are non-default unless explicitly chosen; existing rows kept their
-- backfilled TRUE above.
ALTER TABLE user_totp_secrets ALTER COLUMN is_default SET DEFAULT FALSE;

-- The primary key is now the per-authenticator id; user_id repeats across rows.
ALTER TABLE user_totp_secrets DROP CONSTRAINT user_totp_secrets_pkey;
ALTER TABLE user_totp_secrets ADD PRIMARY KEY (id);
CREATE INDEX user_totp_secrets_user_id_idx ON user_totp_secrets (user_id);

-- At most one default authenticator per user.
CREATE UNIQUE INDEX user_totp_secrets_one_default_per_user
    ON user_totp_secrets (user_id) WHERE is_default;

-- A user-assigned label for a passkey, so multiple passkeys are distinguishable.
ALTER TABLE webauthn_credentials ADD COLUMN name TEXT NOT NULL DEFAULT '';
