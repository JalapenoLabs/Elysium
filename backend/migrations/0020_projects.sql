-- Projects: the org -> project -> team tenancy tier.
--
-- Introduces Project as a first-class tier between Org and Team. A project is a
-- sub-tenant workspace that owns its domains, Drives, integrations, teams, and
-- membership; the org above it is a thin umbrella. Every org has at least one
-- project: a default is auto-created on org creation (see backend/src/projects.rs).
-- Roles are stored as smallints mirroring the elysium.project.v1.ProjectRole enum
-- (1=member, 2=admin, 3=owner); 0 (UNSPECIFIED) is never persisted. See
-- docs/projects.md.

-- A sub-tenant workspace under an org. Slugs are unique within their org.
CREATE TABLE projects (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    slug       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Slugs are unique within their organization, not globally.
    UNIQUE (org_id, slug)
);

CREATE INDEX projects_org_id_idx ON projects (org_id);

-- Explicit project membership + role. Full org members reach every project
-- implicitly; this edge carries a per-project role and covers scoped guests.
CREATE TABLE project_members (
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 1=member, 2=admin, 3=owner (elysium.project.v1.ProjectRole minus UNSPECIFIED).
    role       SMALLINT NOT NULL CHECK (role IN (1, 2, 3)),
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, user_id)
);

CREATE INDEX project_members_user_id_idx ON project_members (user_id);

-- Teams gain an optional project scope: NULL = org-level (inherited by every
-- project), set = scoped to that one project.
ALTER TABLE teams
    ADD COLUMN project_id UUID REFERENCES projects (id) ON DELETE CASCADE;

CREATE INDEX teams_project_id_idx ON teams (project_id);

-- A project owns its domains, and a domain maps to exactly one project across the
-- whole deployment (mail routing and address issuance depend on this uniqueness).
CREATE TABLE project_domains (
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    domain     TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, domain)
);
