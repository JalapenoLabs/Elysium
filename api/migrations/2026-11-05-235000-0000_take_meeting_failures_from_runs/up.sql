-- Every step of a meeting is a step of its run of the meeting workflow, segmenting and
-- publishing included, so a meeting's failure is its run's: the run's failed step holds the
-- reason, and retrying that step is what moves the meeting on. The meeting keeps only how far its
-- processing got. See docs/meetings.md, Processing.
--
-- A meeting given up on before keeps what it stored and goes back to where that leaves it, so
-- the meetings loop starts a run that carries it on from there.
UPDATE meetings
SET state = CASE
    WHEN segments IS NOT NULL THEN 'segmented'
    -- A meeting fetched before transcripts were hashed shows it by its recording alone.
    WHEN transcript_hash IS NOT NULL OR recording_path IS NOT NULL THEN 'recorded'
    ELSE 'pending'
END
WHERE state = 'failed';

DROP INDEX meetings_unfinished;

ALTER TABLE meetings
    DROP CONSTRAINT meetings_state_check,
    ADD CONSTRAINT meetings_state_check
        CHECK (state IN ('pending', 'recorded', 'segmented', 'done')),
    DROP COLUMN attempts,
    DROP COLUMN last_error;

-- The meetings loop follows every meeting whose segments are not on the timeline yet.
CREATE INDEX meetings_unfinished ON meetings (created_at) WHERE state <> 'done';
