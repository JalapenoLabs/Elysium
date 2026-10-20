-- Browsers the host runs for a person to sign in to their accounts in, streamed to them over
-- WebRTC. Each is one Neko container with its own profile volume, Docker network, and media
-- port. See docs/browser.md.

CREATE TABLE browser_sessions (
    id UUID PRIMARY KEY,
    -- What the person calls it, such as "Work Outlook".
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    -- The page the browser opens when it is created.
    start_url TEXT NOT NULL CHECK (start_url LIKE 'https://%' AND char_length(start_url) <= 2048),
    -- The host port its media arrives on, from the range nginx publishes. One browser per port.
    port INTEGER NOT NULL UNIQUE CHECK (port BETWEEN 59000 AND 59007),
    -- The password of the Neko member session the API relays to the browser's owner, sealed with the
    -- encryption key. Only the API signs in with it.
    member_password_encrypted BYTEA NOT NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX browser_sessions_created_by ON browser_sessions (created_by);

SELECT diesel_manage_updated_at('browser_sessions');
