-- Device metadata on sessions, for the account owner's "active sessions" view.
--
-- Captured at sign-in from the same request info the login history uses: the
-- user agent identifies the browser/app and the IP the origin. Both default to
-- empty/NULL so existing rows remain valid. `last_seen_at` (already present) is
-- now kept current on authenticated activity so the view can show "last active".

ALTER TABLE sessions
    ADD COLUMN user_agent TEXT NOT NULL DEFAULT '',
    ADD COLUMN ip_address INET;
