DROP INDEX knowledge_documents_of_meeting;
ALTER TABLE knowledge_documents DROP COLUMN meeting_id;

DROP TABLE meeting_shares;

DROP INDEX meetings_visibility_items;
ALTER TABLE meetings
    DROP COLUMN visibility_item_id,
    DROP COLUMN visibility;
