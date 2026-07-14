-- Per-organization authentication policy.
--
-- One row per org, created alongside the org. Every column defaults to today's
-- behavior (passkey-only sign-in, no mandated second factor), so a fresh row is
-- always safe. Enforcement lands with each auth method as it ships
-- (docs/auth-model.md); this table is the control surface orgs configure.

CREATE TABLE org_auth_policies (
    org_id UUID PRIMARY KEY REFERENCES orgs (id) ON DELETE CASCADE,

    -- Which sign-in methods members may use. At least one must stay enabled so
    -- a policy change can never lock an entire org out.
    allow_passkeys    BOOLEAN NOT NULL DEFAULT TRUE,
    allow_passwords   BOOLEAN NOT NULL DEFAULT FALSE, -- OPAQUE (RFC 9807) password login
    allow_magic_links BOOLEAN NOT NULL DEFAULT FALSE, -- passwordless email sign-in
    CHECK (allow_passkeys OR allow_passwords OR allow_magic_links),

    -- Whether members must enroll a TOTP second factor.
    require_totp BOOLEAN NOT NULL DEFAULT FALSE,

    -- Absolute session lifetime. Mirrors the `sessions` table default (30 days).
    session_lifetime_seconds INTEGER NOT NULL DEFAULT 2592000
                             CHECK (session_lifetime_seconds > 0),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER org_auth_policies_set_updated_at
    BEFORE UPDATE ON org_auth_policies
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();

-- Existing orgs adopt the default policy.
INSERT INTO org_auth_policies (org_id)
SELECT id FROM orgs
ON CONFLICT (org_id) DO NOTHING;
