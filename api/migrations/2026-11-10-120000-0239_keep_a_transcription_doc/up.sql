-- How speech is misheard in a company's meetings is one doc, Transcription: every correction and
-- every transcription reads it, and Elysium adds to it as people fix transcripts and as its
-- corrections are sure. See docs/meeting-review.md, The Transcription doc, and docs/meetings.md,
-- The Transcription doc.

-- The Transcription page, which Elysium relies on as it does Learnings and Company: seeded at an
-- id fixed in code (crate::models::page::TRANSCRIPTION_PAGE_ID), after them at the top of the
-- tree, and never moved, deleted, or renamed. Its body is the skeleton crate::transcription_doc
-- reads and writes: an Entries table, and what it taught itself, newest first. It enters the
-- knowledge index the first time it is published, when it says something of its own.
INSERT INTO pages (id, position, title, body, created_by, created_at)
VALUES (
    '00000000-0000-0000-0001-000000000005',
    coalesce((SELECT max(position) FROM pages WHERE parent_id IS NULL), 0) + 1048576,
    'Transcription',
    'How speech is misheard in the company''s meetings, and what it really is. Every '
        || 'meeting''s correction reads this page, and every transcription is told what each '
        || 'entry is, so it is heard right more often. Elysium adds to it as people fix '
        || 'transcripts and as its corrections are sure. Anyone may edit it, and every change '
        || E'is a version.\n\n'
        || E'## Entries\n\n'
        || 'What was heard, what it is, where it is heard (the transcript that heard it, and '
        || E'who was speaking), and the meetings it was heard in.\n\n'
        || E'| Heard | Is | Sources | Speakers | Meetings |\n'
        || E'| --- | --- | --- | --- | --- |\n\n'
        || E'## What it taught itself\n\n'
        || 'Newest first: each entry Elysium added or heard again, and why, for a person to '
        || 'review and prune.',
    '00000000-0000-0000-0000-000000000001',
    now()
);
INSERT INTO page_versions (page_id, version, title, body, created_by, created_at)
SELECT id, 1, title, body, created_by, created_at
FROM pages
WHERE id = '00000000-0000-0000-0001-000000000005';
INSERT INTO page_events (id, page_id, kind, created_by, data, created_at)
SELECT uuidv7(), id, 'created', created_by, jsonb_build_object('parentId', NULL, 'version', 1),
       created_at
FROM pages
WHERE id = '00000000-0000-0000-0001-000000000005';
