-- Every project has a brief: a page in the knowledge base, linked to the project, that holds its
-- goals, its decisions, and what is out of scope. Its Project Manager reads it every turn and keeps
-- it current, every batch names the goal in it that the batch serves, and a person's edit wakes the
-- Project Manager. See docs/autonomy.md, The project brief, and docs/knowledge.md, Pages.

-- Every brief sits under one page, Project briefs, which Elysium relies on as it does Learnings
-- and Company: seeded at an id fixed in code (crate::models::page::PROJECT_BRIEFS_PAGE_ID), after
-- them at the top of the tree, and never moved, deleted, or renamed.
INSERT INTO pages (id, position, title, created_by, created_at)
VALUES (
    '00000000-0000-0000-0001-000000000003',
    coalesce((SELECT max(position) FROM pages WHERE parent_id IS NULL), 0) + 1048576,
    'Project briefs',
    '00000000-0000-0000-0000-000000000001',
    now()
);
INSERT INTO page_versions (page_id, version, title, body, created_by, created_at)
SELECT id, 1, title, body, created_by, created_at
FROM pages
WHERE id = '00000000-0000-0000-0001-000000000003';
INSERT INTO page_events (id, page_id, kind, created_by, data, created_at)
SELECT uuidv7(), id, 'created', created_by, jsonb_build_object('parentId', NULL, 'version', 1),
       created_at
FROM pages
WHERE id = '00000000-0000-0000-0001-000000000003';

-- The project whose brief a page is. A brief holds its place while its project exists, since its
-- Project Manager and the goal check find it by this column; deleting the project leaves it an
-- ordinary page, as every page linked to a project stays. One brief per project.
ALTER TABLE pages
    ADD COLUMN brief_project_id UUID UNIQUE REFERENCES projects (id) ON DELETE SET NULL;
-- A brief is an ordinary page in every other way: no learning, and agents edit it directly.
ALTER TABLE pages ADD CONSTRAINT pages_brief_is_a_plain_page CHECK (
    brief_project_id IS NULL OR (category IS NULL AND NOT proposal_only)
);

-- A brief for every project there is, under Project briefs in the order they were created, as
-- creating a project makes one (crate::models::project_brief), with the same skeleton word for
-- word. It is the system user's, since Elysium made it. It enters the knowledge index the first
-- time someone publishes it, when it says something of its own.
CREATE TEMPORARY TABLE new_briefs AS
SELECT uuidv7() AS page_id,
       project.id AS project_id,
       project.name AS project_name,
       row_number() OVER (ORDER BY project.created_at, project.id) * 1048576 AS position
FROM projects AS project;

INSERT INTO pages (
    id, parent_id, position, title, body, brief_project_id, created_by, created_at
)
SELECT page_id,
       '00000000-0000-0000-0001-000000000003',
       position,
       project_name || ' brief',
       E'## Goals\n\n'
           || 'Each goal is a heading of its own under this one, with what it means and who '
           || E'asked for it beneath it. Every work batch names the goal it serves.\n\n'
           || E'## Decisions\n\n'
           || 'What has been decided about the project, by whom, and when. A work batch a person '
           || E'rejected is recorded here with their reason.\n\n'
           || E'## Out of scope\n\n'
           || 'What the project does not do, so no work batch does it.',
       project_id,
       '00000000-0000-0000-0000-000000000001',
       now()
FROM new_briefs;

INSERT INTO page_versions (page_id, version, title, body, created_by, created_at)
SELECT pages.id, 1, pages.title, pages.body, pages.created_by, pages.created_at
FROM pages
JOIN new_briefs ON new_briefs.page_id = pages.id;

INSERT INTO page_events (id, page_id, kind, created_by, data, created_at)
SELECT uuidv7(), page_id, 'created', '00000000-0000-0000-0000-000000000001',
       jsonb_build_object('parentId', '00000000-0000-0000-0001-000000000003', 'version', 1), now()
FROM new_briefs;

INSERT INTO page_projects (page_id, project_id, created_by, created_at)
SELECT page_id, project_id, '00000000-0000-0000-0000-000000000001', now()
FROM new_briefs;

INSERT INTO page_events (id, page_id, kind, created_by, data, created_at)
SELECT uuidv7(), page_id, 'project_linked', '00000000-0000-0000-0000-000000000001',
       jsonb_build_object('projectId', project_id, 'projectName', project_name), now()
FROM new_briefs;

DROP TABLE new_briefs;

-- A brief published by anyone but its own Project Manager or Elysium's system user wakes that
-- Project Manager, naming the version that woke it. The kind type is rebuilt rather than extended,
-- because a value added with ADD VALUE cannot be used in the same transaction, and the constraint
-- below names it.
ALTER TABLE project_manager_wakes DROP CONSTRAINT project_manager_wakes_kind_fields;
ALTER TYPE project_manager_wake_kind RENAME TO project_manager_wake_kind_without_brief;
CREATE TYPE project_manager_wake_kind AS ENUM (
    -- A record was attributed to the project: a meeting's segment, a conversation's message,
    -- or a record a person put on it.
    'record',
    -- A batch it submitted finished.
    'batch',
    -- A person or Elysia wrote to it.
    'message',
    -- A promise was found on the project, or the work it waits on finished.
    'promise',
    -- Someone else published the project's brief.
    'brief'
);
ALTER TABLE project_manager_wakes
    ALTER COLUMN kind TYPE project_manager_wake_kind
    USING kind::text::project_manager_wake_kind;
DROP TYPE project_manager_wake_kind_without_brief;

-- The version of the brief a publish made. Each publish is a wake of its own, so a turn names
-- everyone who edited the brief since the last.
ALTER TABLE project_manager_wakes
    ADD COLUMN page_version INTEGER CHECK (page_version >= 1);
ALTER TABLE project_manager_wakes ADD CONSTRAINT project_manager_wakes_kind_fields CHECK (
    (timeline_record_id IS NOT NULL) = (kind = 'record')
    AND (work_batch_id IS NOT NULL) = (kind = 'batch')
    AND (body IS NOT NULL) = (kind = 'message')
    AND (promise_id IS NOT NULL) = (kind = 'promise')
    AND (page_version IS NOT NULL) = (kind = 'brief')
);

-- The goal in its project's brief a batch serves, as the brief's heading spelled it when the batch
-- was submitted. Every batch a Project Manager submits names one. Those from before briefs name
-- none, and so does one a person made by accepting a finding, since their acceptance is the
-- decision the goal check would have asked them for.
ALTER TABLE work_batches
    ADD COLUMN serves TEXT CHECK (char_length(serves) BETWEEN 1 AND 300);
