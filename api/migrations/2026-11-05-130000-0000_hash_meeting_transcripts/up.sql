-- The SHA-256 of the transcript a meeting's fetch step stored, as lowercase hex: what tells a
-- second fetch of the same meeting that there is nothing to change. NULL until it is fetched.
-- See docs/meetings.md, Fetching.
ALTER TABLE meetings
    ADD COLUMN transcript_hash TEXT CHECK (transcript_hash ~ '^[0-9a-f]{64}$');
