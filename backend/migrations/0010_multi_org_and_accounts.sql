-- Global profiles, many-to-many org membership, and multi-account sessions.
--
-- A user row is now a deployment-global profile (identity + credentials + a
-- globally-unique email) that can belong to many orgs, so `users.org_id` gives way
-- to an `org_members` join. And a session now holds several signed-in accounts
-- with one active, so `sessions.user_id` gives way to `session_accounts` plus an
-- active pointer. See docs/identity.md and docs/auth.md.

-- 1) Org membership is many-to-many. Each membership carries its own role, the
--    org-level analogue of `team_members`.
CREATE TABLE org_members (
    user_id   UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    org_id    UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    role      TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'admin', 'owner')),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, org_id)
);

CREATE INDEX org_members_org_id_idx ON org_members (org_id);

-- Backfill: today every user belongs to exactly one org, as its owner (the only
-- existing users are first-run root claimers).
INSERT INTO org_members (user_id, org_id, role)
SELECT id, org_id, 'owner' FROM users;

DROP INDEX users_org_id_idx;
ALTER TABLE users DROP COLUMN org_id;

-- 2) Email is the global profile key: one account per email, case-insensitively.
CREATE UNIQUE INDEX users_email_unique_idx ON users (lower(email));

-- 3) A browser session holds multiple accounts with one active (Google-style).
CREATE TABLE session_accounts (
    session_id UUID NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    added_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (session_id, user_id)
);

ALTER TABLE sessions
    -- The account this session currently acts as; SET NULL on delete so the
    -- session survives (the app re-points it to another signed-in account).
    ADD COLUMN active_user_id UUID REFERENCES users (id) ON DELETE SET NULL,
    -- The active account's current org (the org switcher); optional.
    ADD COLUMN active_org_id UUID REFERENCES orgs (id) ON DELETE SET NULL;

-- Backfill: each existing session becomes a one-account session, active on that
-- account and its (now sole) org.
INSERT INTO session_accounts (session_id, user_id) SELECT id, user_id FROM sessions;
UPDATE sessions SET active_user_id = user_id;
UPDATE sessions s
   SET active_org_id = (SELECT org_id FROM org_members m WHERE m.user_id = s.active_user_id LIMIT 1);

DROP INDEX sessions_user_id_idx;
ALTER TABLE sessions DROP COLUMN user_id;
