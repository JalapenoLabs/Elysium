-- Takes away where each capture sits on its meeting's list and how its offset was found, and the
-- record of meetings that looked like one. Offsets stay as they are: the code of before reads them.

-- A merged meeting's captures live in the meeting they were merged into, and only `merged_into`
-- says where they came from, so the revert refuses rather than lose it. Split them first.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM meetings WHERE merged_into IS NOT NULL) THEN
        RAISE EXCEPTION 'meetings were merged; split their captures back out before reverting';
    END IF;
END $$;

DROP TABLE meeting_duplicates;

DROP INDEX meetings_started_by_person;
DROP INDEX meetings_merged_into;
ALTER TABLE meetings
    DROP CONSTRAINT meetings_merged_into_another,
    DROP COLUMN merged_into;

DROP INDEX meeting_captures_of_meeting;
CREATE INDEX meeting_captures_of_meeting ON meeting_captures (meeting_id, created_at, id);

ALTER TABLE meeting_captures
    DROP COLUMN joined_at,
    DROP COLUMN offset_source;
