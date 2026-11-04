-- Puts back the columns the meetings loop recorded its own failures in. A meeting the up
-- migration carried on from a failure stays where it went back to: nothing kept says which
-- meetings those were, and each one's run says why it failed when it does.
DROP INDEX meetings_unfinished;

ALTER TABLE meetings
    ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    ADD COLUMN last_error TEXT,
    DROP CONSTRAINT meetings_state_check,
    ADD CONSTRAINT meetings_state_check
        CHECK (state IN ('pending', 'recorded', 'segmented', 'done', 'failed'));

CREATE INDEX meetings_unfinished ON meetings (created_at) WHERE state NOT IN ('done', 'failed');
