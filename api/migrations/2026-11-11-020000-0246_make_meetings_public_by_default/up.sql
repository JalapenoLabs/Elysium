-- Who sees a meeting (docs/meeting-review.md, Who sees a meeting): every member reads a public
-- meeting, a private one is read by its owner and the people it is shared with, and admins read
-- every meeting. A new meeting is private until its correction reads it and decides.

-- `unchecked`: new, private to its owner until its correction decides. `private`: read by its
-- owner and the people it is shared with. `public`: read by every member. The meetings already
-- here were never checked for anything sensitive, so they stay private, for their owners to open.
ALTER TABLE meetings
    ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'
        CHECK (visibility IN ('unchecked', 'private', 'public')),
    -- The inbox item that told its owner the correction kept it private, and why, which the
    -- item's page opens it from.
    ADD COLUMN visibility_item_id UUID REFERENCES action_items (id) ON DELETE SET NULL;
ALTER TABLE meetings ALTER COLUMN visibility SET DEFAULT 'unchecked';

CREATE INDEX meetings_visibility_items ON meetings (visibility_item_id)
    WHERE visibility_item_id IS NOT NULL;

-- Whom a meeting is shared with. A private meeting is read by them besides its owner; a guest
-- reads only a meeting shared with them, public or not.
CREATE TABLE meeting_shares (
    meeting_id UUID NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    person_id UUID NOT NULL REFERENCES users (id),
    -- Who shared it: its owner, an admin, or the system for its correction's decision.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (meeting_id, person_id)
);

-- The meetings shared with a person, which every listing and search asks for.
CREATE INDEX meeting_shares_of_person ON meeting_shares (person_id, meeting_id);

-- The meeting whose readers alone find a document: the meeting itself for its whole transcript,
-- the excerpt's meeting for an excerpt's record, and NULL for every other source, which every
-- member reads (docs/knowledge.md, Who sees what). Not a source of its own: a document's source is
-- still its record, its page, or its meeting.
ALTER TABLE knowledge_documents
    ADD COLUMN access_meeting_id UUID REFERENCES meetings (id) ON DELETE CASCADE;

CREATE INDEX knowledge_documents_of_access_meeting ON knowledge_documents (access_meeting_id)
    WHERE access_meeting_id IS NOT NULL;

-- Every document indexed so far of a meeting names it: a transcript by its source, an excerpt by
-- the meeting its record names. Its `updated_at` stays as it was: nothing it says changed.
ALTER TABLE knowledge_documents DISABLE TRIGGER set_updated_at;
UPDATE knowledge_documents
SET access_meeting_id = meeting_id
WHERE meeting_id IS NOT NULL;
UPDATE knowledge_documents documents
SET access_meeting_id = meetings.id
FROM timeline_records records
JOIN meetings ON meetings.id::text = records.metadata ->> 'meetingId'
WHERE records.id = documents.timeline_record_id
    AND records.source_kind = 'meeting';
ALTER TABLE knowledge_documents ENABLE TRIGGER set_updated_at;

-- A meeting's whole transcript is found by that meeting's readers alone, whatever wrote it.
ALTER TABLE knowledge_documents ADD CONSTRAINT knowledge_documents_transcript_access
    CHECK (meeting_id IS NULL OR access_meeting_id IS NOT DISTINCT FROM meeting_id);
