-- Learnings and the company doc become pages, so one tree, one history, and one index carry
-- everything authored. Every learning moves under a Learnings page, keeping its id, and each
-- company doc section under a Company page; their revisions become versions, and their entries
-- page history. See docs/knowledge.md, Pages.
--
-- Every id here is a fixed value the API names too (crate::models::page, and Section::page_id in
-- crate::models::company_doc), as the machine users' are, so the pages Elysium relies on are found
-- without a lookup and down.sql hands back what it was given.

-- One change can reach several pages: a person saving the company doc changes each section they
-- touched, as one change with one Undo. It is one id with a row per page, so an entry is named by
-- its page and its id.
ALTER TABLE page_events DROP CONSTRAINT page_events_pkey;
DROP INDEX page_events_page_id_idx;
ALTER TABLE page_events ADD PRIMARY KEY (page_id, id);
-- Reads a feed across pages in order, and finds a change's rows.
CREATE INDEX page_events_id ON page_events (id);

-- The two pages at the top that hold what moves here, after whatever the tree already has.
INSERT INTO pages (id, position, title, proposal_only, created_by, created_at)
SELECT root.id,
       coalesce((SELECT max(position) FROM pages WHERE parent_id IS NULL), 0)
           + root.place * 1048576,
       root.title,
       root.proposal_only,
       '00000000-0000-0000-0000-000000000001',
       now()
FROM (VALUES
    ('00000000-0000-0000-0001-000000000001'::uuid, 1, 'Learnings', FALSE),
    ('00000000-0000-0000-0001-000000000002'::uuid, 2, 'Company', TRUE)
) AS root (id, place, title, proposal_only);

INSERT INTO page_versions (page_id, version, title, body, created_by, created_at)
SELECT id, 1, title, body, created_by, created_at
FROM pages
WHERE id IN (
    '00000000-0000-0000-0001-000000000001',
    '00000000-0000-0000-0001-000000000002'
);

INSERT INTO page_events (id, page_id, kind, created_by, data, created_at)
SELECT uuidv7(), id, 'created', created_by, jsonb_build_object('parentId', NULL, 'version', 1), created_at
FROM pages
WHERE id IN (
    '00000000-0000-0000-0001-000000000001',
    '00000000-0000-0000-0001-000000000002'
);

-- Each learning, replayed from its history: version 1 is what its first save wrote, and each
-- revision the next version, by whoever made it. The entry says what it replaced, and the replay
-- refuses to go on when that is not what the learning held, or when it does not end on the row:
-- the history would be wrong, and nothing here should guess at it. The migration is one
-- transaction, so a refusal changes nothing.
DO $$
DECLARE
    learning learnings%ROWTYPE;
    entry learning_events%ROWTYPE;
    fields JSONB;
    version_number INTEGER;
    place BIGINT := 0;
BEGIN
    FOR learning IN SELECT * FROM learnings ORDER BY created_at, id LOOP
        place := place + 1;
        INSERT INTO pages (
            id, parent_id, position, title, body, category, evidence, current_version,
            deleted_at, created_by, created_at, updated_at
        )
        SELECT learning.id, '00000000-0000-0000-0001-000000000001', place * 1048576,
               learning.title, learning.body, learning.category, learning.evidence,
               1 + count(*), learning.deleted_at, learning.created_by, learning.created_at,
               learning.updated_at
        FROM learning_events
        WHERE learning_id = learning.id AND kind = 'revised';

        SELECT * INTO entry FROM learning_events WHERE learning_id = learning.id AND kind = 'created';
        IF NOT FOUND THEN
            RAISE EXCEPTION 'learning % has no created entry in its history', learning.id;
        END IF;
        fields := entry.data;
        version_number := 1;
        INSERT INTO page_versions (
            page_id, version, title, body, category, evidence, created_by, agent, agent_name,
            created_at
        ) VALUES (
            learning.id, 1, fields ->> 'title', fields ->> 'body', fields ->> 'category',
            fields ->> 'evidence', learning.created_by, learning.agent, learning.agent_name,
            learning.created_at
        );

        FOR entry IN
            SELECT * FROM learning_events
            WHERE learning_id = learning.id AND kind = 'revised'
            ORDER BY id
        LOOP
            IF EXISTS (
                SELECT FROM jsonb_each(entry.data -> 'changes') AS change (field, value)
                WHERE fields -> change.field IS DISTINCT FROM change.value -> 'from'
            ) THEN
                RAISE EXCEPTION 'entry % of learning % replaces what it did not hold', entry.id, learning.id;
            END IF;
            SELECT fields || jsonb_object_agg(change.field, change.value -> 'to')
            INTO fields
            FROM jsonb_each(entry.data -> 'changes') AS change (field, value);

            version_number := version_number + 1;
            INSERT INTO page_versions (
                page_id, version, title, body, category, evidence, created_by, agent, agent_name,
                created_at
            ) VALUES (
                learning.id, version_number, fields ->> 'title', fields ->> 'body',
                fields ->> 'category', fields ->> 'evidence', entry.created_by, entry.agent,
                entry.agent_name, entry.created_at
            );
            INSERT INTO page_events (id, page_id, kind, created_by, agent, agent_name, data, created_at)
            VALUES (
                entry.id, learning.id, 'published', entry.created_by, entry.agent, entry.agent_name,
                jsonb_strip_nulls(jsonb_build_object(
                    'version', version_number, 'undid', entry.data -> 'undid'
                )),
                entry.created_at
            );
        END LOOP;

        IF fields IS DISTINCT FROM jsonb_build_object(
            'title', learning.title, 'body', learning.body,
            'category', learning.category, 'evidence', learning.evidence
        ) THEN
            RAISE EXCEPTION 'the history of learning % does not end on what it says', learning.id;
        END IF;
    END LOOP;
END $$;

-- Everything else a learning's history holds. A created entry says only where the page went, since
-- version 1 holds what it said; a deletion and a restore mean what they meant.
INSERT INTO page_events (id, page_id, kind, created_by, agent, agent_name, data, created_at)
SELECT id, learning_id, kind, created_by, agent, agent_name,
       CASE kind
           WHEN 'created' THEN jsonb_build_object(
               'parentId', '00000000-0000-0000-0001-000000000001', 'version', 1
           )
           ELSE data
       END,
       created_at
FROM learning_events
WHERE kind <> 'revised';

-- Each section, replayed the same way from the doc's history, under the Company page in the doc's
-- order. Version 1 is the empty section its migration seeded, and each change that touched the
-- section the next version. An entry that changed three sections becomes one row on each of their
-- pages under the entry's own id, which is what keeps it one change with one Undo.
DO $$
DECLARE
    section RECORD;
    entry company_doc_events%ROWTYPE;
    written TEXT;
    version_number INTEGER;
BEGIN
    FOR section IN
        SELECT sections.*, keys.id AS page_id, keys.place, keys.title
        FROM company_doc_sections AS sections
        JOIN (VALUES
            ('company', '00000000-0000-0000-0002-000000000001'::uuid, 1, 'Company'),
            ('leadership', '00000000-0000-0000-0002-000000000002'::uuid, 2, 'Leadership'),
            ('people', '00000000-0000-0000-0002-000000000003'::uuid, 3, 'People'),
            ('stakeholders', '00000000-0000-0000-0002-000000000004'::uuid, 4, 'Stakeholders'),
            ('products', '00000000-0000-0000-0002-000000000005'::uuid, 5, 'Products'),
            ('projects', '00000000-0000-0000-0002-000000000006'::uuid, 6, 'Projects'),
            ('glossary', '00000000-0000-0000-0002-000000000007'::uuid, 7, 'Glossary')
        ) AS keys (section, id, place, title) ON keys.section = sections.section
        ORDER BY keys.place
    LOOP
        INSERT INTO pages (
            id, parent_id, position, title, body, current_version, proposal_only, created_by,
            created_at, updated_at
        )
        SELECT section.page_id, '00000000-0000-0000-0001-000000000002', section.place * 1048576,
               section.title, section.content, 1 + count(*), TRUE, section.created_by,
               section.created_at, section.updated_at
        FROM company_doc_events
        WHERE data -> 'changes' ? section.section;

        INSERT INTO page_versions (page_id, version, title, body, created_by, created_at)
        VALUES (section.page_id, 1, section.title, '', section.created_by, section.created_at);
        -- Dated as the section was, so it reads before every change made since.
        INSERT INTO page_events (id, page_id, kind, created_by, data, created_at)
        VALUES (
            uuidv7(section.created_at - now()), section.page_id, 'created', section.created_by,
            jsonb_build_object('parentId', '00000000-0000-0000-0001-000000000002', 'version', 1),
            section.created_at
        );

        written := '';
        version_number := 1;
        FOR entry IN
            SELECT * FROM company_doc_events
            WHERE data -> 'changes' ? section.section
            ORDER BY id
        LOOP
            IF entry.data -> 'changes' -> section.section ->> 'from' IS DISTINCT FROM written THEN
                RAISE EXCEPTION 'entry % replaces what the % section did not hold', entry.id, section.section;
            END IF;
            written := entry.data -> 'changes' -> section.section ->> 'to';
            version_number := version_number + 1;
            INSERT INTO page_versions (page_id, version, title, body, created_by, created_at)
            VALUES (
                section.page_id, version_number, section.title, written, entry.created_by,
                entry.created_at
            );
            INSERT INTO page_events (id, page_id, kind, created_by, data, created_at)
            VALUES (
                entry.id, section.page_id, 'published', entry.created_by,
                jsonb_strip_nulls(jsonb_build_object(
                    'version', version_number,
                    'undid', entry.data -> 'undid',
                    'proposalId', entry.proposal_id
                )),
                entry.created_at
            );
        END LOOP;

        IF written IS DISTINCT FROM section.content THEN
            RAISE EXCEPTION 'the history of the % section does not end on what it says', section.section;
        END IF;
    END LOOP;
END $$;

-- Documents move to their page's column and keep their rows, so their passages keep their
-- embeddings; `source` keeps its name, so the search's filter finds them as it did. The trigger is
-- held off because nothing about a document changes here but the column naming its source.
ALTER TABLE knowledge_documents DISABLE TRIGGER set_updated_at;
ALTER TABLE knowledge_documents DROP CONSTRAINT knowledge_documents_one_source;

UPDATE knowledge_documents
SET page_id = learning_id, learning_id = NULL
WHERE learning_id IS NOT NULL;

UPDATE knowledge_documents AS documents
SET page_id = keys.id, company_doc_section = NULL
FROM (VALUES
    ('company', '00000000-0000-0000-0002-000000000001'::uuid),
    ('leadership', '00000000-0000-0000-0002-000000000002'::uuid),
    ('people', '00000000-0000-0000-0002-000000000003'::uuid),
    ('stakeholders', '00000000-0000-0000-0002-000000000004'::uuid),
    ('products', '00000000-0000-0000-0002-000000000005'::uuid),
    ('projects', '00000000-0000-0000-0002-000000000006'::uuid),
    ('glossary', '00000000-0000-0000-0002-000000000007'::uuid)
) AS keys (section, id)
WHERE documents.company_doc_section = keys.section;

ALTER TABLE knowledge_documents ENABLE TRIGGER set_updated_at;
ALTER TABLE knowledge_documents
    DROP COLUMN learning_id,
    DROP COLUMN company_doc_section;
ALTER TABLE knowledge_documents ADD CONSTRAINT knowledge_documents_one_source
    CHECK (num_nonnulls(timeline_record_id, page_id) = 1);

-- A proposal still names its section by key. Pending and decided ones alike stay as they are.
ALTER TABLE company_doc_proposals
    DROP CONSTRAINT company_doc_proposals_section_fkey,
    ADD CONSTRAINT company_doc_proposals_section_known CHECK (
        section IN (
            'company', 'leadership', 'people', 'stakeholders', 'products', 'projects', 'glossary'
        )
    );

DROP TABLE learning_events;
DROP TABLE learnings;
DROP TABLE company_doc_events;
DROP TABLE company_doc_sections;

-- Saving the same title again revises that learning, so a title names one learning, whatever its
-- case. Deleted learnings are included: saving a deleted learning's title brings it back revised,
-- and restoring one never meets a live twin. A page that is no learning may share any title.
-- lower() follows the database's collation, whose changes `migrate run` rebuilds indexes for
-- (docs/database.md, Collations).
CREATE UNIQUE INDEX pages_learning_title_unique ON pages (lower(title)) WHERE category IS NOT NULL;

-- The bounds each kept before they were pages. A section's keeps the whole doc inside the review's
-- one prompt (crate::company_doc::review, Size).
ALTER TABLE pages
    ADD CONSTRAINT pages_learning_body_fits CHECK (
        category IS NULL OR char_length(body) BETWEEN 1 AND 20000
    ),
    ADD CONSTRAINT pages_company_doc_section_fits CHECK (
        id NOT IN (
            '00000000-0000-0000-0002-000000000001',
            '00000000-0000-0000-0002-000000000002',
            '00000000-0000-0000-0002-000000000003',
            '00000000-0000-0000-0002-000000000004',
            '00000000-0000-0000-0002-000000000005',
            '00000000-0000-0000-0002-000000000006',
            '00000000-0000-0000-0002-000000000007'
        ) OR char_length(body) <= 12000
    );
