-- A person deletes their meetings, one, many, or with the connector that read them. Deleting is
-- soft: nothing is deleted for good yet, so restoring brings back all of it. See docs/meetings.md,
-- Deleting.

-- When the meeting was deleted, while it is. A deleted meeting is listed nowhere, processed no
-- further, and changes no further until it is restored. Its captures stay, which is what keeps its
-- connector from storing it again (`meeting_captures_once_per_connector`), and so do its
-- recording and clips in storage. Who deleted and restored it is in its history
-- (`meeting_events`, kinds `deleted` and `restored`).
ALTER TABLE meetings ADD COLUMN deleted_at TIMESTAMPTZ;

-- When the record was deleted with what it came from, while it is: today a meeting's segment,
-- taken with its meeting and stamped with the meeting's own `deleted_at`, so restoring the meeting
-- brings back exactly the records its deletion took. A deleted record is on no timeline and out of
-- the knowledge index, and keeps its attributions and their history for its return.
ALTER TABLE timeline_records ADD COLUMN deleted_at TIMESTAMPTZ;
