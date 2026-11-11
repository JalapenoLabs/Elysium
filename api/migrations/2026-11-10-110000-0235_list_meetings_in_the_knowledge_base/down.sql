DROP INDEX timeline_records_meeting_segments;

-- What people put under Meetings, deleted pages included, moves to the top of the tree after
-- everything there, in its order, and the Meetings page goes with its version and history.
UPDATE pages
SET parent_id = NULL,
    position = (SELECT max(position) FROM pages WHERE parent_id IS NULL) + moved.place * 1048576
FROM (
    SELECT id, row_number() OVER (ORDER BY position) AS place
    FROM pages
    WHERE parent_id = '00000000-0000-0000-0001-000000000004'
) AS moved
WHERE pages.id = moved.id;
DELETE FROM pages WHERE id = '00000000-0000-0000-0001-000000000004';
