DROP INDEX knowledge_documents_of_access_meeting;
ALTER TABLE knowledge_documents
    DROP CONSTRAINT knowledge_documents_transcript_access,
    DROP COLUMN access_meeting_id;

DROP TABLE meeting_shares;

DROP INDEX meetings_visibility_items;
ALTER TABLE meetings
    DROP COLUMN visibility_item_id,
    DROP COLUMN visibility;
