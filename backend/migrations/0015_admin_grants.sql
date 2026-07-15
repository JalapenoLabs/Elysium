-- Delegated admin capabilities.
--
-- An org can grant a member specific admin powers without making them a full
-- admin (the Google Workspace "delegated admin role" model). Owners and admins
-- hold every capability implicitly; this table records the extra grants given to
-- ordinary members. No billing capability exists: the project is open source with
-- nothing gated. See docs/org-admin.md.

CREATE TABLE org_admin_grants (
    org_id     UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    capability TEXT NOT NULL
               CHECK (capability IN ('manage_members', 'manage_security', 'manage_teams', 'view_audit')),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id, capability)
);

CREATE INDEX org_admin_grants_lookup_idx ON org_admin_grants (org_id, user_id);
