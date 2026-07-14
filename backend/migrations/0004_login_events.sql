-- Login history: an append-only audit trail of sign-in attempts.
--
-- Every sign-in attempt, successful or not, is recorded so users and admins can
-- audit account access (docs/auth-model.md). `user_id` is SET NULL on user
-- deletion so the audit trail outlives the account. `email` captures the
-- identifier the client presented, which for a failed lookup may not correspond
-- to any user.

CREATE TABLE login_events (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID REFERENCES users (id) ON DELETE SET NULL,
    -- The identifier presented at sign-in; empty when the step provides none
    -- (the finish half of a ceremony identifies the user by ceremony state).
    email      TEXT NOT NULL DEFAULT '',
    -- Only the methods the API actually records; extended by migration as new
    -- methods ship (passwords, magic links, TOTP, SSO per the roadmap).
    method     TEXT NOT NULL CHECK (method IN ('passkey')),
    outcome    TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
    ip_address INET,
    user_agent TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The account-security view: a user's history, newest first.
CREATE INDEX login_events_user_id_created_at_idx ON login_events (user_id, created_at DESC);
