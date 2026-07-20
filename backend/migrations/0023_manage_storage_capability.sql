-- Extend the delegated admin capabilities with project and storage management.
--
-- The Capability enum (backend/src/authz.rs) already carries manage_projects, and
-- now manage_storage for the Drive management API, but the original CHECK omitted
-- both. Rewriting the constraint keeps the delegated-grant vocabulary in lockstep
-- with the code, so an org can delegate either capability to an ordinary member.

ALTER TABLE org_admin_grants DROP CONSTRAINT org_admin_grants_capability_check;

ALTER TABLE org_admin_grants
    ADD CONSTRAINT org_admin_grants_capability_check
    CHECK (capability IN ('manage_members', 'manage_security', 'manage_teams',
                          'manage_projects', 'manage_storage', 'view_audit'));
