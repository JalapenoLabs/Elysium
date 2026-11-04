-- A new authenticator app a person is moving to, waiting for its first code. Kratos holds the
-- app in use and offers a new one only once that one is removed, so Elysium holds the new
-- app's secret itself until a code from it is verified, then swaps it into Kratos in one
-- write: the old app works until that moment, and two-factor authentication is never off.
-- A row lives for minutes and is replaced when the person starts again. See docs/auth.md.
CREATE TABLE authenticator_replacements (
    -- A child of the person's account, one at a time.
    user_id UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    -- The new app's TOTP secret, sealed with `authenticator_replacements.secret:<user_id>`.
    secret_encrypted BYTEA NOT NULL,
    -- After this the row is refused, and removed the next time anyone starts a replacement.
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
