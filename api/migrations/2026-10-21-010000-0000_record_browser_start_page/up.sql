-- When the browser's start page was opened in it. The API opens it once Chromium first answers,
-- which can be after an API restart or a failed attempt, so whether it is still owed is kept with
-- the row rather than in memory. See docs/browser.md.
ALTER TABLE browser_sessions ADD COLUMN start_page_opened_at TIMESTAMPTZ;
