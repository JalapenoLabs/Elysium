-- Backfill: give every org that predates the projects tier a default project.
--
-- The invariant is "every org has at least one project" (docs/projects.md), upheld
-- for new orgs by projects::create_default. Orgs created before 0020_projects.sql
-- never ran that path and have zero projects, so GET /projects returns empty for
-- them. This backfill closes the gap, mirroring create_default exactly: a project
-- named "Default" with slug "default", owned by the org's owner via a
-- project_members row with role 3 (OWNER, per elysium.project.v1.ProjectRole).
--
-- Idempotent and safe to re-run: it only touches orgs with no project at all, and
-- the owner membership uses ON CONFLICT DO NOTHING. Freshly-created orgs already
-- have a default project, so they are skipped and stay identical to backfilled ones.

WITH seeded AS (
    -- One default project per org that currently has none. On re-run every org has a
    -- project, so this inserts nothing.
    INSERT INTO projects (org_id, name, slug)
    SELECT o.id, 'Default', 'default'
      FROM orgs o
     WHERE NOT EXISTS (SELECT 1 FROM projects p WHERE p.org_id = o.id)
    RETURNING id AS project_id, org_id
),
owners AS (
    -- The org owner becomes the default project's OWNER. An org may list more than
    -- one owner; pick the earliest-joined one deterministically. An org with no
    -- owner at all still gets its project (invariant preserved), just no owner row.
    SELECT DISTINCT ON (s.project_id)
           s.project_id, m.user_id
      FROM seeded s
      JOIN org_members m ON m.org_id = s.org_id AND m.role = 'owner'
     ORDER BY s.project_id, m.joined_at, m.user_id
)
INSERT INTO project_members (project_id, user_id, role)
SELECT project_id, user_id, 3
  FROM owners
ON CONFLICT (project_id, user_id) DO NOTHING;
