-- When the engine last reloaded the service a browser is signed in to, so its rolling session
-- renews. Null until the first refresh; until then the engine counts a day from when the
-- browser's start page opened. Kept on the row, so a restart neither reloads every browser at once
-- nor skips one. See docs/browser.md, Browser sessions.
ALTER TABLE browser_sessions ADD COLUMN refreshed_at TIMESTAMPTZ;
