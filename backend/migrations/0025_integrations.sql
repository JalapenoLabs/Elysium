-- Orchestration: service integrations, their storage-role bindings, and access.
--
-- An integration is to services what a Drive is to files (docs/integrations.md):
-- an org owns many, of many kinds, each MANAGED (Elysium provisions a container)
-- or LINKED (Elysium holds credentials to an external endpoint). Managed
-- integrations are desired state, reconciled to actual containers on boot and on
-- change, so a host reboot or image upgrade never loses them.
--
-- Smallint columns mirror the elysium.integration.v1 proto enums; the 0
-- UNSPECIFIED sentinels are never persisted:
--   kind:        1=git, 2=mail, 3=registry, 4=perforce, 5=runner
--   mode:        1=managed, 2=linked
--   owner_scope: 1=org, 2=project, 3=personal (docs/projects.md)
--   visibility:  1=org_wide, 2=restricted (mirrors drives)
--   status:      1=pending, 2=provisioning, 3=running, 4=stopped, 5=failed
-- `desired` is the admin's intended lifecycle (1=running, 2=stopped), internal to
-- the reconciler and driven by the start/stop endpoints.

CREATE TABLE integrations (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The owning org. Org isolation comes from this column, never from names.
    org_id       UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    -- The owning project for project-scoped integrations (the default home for
    -- work services; one container per project). NULL for org or personal scope.
    project_id   UUID REFERENCES projects (id) ON DELETE CASCADE,
    kind         SMALLINT NOT NULL CHECK (kind IN (1, 2, 3, 4, 5)),
    mode         SMALLINT NOT NULL CHECK (mode IN (1, 2)),
    owner_scope  SMALLINT NOT NULL CHECK (owner_scope IN (1, 2, 3)),
    -- The owning principal per owner_scope: the org_id (org), the project_id
    -- (project), or the user_id (personal). No FK of its own since the referenced
    -- table depends on the scope, mirroring drive_access.principal_id.
    owner_id     UUID NOT NULL,
    name         TEXT NOT NULL,
    visibility   SMALLINT NOT NULL DEFAULT 1 CHECK (visibility IN (1, 2)),
    -- Kind-specific non-secret settings, validated as a JSON object in Rust so a
    -- new kind needs no migration.
    config       JSONB NOT NULL DEFAULT '{}',
    -- crypto.rs-sealed credential (API token, admin cred); NULL when none is set.
    secret       BYTEA,
    desired      SMALLINT NOT NULL DEFAULT 1 CHECK (desired IN (1, 2)),
    status       SMALLINT NOT NULL DEFAULT 1 CHECK (status IN (1, 2, 3, 4, 5)),
    -- Human-readable reason for PENDING/FAILED states, rendered by the console.
    status_detail TEXT NOT NULL DEFAULT '',
    -- The user who created the integration; NULL once that user is deleted.
    created_by   UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Project scope and project_id travel together: exactly the project-scoped
    -- rows carry a project.
    CHECK ((owner_scope = 2) = (project_id IS NOT NULL))
);

CREATE INDEX integrations_org_id_idx ON integrations (org_id);
CREATE INDEX integrations_project_id_idx ON integrations (project_id);

-- Storage-role bindings for managed integrations: which Drive each consumer role
-- (git "repos", registry "blobs", ...) persists onto. Capability-checked in Rust
-- against the Drive kind's advertised capabilities (docs/storage.md), so "git on
-- OneDrive" is an enforced impossibility. The plain drives FK blocks deleting a
-- Drive a managed service still persists onto.
CREATE TABLE integration_storage_bindings (
    integration_id UUID NOT NULL REFERENCES integrations (id) ON DELETE CASCADE,
    role           TEXT NOT NULL,
    drive_id       UUID NOT NULL REFERENCES drives (id),
    -- Path prefix within the Drive; empty uses the reserved
    -- ".elysium/<kind>/<role>" consumer area.
    prefix         TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (integration_id, role)
);

-- Access grants for a restricted integration: a mix of teams and individual
-- users, mirroring drive_access. An org-wide integration needs no rows here;
-- admins and up bypass in the authz check, never stored.
CREATE TABLE integration_access (
    integration_id UUID NOT NULL REFERENCES integrations (id) ON DELETE CASCADE,
    -- Principal kind, mirroring drive_access: 1=team, 2=user.
    principal      SMALLINT NOT NULL CHECK (principal IN (1, 2)),
    principal_id   UUID NOT NULL,
    PRIMARY KEY (integration_id, principal, principal_id)
);

-- Extend the delegated admin capabilities with integration management, exactly
-- as 0023 did for manage_projects/manage_storage: the Capability enum
-- (backend/src/authz.rs) gains manage_integrations, so the CHECK follows.
ALTER TABLE org_admin_grants DROP CONSTRAINT org_admin_grants_capability_check;

ALTER TABLE org_admin_grants
    ADD CONSTRAINT org_admin_grants_capability_check
    CHECK (capability IN ('manage_members', 'manage_security', 'manage_teams',
                          'manage_projects', 'manage_storage',
                          'manage_integrations', 'view_audit'));
