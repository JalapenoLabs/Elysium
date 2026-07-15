-- Org-wide audit log: an append-only trail of admin-consequential actions.
--
-- Who did what, to whom, when, in an org. Rows survive the actor's or target's
-- deletion (`ON DELETE SET NULL`) so the history is never silently rewritten.
-- `detail` carries action-specific context as JSON. See docs/org-admin.md.

CREATE TABLE audit_log (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id         UUID NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    actor_user_id  UUID REFERENCES users (id) ON DELETE SET NULL,
    action         TEXT NOT NULL,
    target_user_id UUID REFERENCES users (id) ON DELETE SET NULL,
    detail         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_log_org_id_created_at_idx ON audit_log (org_id, created_at DESC);
