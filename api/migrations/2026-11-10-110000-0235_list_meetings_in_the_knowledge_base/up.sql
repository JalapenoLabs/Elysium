-- Meetings are daily work, so they live in the knowledge base: a Meetings page lists every meeting
-- the reader can see and is each one's home, and each project's brief holds a Meetings group of
-- the meetings with a topic on it. See docs/meeting-review.md, In the knowledge base, and
-- docs/knowledge.md, Pages.

-- The Meetings page, which Elysium relies on as it does Learnings, Company, and Project briefs:
-- seeded at an id fixed in code (crate::models::page::MEETINGS_PAGE_ID), after them at the top of
-- the tree, and never moved, deleted, or renamed. Its meetings are no pages of their own: the web
-- app lists them under it, and under each brief, from the meetings their reader can see.
INSERT INTO pages (id, position, title, created_by, created_at)
VALUES (
    '00000000-0000-0000-0001-000000000004',
    coalesce((SELECT max(position) FROM pages WHERE parent_id IS NULL), 0) + 1048576,
    'Meetings',
    '00000000-0000-0000-0000-000000000001',
    now()
);
INSERT INTO page_versions (page_id, version, title, body, created_by, created_at)
SELECT id, 1, title, body, created_by, created_at
FROM pages
WHERE id = '00000000-0000-0000-0001-000000000004';
INSERT INTO page_events (id, page_id, kind, created_by, data, created_at)
SELECT uuidv7(), id, 'created', created_by, jsonb_build_object('parentId', NULL, 'version', 1),
       created_at
FROM pages
WHERE id = '00000000-0000-0000-0001-000000000004';

-- A meeting's topics are its segments' records, each naming its meeting in its metadata. Listing
-- meetings reads the projects every one's topics are on, so they are found by the meeting rather
-- than by reading every record the person has.
CREATE INDEX timeline_records_meeting_segments ON timeline_records ((metadata ->> 'meetingId'))
    WHERE source_kind = 'meeting';
