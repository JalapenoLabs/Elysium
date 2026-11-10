-- Filings a person owed by accepting a finding have no proposer the older shape admits, so they
-- go with the findings that owed them; their items stay, without the issue.
DELETE FROM action_item_issue_filings WHERE filed_by LIKE 'user:%';

ALTER TABLE action_item_issue_filings
    DROP CONSTRAINT action_item_issue_filings_filed_by_shape,
    ADD CONSTRAINT action_item_issue_filings_filed_by_shape
        CHECK (filed_by ~ '^(elysia|session:[0-9]+|project_manager:[0-9a-f-]{36})$');

DROP TABLE findings;
DROP TYPE finding_cost;
DROP TYPE finding_state;
