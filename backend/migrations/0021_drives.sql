-- Storage: project-scoped Drives, their access grants, and org storage settings.
--
-- A Drive is one OpenDAL operator rooted verbatim where the org points it, plus
-- Elysium metadata (name, kind, visibility, default flag, sealed credential). The
-- provider is always the source of truth; nothing is mirrored here. Drives are
-- project-scoped (docs/projects.md, docs/storage.md): every Drive carries a
-- project_id and access resolves through the org -> project -> team hierarchy.
--
-- `kind` and `visibility` are smallints mirroring the elysium.storage.v1 proto
-- enums (DriveKind: 1=FS, 2=S3, 3=SFTP; DriveVisibility: 1=ORG_WIDE, 2=RESTRICTED);
-- the 0 UNSPECIFIED sentinels are never persisted. Non-secret connection settings
-- (endpoint/bucket/prefix/host/base_path/root...) live in `config` JSONB, validated
-- per-kind in Rust so a new provider needs no migration. The provider secret is a
-- single crypto.rs-sealed blob in `secret`, NULL for FS which has no credential.

CREATE TABLE drives (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The project that owns this Drive. Drives are project-scoped, not loose in the
    -- org; the auto-provisioned default Drive lands in the org's default project.
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    -- The owning org. Org isolation comes from this column, not from paths.
    org_id     UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    -- DriveKind proto enum: 1=FS, 2=S3, 3=SFTP (0 UNSPECIFIED never stored).
    kind       SMALLINT NOT NULL CHECK (kind IN (1, 2, 3)),
    -- Non-secret connection settings; the shape is validated per-kind in Rust before
    -- the OpenDAL operator is built.
    config     JSONB NOT NULL DEFAULT '{}',
    -- crypto.rs-sealed provider credential (XChaCha20-Poly1305); NULL for FS.
    secret     BYTEA,
    -- DriveVisibility proto enum: 1=ORG_WIDE, 2=RESTRICTED.
    visibility SMALLINT NOT NULL DEFAULT 1 CHECK (visibility IN (1, 2)),
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    -- The user who created the Drive; NULL once that user is deleted, and unset for
    -- system-provisioned Drives.
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Exactly one default Drive per project (not per org): Drives are project-scoped and
-- each project has its own default, where drag-and-drop uploads land.
CREATE UNIQUE INDEX drives_one_default_per_project ON drives (project_id) WHERE is_default;
CREATE INDEX drives_project_id_idx ON drives (project_id);
CREATE INDEX drives_org_id_idx ON drives (org_id);

-- Access grants for a restricted Drive: a mix of teams and individual users. An
-- org-wide Drive needs no rows here; admins and up bypass in the authz check, never
-- stored. `principal_id` is a team_id or user_id per `principal`, so it carries no
-- foreign key of its own (the referenced table depends on the principal kind).
CREATE TABLE drive_access (
    drive_id     UUID NOT NULL REFERENCES drives (id) ON DELETE CASCADE,
    -- DriveAccess.Principal proto enum: 1=team, 2=user.
    principal    SMALLINT NOT NULL CHECK (principal IN (1, 2)),
    principal_id UUID NOT NULL,
    PRIMARY KEY (drive_id, principal, principal_id)
);

-- Per-org storage settings. The per-file upload cap lives in the DB, not .env, so an
-- admin edits it in-app. Because transfers stream through the backend, the cap is a
-- policy choice, not a memory limit (docs/storage.md). Default 5 GiB.
CREATE TABLE org_storage_settings (
    org_id           UUID PRIMARY KEY REFERENCES orgs (id) ON DELETE CASCADE,
    max_upload_bytes BIGINT NOT NULL DEFAULT 5368709120
);
