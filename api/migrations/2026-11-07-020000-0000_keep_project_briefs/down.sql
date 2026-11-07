ALTER TABLE work_batches DROP COLUMN serves;

-- Without briefs their wakes mean nothing, so they go, and the kind type is rebuilt without
-- theirs: Postgres cannot drop an enum value.
DELETE FROM project_manager_wakes WHERE kind = 'brief';
ALTER TABLE project_manager_wakes DROP CONSTRAINT project_manager_wakes_kind_fields;
ALTER TABLE project_manager_wakes DROP COLUMN page_version;

ALTER TYPE project_manager_wake_kind RENAME TO project_manager_wake_kind_with_brief;
CREATE TYPE project_manager_wake_kind AS ENUM (
    -- A record was attributed to the project: a meeting's segment, a conversation's message,
    -- or a record a person put on it.
    'record',
    -- A batch it submitted finished.
    'batch',
    -- A person or Elysia wrote to it.
    'message',
    -- A promise was found on the project, or the work it waits on finished.
    'promise'
);
ALTER TABLE project_manager_wakes
    ALTER COLUMN kind TYPE project_manager_wake_kind
    USING kind::text::project_manager_wake_kind;
DROP TYPE project_manager_wake_kind_with_brief;

ALTER TABLE project_manager_wakes ADD CONSTRAINT project_manager_wakes_kind_fields CHECK (
    (timeline_record_id IS NOT NULL) = (kind = 'record')
    AND (work_batch_id IS NOT NULL) = (kind = 'batch')
    AND (body IS NOT NULL) = (kind = 'message')
    AND (promise_id IS NOT NULL) = (kind = 'promise')
);

-- A brief nobody wrote in is exactly what up.sql or creating a project made, so it goes, with its
-- version, history, link, and labels. A brief someone published, or hung a page or a file on, keeps
-- what they wrote: it stays, an ordinary page linked to its project.
DELETE FROM pages AS brief
WHERE brief.brief_project_id IS NOT NULL
    AND brief.current_version = 1
    AND NOT EXISTS (SELECT 1 FROM pages AS child WHERE child.parent_id = brief.id)
    AND NOT EXISTS (SELECT 1 FROM page_attachments WHERE page_attachments.page_id = brief.id);

ALTER TABLE pages DROP CONSTRAINT pages_brief_is_a_plain_page;
ALTER TABLE pages DROP COLUMN brief_project_id;

-- What is left under Project briefs, deleted pages included, moves to the top of the tree after
-- everything there, in its order, and the Project briefs page goes with its version and history.
UPDATE pages
SET parent_id = NULL,
    position = (SELECT max(position) FROM pages WHERE parent_id IS NULL) + moved.place * 1048576
FROM (
    SELECT id, row_number() OVER (ORDER BY position) AS place
    FROM pages
    WHERE parent_id = '00000000-0000-0000-0001-000000000003'
) AS moved
WHERE pages.id = moved.id;
DELETE FROM pages WHERE id = '00000000-0000-0000-0001-000000000003';
