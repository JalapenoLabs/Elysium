-- Split the user's name and make presence automatic.
--
-- Names follow the Slack model: `given_name`/`family_name` are the full name (both
-- optional), and `display_name` is the single field the UI renders, defaulting to
-- `given_name` when blank. Presence (online/away/offline) is now DERIVED from live
-- connections and activity, never a stored choice, so the old manually-set `status`
-- column is replaced by an explicit, expiring manual override (docs/auth-model.md,
-- docs/identity.md).

ALTER TABLE users
    ADD COLUMN given_name  TEXT NOT NULL DEFAULT '',
    ADD COLUMN family_name TEXT NOT NULL DEFAULT '';

-- Derived presence is computed at read time (live sessions + `last_seen_at`), so
-- there is no stored presence column. The manual override is what users set.
ALTER TABLE users DROP COLUMN status;

ALTER TABLE users
    ADD COLUMN manual_status TEXT
        CHECK (manual_status IN ('out_of_office', 'do_not_disturb', 'custom')),
    -- Label for a 'custom' status; empty for the fixed kinds.
    ADD COLUMN manual_status_text TEXT NOT NULL DEFAULT '',
    -- When the override lapses back to automatic presence. NULL when none is set.
    ADD COLUMN manual_status_expires_at TIMESTAMPTZ;
