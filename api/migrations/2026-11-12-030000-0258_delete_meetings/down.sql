DROP INDEX timeline_records_by_meeting;
ALTER TABLE timeline_records DROP COLUMN deleted_at;
ALTER TABLE meetings DROP COLUMN deleted_at;
