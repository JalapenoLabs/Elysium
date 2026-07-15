-- Member lifecycle: suspension and the guest tier.
--
-- `suspended_at` marks a membership inactive without deleting it: a suspended
-- member keeps their row (so an admin can reactivate them) but the authorization
-- guard treats them as having no access. A new lowest role, `guest`, sits below
-- `member` for limited-access collaborators. See docs/org-admin.md.

ALTER TABLE org_members ADD COLUMN suspended_at TIMESTAMPTZ;

ALTER TABLE org_members DROP CONSTRAINT org_members_role_check;
ALTER TABLE org_members
    ADD CONSTRAINT org_members_role_check
    CHECK (role IN ('guest', 'member', 'admin', 'owner'));
