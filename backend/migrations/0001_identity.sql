-- Identity schema: organizations, teams, users, and memberships.
--
-- Keycloak owns credentials; this schema owns profile and membership data. Each
-- user is linked back to Keycloak by its subject claim (`keycloak_sub`). Roles
-- and status are stored as text guarded by CHECK constraints, mirroring the
-- `elysium.user.v1` proto enums without the rigidity of Postgres enum types.

-- A top-level tenant. Every team and user belongs to exactly one organization.
CREATE TABLE orgs (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL,
    slug       TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A group within an organization (admins, dev, qa, contractors, interns, ...).
CREATE TABLE teams (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    slug       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Slugs are unique within their organization, not globally.
    UNIQUE (org_id, slug)
);

-- An application user, mirrored from Keycloak plus Elysium-owned profile fields.
CREATE TABLE users (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The stable link to Keycloak; the subject (`sub`) claim of its tokens.
    keycloak_sub TEXT NOT NULL UNIQUE,
    org_id       UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    email        TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    avatar_url   TEXT NOT NULL DEFAULT '',
    bio          TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'offline'
                 CHECK (status IN ('online', 'offline', 'away', 'out_of_office')),
    last_seen_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX users_org_id_idx ON users (org_id);

-- Membership edge linking a user to a team with a role.
CREATE TABLE team_members (
    user_id   UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    team_id   UUID NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    role      TEXT NOT NULL DEFAULT 'member'
              CHECK (role IN ('member', 'admin', 'owner')),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, team_id)
);

CREATE INDEX team_members_team_id_idx ON team_members (team_id);

-- Keeps `users.updated_at` honest without relying on every writer to set it.
CREATE FUNCTION set_updated_at() RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_set_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();
