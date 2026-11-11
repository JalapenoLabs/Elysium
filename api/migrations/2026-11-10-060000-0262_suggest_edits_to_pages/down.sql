-- Hands the company doc its proposals back, as they stood before suggestions were every page's:
-- every suggestion to a section, pending and decided, under its own id. Suggestions to any other
-- page, which the old shape cannot hold, go, and so do the reasons dismissals gave and which changes
-- an acceptance took, which it has no column for.

CREATE TABLE company_doc_proposals (
    id UUID PRIMARY KEY,
    section TEXT NOT NULL,
    base_content TEXT NOT NULL CHECK (char_length(base_content) <= 12000),
    content TEXT NOT NULL CHECK (char_length(content) <= 12000),
    reason TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 2000),
    quote TEXT CHECK (char_length(quote) BETWEEN 1 AND 2000),
    source TEXT CHECK (char_length(source) BETWEEN 1 AND 500),
    timeline_record_id UUID REFERENCES timeline_records (id) ON DELETE SET NULL,
    workflow_run_id UUID REFERENCES workflow_runs (id) ON DELETE SET NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'approved', 'rejected')),
    decided_by UUID REFERENCES users (id),
    decided_at TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users (id),
    agent TEXT,
    agent_name TEXT CHECK (char_length(agent_name) BETWEEN 1 AND 200),
    created_at TIMESTAMPTZ NOT NULL,

    CONSTRAINT company_doc_proposals_agent_check CHECK (
        agent ~ '^(mcp:[^[:space:]]{1,255}|session:[0-9]{1,18}|project_manager:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$'
    ),
    CONSTRAINT company_doc_proposals_changes_something CHECK (content <> base_content),
    CONSTRAINT company_doc_proposals_decided_once CHECK (
        (state = 'pending') = (decided_by IS NULL) AND (decided_by IS NULL) = (decided_at IS NULL)
    ),
    CONSTRAINT company_doc_proposals_agent_named_with_its_agent CHECK (
        agent IS NOT NULL OR agent_name IS NULL
    ),
    CONSTRAINT company_doc_proposals_section_known CHECK (
        section IN (
            'company', 'leadership', 'people', 'stakeholders', 'products', 'projects', 'glossary'
        )
    )
);

CREATE UNIQUE INDEX company_doc_proposals_pending_once
    ON company_doc_proposals (section, md5(content))
    WHERE state = 'pending';

CREATE INDEX company_doc_proposals_by_state ON company_doc_proposals (state, id);

INSERT INTO company_doc_proposals (
    id, section, base_content, content, reason, quote, source, timeline_record_id,
    workflow_run_id, state, decided_by, decided_at, created_by, agent, agent_name, created_at
)
SELECT suggestions.id, sections.section, suggestions.base_body, suggestions.body,
       suggestions.reason, suggestions.quote, suggestions.source, suggestions.timeline_record_id,
       suggestions.workflow_run_id,
       CASE suggestions.state
           WHEN 'accepted' THEN 'approved'
           WHEN 'dismissed' THEN 'rejected'
           ELSE 'pending'
       END,
       suggestions.decided_by, suggestions.decided_at, suggestions.created_by, suggestions.agent,
       suggestions.agent_name, suggestions.created_at
FROM page_suggestions AS suggestions
JOIN (VALUES
    ('company', '00000000-0000-0000-0002-000000000001'::uuid),
    ('leadership', '00000000-0000-0000-0002-000000000002'::uuid),
    ('people', '00000000-0000-0000-0002-000000000003'::uuid),
    ('stakeholders', '00000000-0000-0000-0002-000000000004'::uuid),
    ('products', '00000000-0000-0000-0002-000000000005'::uuid),
    ('projects', '00000000-0000-0000-0002-000000000006'::uuid),
    ('glossary', '00000000-0000-0000-0002-000000000007'::uuid)
) AS sections (section, page_id) ON sections.page_id = suggestions.page_id;

-- A section's version an accepted suggestion wrote names it as the proposal it was. Any other page's
-- names a suggestion that no longer exists, so it names nothing.
UPDATE page_events
SET data = (data - 'suggestionId') || jsonb_build_object('proposalId', data -> 'suggestionId')
WHERE data ? 'suggestionId'
  AND page_id IN (
      '00000000-0000-0000-0002-000000000001', '00000000-0000-0000-0002-000000000002',
      '00000000-0000-0000-0002-000000000003', '00000000-0000-0000-0002-000000000004',
      '00000000-0000-0000-0002-000000000005', '00000000-0000-0000-0002-000000000006',
      '00000000-0000-0000-0002-000000000007'
  );

UPDATE page_events SET data = data - 'suggestionId' WHERE data ? 'suggestionId';

DROP TABLE page_suggestion_items;
DROP TABLE page_suggestions;
