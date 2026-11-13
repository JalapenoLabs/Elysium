-- What agents write from a meeting is read only by whoever may read every meeting it came from
-- (docs/knowledge.md, Who sees what, What agents write from a meeting). Every row names a meeting
-- whatever its visibility, and who may read it is asked on every request, so making a meeting
-- public or private again changes who reads what was written from it with nothing written again.
--
-- A meeting is never deleted for good, and the meeting key below refuses it (NO ACTION) rather
-- than cascade: a row that went with its meeting would open what was held back to everyone.

-- The meetings a timeline record's words come from: an excerpt its own meeting, and an agent's
-- record every meeting its writer had read when it wrote it.
CREATE TABLE timeline_record_meetings (
    timeline_record_id UUID NOT NULL REFERENCES timeline_records (id) ON DELETE CASCADE,
    meeting_id UUID NOT NULL REFERENCES meetings (id),
    PRIMARY KEY (timeline_record_id, meeting_id)
);

CREATE INDEX timeline_record_meetings_of_meeting ON timeline_record_meetings (meeting_id);

-- The meetings that reached a Project Manager's turn: what woke it, and what its tools handed it.
CREATE TABLE project_manager_turn_meetings (
    turn_id UUID NOT NULL REFERENCES project_manager_turns (id) ON DELETE CASCADE,
    meeting_id UUID NOT NULL REFERENCES meetings (id),
    PRIMARY KEY (turn_id, meeting_id)
);

CREATE INDEX project_manager_turn_meetings_of_meeting ON project_manager_turn_meetings (meeting_id);

-- The meetings that reached one of Elysia's turns: the meeting whose thread asked her, and what
-- her tools handed her.
CREATE TABLE elysia_turn_meetings (
    turn_id UUID NOT NULL REFERENCES elysia_turns (id) ON DELETE CASCADE,
    meeting_id UUID NOT NULL REFERENCES meetings (id),
    PRIMARY KEY (turn_id, meeting_id)
);

CREATE INDEX elysia_turn_meetings_of_meeting ON elysia_turn_meetings (meeting_id);

-- The meetings that reached a coding or Studio session's turn: every meeting its session's earlier
-- turns read, which it resumes, and what its tools handed it. Keyed as `turn_summaries` is, by its
-- session and its turn, with no key to `session_turns`: what holds a summary back goes with the
-- session that holds the summary, and never with its turn's row alone, which would open it.
CREATE TABLE session_turn_meetings (
    session_id BIGINT NOT NULL REFERENCES coding_sessions (id) ON DELETE CASCADE,
    turn_id UUID NOT NULL,
    meeting_id UUID NOT NULL REFERENCES meetings (id),
    PRIMARY KEY (turn_id, meeting_id)
);

CREATE INDEX session_turn_meetings_of_meeting ON session_turn_meetings (meeting_id);

-- What was written before this is held to what can be told of it, and to more where nothing can:
-- unsure is held back. First what each thing read on its own account.

-- Every excerpt, and every topic segmented before excerpts, names its meeting.
INSERT INTO timeline_record_meetings (timeline_record_id, meeting_id)
SELECT records.id, meetings.id
FROM timeline_records records
JOIN meetings ON meetings.id::text = records.metadata ->> 'meetingId'
WHERE records.source_kind = 'meeting';

-- A Project Manager's turn read the meeting of each task that woke it.
INSERT INTO project_manager_turn_meetings (turn_id, meeting_id)
SELECT DISTINCT wakes.turn_id, tasks.meeting_id
FROM project_manager_wakes wakes
JOIN meeting_tasks tasks ON tasks.mark_id = wakes.meeting_task_id
WHERE wakes.turn_id IS NOT NULL;

-- A change a Project Manager made from a task is held to the task's meeting.
INSERT INTO timeline_record_meetings (timeline_record_id, meeting_id)
SELECT records.id, tasks.meeting_id
FROM timeline_records records
JOIN meeting_tasks tasks ON tasks.mark_id::text = records.metadata ->> 'meetingTaskId'
WHERE records.source_kind = 'project_manager' AND records.metadata ->> 'kind' = 'write'
ON CONFLICT DO NOTHING;

-- Elysia read the meeting whose comment thread asked her.
INSERT INTO elysia_turn_meetings (turn_id, meeting_id)
SELECT turns.id, meetings.id
FROM elysia_turns turns
JOIN meetings ON meetings.id::text = turns.trigger_detail ->> 'meetingId'
WHERE turns.trigger_kind = 'meeting-comment';

-- A session's turn read the meeting of each document and file it opened. Which documents a search
-- or a listing of meetings answered is not kept, so a turn that searched or listed is held to
-- every meeting whoever started its session may read (models::meeting::readable_by, as it stood
-- when this ran).
INSERT INTO session_turn_meetings (session_id, turn_id, meeting_id)
SELECT events.session_id, events.turn_id, documents.access_meeting_id
FROM session_events events
JOIN knowledge_documents documents
    ON documents.id::text = events.payload -> 'input' ->> 'documentId'
WHERE events.turn_id IS NOT NULL
    AND events.payload ->> 'kind' = 'toolStarted'
    AND events.payload ->> 'toolName' = 'mcp__elysium_knowledge__knowledge_get'
    AND documents.access_meeting_id IS NOT NULL
UNION
SELECT events.session_id, events.turn_id, attachments.meeting_id
FROM session_events events
JOIN meeting_attachments attachments
    ON attachments.id::text = events.payload -> 'input' ->> 'attachmentId'
WHERE events.turn_id IS NOT NULL
    AND events.payload ->> 'kind' = 'toolStarted'
    AND events.payload ->> 'toolName' = 'mcp__elysium_knowledge__meeting_attachment_get'
UNION
SELECT searched.session_id, searched.turn_id, meetings.id
FROM (
    -- Each turn once, however many times it searched.
    SELECT DISTINCT events.session_id, events.turn_id, sessions.created_by
    FROM session_events events
    JOIN coding_sessions sessions ON sessions.id = events.session_id
    WHERE events.turn_id IS NOT NULL
        AND events.payload ->> 'kind' = 'toolStarted'
        AND events.payload ->> 'toolName' IN (
            'mcp__elysium_knowledge__knowledge_search',
            'mcp__elysium_knowledge__meetings_list'
        )
) searched
JOIN users starter ON starter.id = searched.created_by
CROSS JOIN meetings
WHERE (starter.status = 'active' AND starter.role = 'admin')
    OR meetings.created_by = starter.id
    OR EXISTS (
        SELECT 1 FROM meeting_shares shares
        WHERE shares.meeting_id = meetings.id AND shares.person_id = starter.id
    )
    OR (
        meetings.visibility = 'public'
        AND (
            (starter.status = 'active' AND starter.role IN ('admin', 'member'))
            OR starter.role IN ('agent', 'system')
        )
    );

-- What the passes below read every time and nothing they write changes, read once: the Project
-- Manager turns whose logs show they read their project's timeline or its promises, the order of
-- each session's turns, a turn's place being when it was queued, as a session carries its reading
-- today (models::meeting_reads::carry_session), and the records whose documents a session's turn
-- opened.
CREATE TEMPORARY TABLE timeline_readers AS
SELECT id, project_id, COALESCE(finished_at, now()) AS read_until
FROM project_manager_turns
WHERE log LIKE '%manager_timeline%' OR log LIKE '%manager_record%';

CREATE TEMPORARY TABLE promise_readers AS
SELECT id, project_id, COALESCE(finished_at, now()) AS read_until
FROM project_manager_turns
WHERE log LIKE '%manager_promises%';

CREATE TEMPORARY TABLE session_turn_order AS
SELECT session_id, id AS turn_id, queued_at AS began
FROM session_turns;

CREATE TEMPORARY TABLE session_records_opened AS
SELECT DISTINCT events.session_id, events.turn_id, documents.timeline_record_id
FROM session_events events
JOIN knowledge_documents documents
    ON documents.id::text = events.payload -> 'input' ->> 'documentId'
WHERE events.turn_id IS NOT NULL
    AND events.payload ->> 'kind' = 'toolStarted'
    AND events.payload ->> 'toolName' = 'mcp__elysium_knowledge__knowledge_get'
    AND documents.timeline_record_id IS NOT NULL;

-- Then what each thing read of what others wrote, until nothing more is learned: a record an
-- agent read carries the meetings it names into what that agent wrote next, which another agent
-- may read in turn. Each pass says how much it learned, so a long upgrade shows its progress.
DO $$
DECLARE
    learned BIGINT;
    total BIGINT;
    pass INTEGER := 0;
BEGIN
    LOOP
        total := 0;
        pass := pass + 1;

        -- A Project Manager's turn read each record that woke it, and the record each promise
        -- that woke it was made in.
        WITH added AS (
            INSERT INTO project_manager_turn_meetings (turn_id, meeting_id)
            SELECT wakes.turn_id, named.meeting_id
            FROM project_manager_wakes wakes
            JOIN timeline_record_meetings named
                ON named.timeline_record_id = wakes.timeline_record_id
            WHERE wakes.turn_id IS NOT NULL
            UNION
            SELECT wakes.turn_id, named.meeting_id
            FROM project_manager_wakes wakes
            JOIN promises ON promises.id = wakes.promise_id
            JOIN timeline_record_meetings named
                ON named.timeline_record_id = promises.timeline_record_id
            WHERE wakes.turn_id IS NOT NULL
            ON CONFLICT DO NOTHING
            RETURNING 1
        )
        SELECT count(*) INTO learned FROM added;
        total := total + learned;

        -- A turn whose log shows it read its project's timeline or promises may have read any
        -- record on its project, or any record a promise there was made in, from before it
        -- finished. Which one is not kept, so it is held to all of them.
        WITH added AS (
            INSERT INTO project_manager_turn_meetings (turn_id, meeting_id)
            SELECT turns.id, named.meeting_id
            FROM timeline_readers turns
            JOIN timeline_record_projects attributed ON attributed.project_id = turns.project_id
            JOIN timeline_record_meetings named
                ON named.timeline_record_id = attributed.timeline_record_id
            WHERE attributed.created_at <= turns.read_until
            UNION
            SELECT turns.id, named.meeting_id
            FROM promise_readers turns
            JOIN promises ON promises.project_id = turns.project_id
            JOIN timeline_record_meetings named
                ON named.timeline_record_id = promises.timeline_record_id
            WHERE promises.created_at <= turns.read_until
            ON CONFLICT DO NOTHING
            RETURNING 1
        )
        SELECT count(*) INTO learned FROM added;
        total := total + learned;

        -- A Project Manager's turn and each batch it submitted are held to what the turn read, and
        -- each change it made to the meetings the record that caused it names.
        WITH added AS (
            INSERT INTO timeline_record_meetings (timeline_record_id, meeting_id)
            SELECT turns.timeline_record_id, read.meeting_id
            FROM project_manager_turns turns
            JOIN project_manager_turn_meetings read ON read.turn_id = turns.id
            WHERE turns.timeline_record_id IS NOT NULL
            UNION
            SELECT records.id, read.meeting_id
            FROM timeline_records records
            JOIN project_manager_turn_meetings read
                ON read.turn_id::text = records.metadata ->> 'turnId'
            WHERE records.source_kind = 'project_manager'
                AND records.metadata ->> 'kind' = 'batch'
            UNION
            SELECT records.id, named.meeting_id
            FROM timeline_records records
            JOIN timeline_record_meetings named
                ON named.timeline_record_id::text = records.metadata ->> 'causeRecordId'
            WHERE records.source_kind = 'project_manager'
                AND records.metadata ->> 'kind' = 'write'
            ON CONFLICT DO NOTHING
            RETURNING 1
        )
        SELECT count(*) INTO learned FROM added;
        total := total + learned;

        -- A worker's merged pull request is held to what its batch came from: what the turn that
        -- submitted it read, and the meetings the records it names name.
        WITH added AS (
            INSERT INTO timeline_record_meetings (timeline_record_id, meeting_id)
            SELECT merges.id, named.meeting_id
            FROM timeline_records merges
            JOIN work_batches batches ON batches.id::text = merges.metadata ->> 'batchId'
            CROSS JOIN LATERAL jsonb_array_elements_text(batches.source_record_ids)
                AS source (record_id)
            JOIN timeline_record_meetings named
                ON named.timeline_record_id::text = source.record_id
            WHERE merges.source_kind = 'merged_pull_request'
            UNION
            SELECT merges.id, read.meeting_id
            FROM timeline_records merges
            JOIN work_batches batches ON batches.id::text = merges.metadata ->> 'batchId'
            JOIN project_manager_turn_meetings read ON read.turn_id = batches.turn_id
            WHERE merges.source_kind = 'merged_pull_request'
            ON CONFLICT DO NOTHING
            RETURNING 1
        )
        SELECT count(*) INTO learned FROM added;
        total := total + learned;

        -- A session's turn read what the documents it opened name, and carries what its
        -- session's earlier turns read; its summary's record is held to all of it.
        WITH added AS (
            INSERT INTO session_turn_meetings (session_id, turn_id, meeting_id)
            SELECT opened.session_id, opened.turn_id, named.meeting_id
            FROM session_records_opened opened
            JOIN timeline_record_meetings named
                ON named.timeline_record_id = opened.timeline_record_id
            UNION
            SELECT later.session_id, later.turn_id, earlier_read.meeting_id
            FROM session_turn_order later
            JOIN session_turn_order earlier
                ON earlier.session_id = later.session_id AND earlier.began < later.began
            JOIN session_turn_meetings earlier_read ON earlier_read.turn_id = earlier.turn_id
            ON CONFLICT DO NOTHING
            RETURNING 1
        )
        SELECT count(*) INTO learned FROM added;
        total := total + learned;

        WITH added AS (
            INSERT INTO timeline_record_meetings (timeline_record_id, meeting_id)
            SELECT records.id, read.meeting_id
            FROM timeline_records records
            JOIN session_turn_meetings read ON read.turn_id = records.turn_summary_turn_id
            WHERE records.turn_summary_turn_id IS NOT NULL
            ON CONFLICT DO NOTHING
            RETURNING 1
        )
        SELECT count(*) INTO learned FROM added;
        total := total + learned;

        RAISE NOTICE 'what agents wrote from meetings, pass %: % held back', pass, total;
        EXIT WHEN total = 0;
    END LOOP;
END $$;

DROP TABLE timeline_readers, promise_readers, session_turn_order, session_records_opened;
