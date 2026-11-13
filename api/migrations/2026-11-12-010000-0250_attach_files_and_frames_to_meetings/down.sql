-- Uploads of attachments go with what they uploaded, files and all: before this migration an
-- upload only ever made a capture.
DELETE FROM meeting_uploads
WHERE attachment_id IS NOT NULL
   OR id IN (SELECT upload_id FROM meeting_upload_files WHERE kind = 'attachment');

ALTER TABLE meeting_upload_files DROP CONSTRAINT meeting_upload_files_kind_check;
ALTER TABLE meeting_upload_files ADD CONSTRAINT meeting_upload_files_kind_check
    CHECK (kind IN ('recording', 'transcript'));

ALTER TABLE meeting_uploads DROP CONSTRAINT meeting_uploads_attachment_to_a_meeting;
ALTER TABLE meeting_uploads DROP CONSTRAINT meeting_uploads_finished;
ALTER TABLE meeting_uploads DROP COLUMN attachment_id;
ALTER TABLE meeting_uploads ADD CONSTRAINT meeting_uploads_finished
    CHECK ((finished_at IS NULL) = (capture_id IS NULL));

ALTER TABLE workspace_settings DROP COLUMN meeting_attachment_max_bytes;

-- Pins name attachments that are gone with this table.
DELETE FROM meeting_marks WHERE kind = 'pin';

DROP TABLE meeting_attachments;
