-- Server-side session store for the BFF (Backend-for-Frontend) auth model.
--
-- The browser holds only an opaque HttpOnly cookie carrying `id`. The real
-- Keycloak tokens live here, server-side, and are never exposed to JavaScript.
-- A row is created at the OIDC callback and deleted on logout or expiry.

CREATE TABLE sessions (
    -- Opaque session identifier; the value stored in the browser's cookie.
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The authenticated principal, projected from the ID token at login.
    keycloak_sub  TEXT NOT NULL,
    email         TEXT NOT NULL DEFAULT '',
    username      TEXT NOT NULL DEFAULT '',
    -- Keycloak tokens held server-side. `id_token` is retained for the
    -- `id_token_hint` on RP-initiated logout. (Encryption at rest is a planned
    -- hardening step.)
    access_token  TEXT NOT NULL,
    refresh_token TEXT NOT NULL DEFAULT '',
    id_token      TEXT NOT NULL DEFAULT '',
    -- Access-token expiry; the session is refreshed or ended past this.
    expires_at    TIMESTAMPTZ NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX sessions_keycloak_sub_idx ON sessions (keycloak_sub);
