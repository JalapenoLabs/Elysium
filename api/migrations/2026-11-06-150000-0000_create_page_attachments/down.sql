ALTER TABLE workspace_settings DROP COLUMN page_attachment_storage_location_id;

-- An entry naming an attachment is about a table that no longer exists, and no version before this
-- one reads those kinds.
DELETE FROM page_events WHERE kind IN ('attached', 'detached');

DROP TABLE page_attachments;
