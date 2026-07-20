-- Per-kind member-linking policies for integrations (docs/integrations.md).
--
-- Admins flip per-kind toggles ("members may connect personal email accounts:
-- on / personal git accounts: off"), exactly like the org auth policies. A
-- member may create a PERSONAL LINKED integration of a kind only while that
-- kind's toggle is on. No row means the documented default: personal linking is
-- OFF until an admin enables it, per kind, so a fresh org starts fully closed.
--
-- `kind` mirrors integrations.kind (1=git, 2=mail, 3=registry, 4=perforce,
-- 5=runner); the proto's 0 UNSPECIFIED sentinel is never persisted.

CREATE TABLE integration_policies (
    org_id           UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    kind             SMALLINT NOT NULL CHECK (kind IN (1, 2, 3, 4, 5)),
    members_may_link BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (org_id, kind)
);
