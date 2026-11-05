-- Hands learnings and the company doc back to their own tables, exactly as they stood: every id,
-- timestamp, author, agent, and history entry, with each revision's `changes` read back from the
-- versions it lay between. Whatever was written since the migration comes along, shaped as those
-- tables shape it; a page's moves have no place there, and are dropped with the page.

ALTER TABLE pages
    DROP CONSTRAINT pages_learning_body_fits,
    DROP CONSTRAINT pages_company_doc_section_fits;
DROP INDEX pages_learning_title_unique;

CREATE TABLE learnings (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 300),
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 20000),
    category TEXT NOT NULL,
    evidence TEXT NOT NULL CHECK (char_length(evidence) BETWEEN 1 AND 20000),
    agent TEXT CHECK (agent ~ '^mcp:[^[:space:]]{1,255}$'),
    agent_name TEXT CHECK (char_length(agent_name) BETWEEN 1 AND 200),
    deleted_at TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT learnings_agent_named_with_its_agent CHECK (agent IS NOT NULL OR agent_name IS NULL),
    CONSTRAINT learnings_category_fkey FOREIGN KEY (category) REFERENCES learning_categories (key)
);

SELECT diesel_manage_updated_at('learnings');

CREATE UNIQUE INDEX learnings_title_unique ON learnings (lower(title));
CREATE INDEX learnings_category_idx ON learnings (category);

CREATE TABLE learning_events (
    id UUID PRIMARY KEY,
    learning_id UUID NOT NULL REFERENCES learnings (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z_]{0,63}$'),
    created_by UUID NOT NULL REFERENCES users (id),
    agent TEXT CHECK (agent ~ '^mcp:[^[:space:]]{1,255}$'),
    agent_name TEXT CHECK (char_length(agent_name) BETWEEN 1 AND 200),
    data JSONB NOT NULL CHECK (jsonb_typeof(data) = 'object'),
    created_at TIMESTAMPTZ NOT NULL,

    CONSTRAINT learning_events_agent_named_with_its_agent CHECK (
        agent IS NOT NULL OR agent_name IS NULL
    )
);

CREATE INDEX learning_events_learning_id_idx ON learning_events (learning_id, id);

CREATE TABLE company_doc_sections (
    section TEXT PRIMARY KEY CHECK (
        section IN (
            'company', 'leadership', 'people', 'stakeholders', 'products', 'projects', 'glossary'
        )
    ),
    content TEXT NOT NULL DEFAULT '' CHECK (char_length(content) <= 12000),
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('company_doc_sections');

CREATE TABLE company_doc_events (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z_]{0,63}$'),
    created_by UUID NOT NULL REFERENCES users (id),
    proposal_id UUID REFERENCES company_doc_proposals (id),
    data JSONB NOT NULL CHECK (jsonb_typeof(data) = 'object'),
    created_at TIMESTAMPTZ NOT NULL,

    CONSTRAINT company_doc_events_approval_names_its_proposal CHECK (
        (kind = 'approved') = (proposal_id IS NOT NULL)
    )
);

-- Learnings' table names only MCP clients as agents, the one kind that could save a learning, so
-- an agent a later release recorded on a page falls away here, leaving the person it acted for.
INSERT INTO learnings (
    id, title, body, category, evidence, agent, agent_name, deleted_at, created_by, created_at,
    updated_at
)
SELECT pages.id, pages.title, pages.body, pages.category, pages.evidence,
       CASE WHEN first.agent LIKE 'mcp:%' THEN first.agent END,
       CASE WHEN first.agent LIKE 'mcp:%' THEN first.agent_name END,
       pages.deleted_at, pages.created_by, pages.created_at, pages.updated_at
FROM pages
JOIN page_versions AS first ON first.page_id = pages.id AND first.version = 1
WHERE pages.category IS NOT NULL;

INSERT INTO learning_events (id, learning_id, kind, created_by, agent, agent_name, data, created_at)
SELECT events.id, events.page_id,
       CASE events.kind WHEN 'published' THEN 'revised' ELSE events.kind END,
       events.created_by,
       CASE WHEN events.agent LIKE 'mcp:%' THEN events.agent END,
       CASE WHEN events.agent LIKE 'mcp:%' THEN events.agent_name END,
       CASE events.kind
           WHEN 'created' THEN (
               SELECT jsonb_build_object(
                   'title', first.title, 'body', first.body,
                   'category', first.category, 'evidence', first.evidence
               )
               FROM page_versions AS first
               WHERE first.page_id = events.page_id AND first.version = 1
           )
           WHEN 'published' THEN jsonb_strip_nulls(jsonb_build_object(
               'changes', (
                   SELECT jsonb_object_agg(
                       changed.field,
                       jsonb_build_object('from', changed.before, 'to', changed.after)
                   )
                   FROM page_versions AS earlier
                   JOIN page_versions AS later
                       ON later.page_id = earlier.page_id AND later.version = earlier.version + 1
                   CROSS JOIN LATERAL (VALUES
                       ('title', earlier.title, later.title),
                       ('body', earlier.body, later.body),
                       ('category', earlier.category, later.category),
                       ('evidence', earlier.evidence, later.evidence)
                   ) AS changed (field, before, after)
                   WHERE later.page_id = events.page_id
                     AND later.version = (events.data ->> 'version')::integer
                     AND changed.before IS DISTINCT FROM changed.after
               ),
               'undid', events.data -> 'undid'
           ))
           -- Being taken with an ancestor is not something a learning's history can say.
           ELSE events.data - 'deletedWith' - 'restoredWith'
       END,
       events.created_at
FROM page_events AS events
JOIN pages ON pages.id = events.page_id
WHERE pages.category IS NOT NULL
  AND events.kind IN ('created', 'published', 'deleted', 'restored');

INSERT INTO company_doc_sections (section, content, created_by, created_at, updated_at)
SELECT keys.section, pages.body, pages.created_by, pages.created_at, pages.updated_at
FROM pages
JOIN (VALUES
    ('company', '00000000-0000-0000-0002-000000000001'::uuid),
    ('leadership', '00000000-0000-0000-0002-000000000002'::uuid),
    ('people', '00000000-0000-0000-0002-000000000003'::uuid),
    ('stakeholders', '00000000-0000-0000-0002-000000000004'::uuid),
    ('products', '00000000-0000-0000-0002-000000000005'::uuid),
    ('projects', '00000000-0000-0000-0002-000000000006'::uuid),
    ('glossary', '00000000-0000-0000-0002-000000000007'::uuid)
) AS keys (section, id) ON keys.id = pages.id;

-- Each change's rows, one per page it reached, become its one entry again.
INSERT INTO company_doc_events (id, kind, created_by, proposal_id, data, created_at)
SELECT events.id,
       CASE WHEN bool_or(events.data ? 'proposalId') THEN 'approved' ELSE 'edited' END,
       (array_agg(events.created_by))[1],
       (array_agg((events.data ->> 'proposalId')::uuid))[1],
       jsonb_strip_nulls(jsonb_build_object(
           'changes', jsonb_object_agg(
               sections.section,
               jsonb_build_object('from', earlier.body, 'to', later.body)
           ),
           'undid', (array_agg(events.data -> 'undid'))[1]
       )),
       min(events.created_at)
FROM page_events AS events
JOIN (VALUES
    ('company', '00000000-0000-0000-0002-000000000001'::uuid),
    ('leadership', '00000000-0000-0000-0002-000000000002'::uuid),
    ('people', '00000000-0000-0000-0002-000000000003'::uuid),
    ('stakeholders', '00000000-0000-0000-0002-000000000004'::uuid),
    ('products', '00000000-0000-0000-0002-000000000005'::uuid),
    ('projects', '00000000-0000-0000-0002-000000000006'::uuid),
    ('glossary', '00000000-0000-0000-0002-000000000007'::uuid)
) AS sections (section, id) ON sections.id = events.page_id
JOIN page_versions AS later
    ON later.page_id = events.page_id
   AND later.version = (events.data ->> 'version')::integer
JOIN page_versions AS earlier
    ON earlier.page_id = events.page_id AND earlier.version = later.version - 1
WHERE events.kind = 'published'
GROUP BY events.id;

-- Documents go back to their sources' columns, again leaving everything else about them alone. A
-- document of the pages that stay is left on its page, and those of the Learnings and Company pages
-- themselves go with them.
ALTER TABLE knowledge_documents
    ADD COLUMN learning_id UUID UNIQUE REFERENCES learnings (id) ON DELETE CASCADE,
    ADD COLUMN company_doc_section TEXT UNIQUE
        REFERENCES company_doc_sections (section) ON DELETE CASCADE;

ALTER TABLE knowledge_documents DROP CONSTRAINT knowledge_documents_one_source;
ALTER TABLE knowledge_documents DISABLE TRIGGER set_updated_at;

UPDATE knowledge_documents AS documents
SET learning_id = documents.page_id, page_id = NULL
FROM pages
WHERE pages.id = documents.page_id AND pages.category IS NOT NULL;

UPDATE knowledge_documents AS documents
SET company_doc_section = keys.section, page_id = NULL
FROM (VALUES
    ('company', '00000000-0000-0000-0002-000000000001'::uuid),
    ('leadership', '00000000-0000-0000-0002-000000000002'::uuid),
    ('people', '00000000-0000-0000-0002-000000000003'::uuid),
    ('stakeholders', '00000000-0000-0000-0002-000000000004'::uuid),
    ('products', '00000000-0000-0000-0002-000000000005'::uuid),
    ('projects', '00000000-0000-0000-0002-000000000006'::uuid),
    ('glossary', '00000000-0000-0000-0002-000000000007'::uuid)
) AS keys (section, id)
WHERE documents.page_id = keys.id;

ALTER TABLE knowledge_documents ENABLE TRIGGER set_updated_at;
ALTER TABLE knowledge_documents ADD CONSTRAINT knowledge_documents_one_source
    CHECK (num_nonnulls(timeline_record_id, learning_id, company_doc_section, page_id) = 1);

ALTER TABLE company_doc_proposals
    DROP CONSTRAINT company_doc_proposals_section_known,
    ADD CONSTRAINT company_doc_proposals_section_fkey
        FOREIGN KEY (section) REFERENCES company_doc_sections (section);

-- People may have put pages of their own under the pages leaving, which go to the top of the tree,
-- after what is there.
WITH leaving AS (
    SELECT id FROM pages
    WHERE category IS NOT NULL
       OR id IN (
           '00000000-0000-0000-0001-000000000001',
           '00000000-0000-0000-0001-000000000002',
           '00000000-0000-0000-0002-000000000001',
           '00000000-0000-0000-0002-000000000002',
           '00000000-0000-0000-0002-000000000003',
           '00000000-0000-0000-0002-000000000004',
           '00000000-0000-0000-0002-000000000005',
           '00000000-0000-0000-0002-000000000006',
           '00000000-0000-0000-0002-000000000007'
       )
),
stranded AS (
    SELECT pages.id, row_number() OVER (ORDER BY pages.parent_id, pages.position) AS place
    FROM pages
    WHERE pages.parent_id IN (SELECT id FROM leaving)
      AND pages.id NOT IN (SELECT id FROM leaving)
)
UPDATE pages
SET parent_id = NULL,
    position = (
        SELECT coalesce(max(position), 0) FROM pages WHERE parent_id IS NULL
    ) + stranded.place * 1048576
FROM stranded
WHERE pages.id = stranded.id;

-- Their versions and history cascade.
DELETE FROM pages
WHERE category IS NOT NULL
   OR id IN (
       '00000000-0000-0000-0001-000000000001',
       '00000000-0000-0000-0001-000000000002',
       '00000000-0000-0000-0002-000000000001',
       '00000000-0000-0000-0002-000000000002',
       '00000000-0000-0000-0002-000000000003',
       '00000000-0000-0000-0002-000000000004',
       '00000000-0000-0000-0002-000000000005',
       '00000000-0000-0000-0002-000000000006',
       '00000000-0000-0000-0002-000000000007'
   );

-- Only the company doc's changes shared an id across pages, so this holds again.
ALTER TABLE page_events DROP CONSTRAINT page_events_pkey;
DROP INDEX page_events_id;
ALTER TABLE page_events ADD PRIMARY KEY (id);
CREATE INDEX page_events_page_id_idx ON page_events (page_id, id);
