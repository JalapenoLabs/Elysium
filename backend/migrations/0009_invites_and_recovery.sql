-- Steady-state onboarding (invites) and account recovery (recovery codes).
--
-- Invites let an existing member bring in a new one without SMTP: minting stores
-- only the hash of a high-entropy link token, so the plaintext lives only in the
-- copyable link. An invite is single-use (`accepted_at`) and expiring. Recovery
-- codes are one-time secrets issued at enrollment so a user who loses their device
-- can still get back in; only their hashes are stored. See docs/auth.md.

CREATE TABLE invites (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id      UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    -- Optional team to place the invitee on; NULL means org-only for now.
    team_id     UUID REFERENCES teams (id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'admin', 'owner')),
    -- Optional suggested email, prefilled at acceptance; never trusted as verified.
    email       TEXT NOT NULL DEFAULT '',
    -- SHA-256 hex of the link token; the plaintext token is never stored.
    token_hash  TEXT NOT NULL UNIQUE,
    created_by  UUID REFERENCES users (id) ON DELETE SET NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    -- Set when consumed; a non-NULL value makes the invite single-use.
    accepted_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX invites_org_id_idx ON invites (org_id);

CREATE TABLE recovery_codes (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- SHA-256 hex of the normalized code; the plaintext is shown once, never stored.
    code_hash  TEXT NOT NULL,
    used_at    TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Lookups only ever seek an unused code for a user.
CREATE INDEX recovery_codes_unused_idx ON recovery_codes (user_id) WHERE used_at IS NULL;

-- Recovery sign-in records its own login-history method.
ALTER TABLE login_events DROP CONSTRAINT login_events_method_check;
ALTER TABLE login_events
    ADD CONSTRAINT login_events_method_check
    CHECK (method IN ('passkey', 'password', 'totp', 'recovery'));
