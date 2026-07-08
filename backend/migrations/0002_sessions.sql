-- Server-side sessions.
--
-- The browser holds only an opaque HttpOnly cookie carrying `id`; the session
-- references the authenticated user. Auth is fully API-owned (the API is the
-- WebAuthn Relying Party, see docs/auth-model.md), so a session holds no external
-- tokens. A row is created on a successful passkey sign-in and deleted on logout.

CREATE TABLE sessions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Absolute lifetime; expired sessions are treated as signed out.
    expires_at   TIMESTAMPTZ NOT NULL DEFAULT now() + INTERVAL '30 days'
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);
