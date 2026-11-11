-- Every capture of a meeting goes on the meeting's clock, and a meeting that two captures turn out
-- to share is merged only when its person says so. See docs/meeting-review.md and docs/meetings.md.

-- A capture's place among its meeting's captures is when it joined the meeting: the one the meeting
-- was listed with comes first and is the clock every other capture is aligned against, so a capture
-- merged in later never takes its place, however old it is.
ALTER TABLE meeting_captures
    ADD COLUMN joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- How its offset was found: from when its source said it started (`listed`), by
    -- cross-correlating its audio against the first capture's (`audio`), by matching its words and
    -- times against the meeting's (`words`), or set by a person (`hand`), which aligning never
    -- changes.
    ADD COLUMN offset_source TEXT NOT NULL DEFAULT 'listed'
        CHECK (offset_source IN ('listed', 'audio', 'words', 'hand'));

-- Every capture joined its meeting as it was listed. Its updated_at stays as it was: nothing about
-- the capture changed.
ALTER TABLE meeting_captures DISABLE TRIGGER set_updated_at;
UPDATE meeting_captures SET joined_at = created_at;
ALTER TABLE meeting_captures ENABLE TRIGGER set_updated_at;

DROP INDEX meeting_captures_of_meeting;
CREATE INDEX meeting_captures_of_meeting ON meeting_captures (meeting_id, joined_at, id);

-- A meeting whose captures were merged into another names it. It holds no captures while it does,
-- and is listed nowhere; splitting the captures back out clears it.
ALTER TABLE meetings
    ADD COLUMN merged_into UUID REFERENCES meetings (id) ON DELETE SET NULL,
    ADD CONSTRAINT meetings_merged_into_another CHECK (merged_into <> id);

CREATE INDEX meetings_merged_into ON meetings (merged_into) WHERE merged_into IS NOT NULL;

-- Two meetings that look like one: they overlap in time and have participants in common. The one
-- found later (`meeting_id`) is asked about, with an inbox item for its person, "Same meeting as"
-- the other (`same_as_id`). Elysium never merges on its own: the person merges it, or says it is
-- not the same, and either is remembered, so a pair is asked about once.
CREATE TABLE meeting_duplicates (
    id UUID PRIMARY KEY,
    -- The meeting asked about, whose captures a merge moves.
    meeting_id UUID NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    -- The meeting it looks like, which a merge moves them into.
    same_as_id UUID NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    -- The inbox item asking, while it exists.
    action_item_id UUID REFERENCES action_items (id) ON DELETE SET NULL,
    -- NULL until the person decides.
    decision TEXT CHECK (decision IN ('merged', 'not_the_same')),
    decided_by UUID REFERENCES users (id),
    decided_at TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT meeting_duplicates_two_meetings CHECK (meeting_id <> same_as_id),
    CONSTRAINT meeting_duplicates_decided
        CHECK ((decision IS NULL) = (decided_at IS NULL) AND (decision IS NULL) = (decided_by IS NULL))
);

SELECT diesel_manage_updated_at('meeting_duplicates');

-- A pair is asked about once, whichever of the two found the other.
CREATE UNIQUE INDEX meeting_duplicates_once_per_pair ON meeting_duplicates (
    LEAST(meeting_id, same_as_id), GREATEST(meeting_id, same_as_id)
);

CREATE INDEX meeting_duplicates_of_meeting ON meeting_duplicates (meeting_id);
CREATE INDEX meeting_duplicates_same_as ON meeting_duplicates (same_as_id);
CREATE INDEX meeting_duplicates_of_item ON meeting_duplicates (action_item_id)
    WHERE action_item_id IS NOT NULL;
