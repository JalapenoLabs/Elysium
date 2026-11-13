// Copyright © 2026 Jalapeno Labs

//! The migration that holds back what agents wrote from meetings (#278), against what production
//! holds before it: a private meeting's excerpt and a public one's on a project, a Project
//! Manager's turns (one woken by the private excerpt, its record, batch, and change; one whose log
//! shows it read its timeline; one woken by a worker's merged pull request), Elysia's answer in a
//! meeting's thread, and two sessions (one that opened the private excerpt's document and then
//! went on, one that searched).
//!
//! Up must name on each what it can be told it read, and more where it cannot be told, carrying
//! what one agent wrote into what another read of it until nothing more is learned. Down must drop
//! it all, and up must apply again.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use std::collections::BTreeSet;

use diesel::QueryableByName;
use diesel::sql_types::Uuid as SqlUuid;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use secrecy::{ExposeSecret, SecretString};
use uuid::{Uuid, uuid};

use super::migrations::{self, MIGRATIONS, MigrationCommand};
use crate::test_support::empty_database;

/// The migration under test.
const WRITTEN_FROM: &str = "2026-11-12-040000-0278_hold_back_what_agents_write_from_meetings";

/// Grace's one-to-one, private to her.
const ONE_TO_ONE: Uuid = uuid!("00000000-0000-7000-8000-000000278001");
/// The weekly sync, every member's.
const WEEKLY: Uuid = uuid!("00000000-0000-7000-8000-000000278002");

/// The one-to-one's excerpt, and the weekly sync's, both on the project.
const PRIVATE_EXCERPT: Uuid = uuid!("00000000-0000-7000-8000-000000278101");
const PUBLIC_EXCERPT: Uuid = uuid!("00000000-0000-7000-8000-000000278102");
/// The Project Manager's record of the turn the private excerpt woke, the batch that turn
/// submitted, and the change it made citing the excerpt.
const TURN_RECORD: Uuid = uuid!("00000000-0000-7000-8000-000000278103");
const BATCH_RECORD: Uuid = uuid!("00000000-0000-7000-8000-000000278104");
const WRITE_RECORD: Uuid = uuid!("00000000-0000-7000-8000-000000278105");
/// The merged pull request of that batch, which woke the Project Manager again.
const MERGE_RECORD: Uuid = uuid!("00000000-0000-7000-8000-000000278106");
/// The summary of the session's second turn.
const SUMMARY_RECORD: Uuid = uuid!("00000000-0000-7000-8000-000000278107");
/// A mail, which no agent wrote.
const MAIL: Uuid = uuid!("00000000-0000-7000-8000-000000278108");

/// The Project Manager's turns: woken by the private excerpt; reading its timeline; woken by the
/// merge.
const WOKEN: Uuid = uuid!("00000000-0000-7000-8000-000000278201");
const BROWSED: Uuid = uuid!("00000000-0000-7000-8000-000000278202");
const MERGED: Uuid = uuid!("00000000-0000-7000-8000-000000278203");

/// Elysia's turn in the one-to-one's thread, and one of hers about nothing in particular.
const THREAD_TURN: Uuid = uuid!("00000000-0000-7000-8000-000000278301");
const IDLE_TURN: Uuid = uuid!("00000000-0000-7000-8000-000000278302");

/// The session that opened the private excerpt's document, its next turn, and a session that
/// searched.
const OPENED: Uuid = uuid!("00000000-0000-7000-8000-000000278401");
const WENT_ON: Uuid = uuid!("00000000-0000-7000-8000-000000278402");
const SEARCHED: Uuid = uuid!("00000000-0000-7000-8000-000000278403");

/// What production holds before the migration.
const BEFORE: &str = r#"
INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) VALUES
 ('00000000-0000-7000-8000-000000278a01', '00000000-0000-7000-8000-000000278a11',
  'ada@example.com', 'Ada', 'admin', 'active', now()),
 ('00000000-0000-7000-8000-000000278a02', '00000000-0000-7000-8000-000000278a12',
  'grace@example.com', 'Grace', 'member', 'active', now()),
 ('00000000-0000-7000-8000-000000278a03', '00000000-0000-7000-8000-000000278a13',
  'lin@example.com', 'Lin', 'member', 'active', now());
INSERT INTO projects (id, name, description, created_by) VALUES
 ('00000000-0000-7000-8000-000000278b01', 'Reorg', '', '00000000-0000-7000-8000-000000278a01');

INSERT INTO meetings (id, title, started_at, organizer, participants, state, created_by,
                      visibility)
VALUES
 ('00000000-0000-7000-8000-000000278001', 'One-to-one', '2026-11-01T15:00:00Z', '', '[]', 'done',
  '00000000-0000-7000-8000-000000278a02', 'private'),
 ('00000000-0000-7000-8000-000000278002', 'Weekly sync', '2026-11-02T15:00:00Z', '', '[]', 'done',
  '00000000-0000-7000-8000-000000278a01', 'public');

INSERT INTO timeline_records (id, source_kind, occurred_at, actor, title, body, metadata,
                              created_by, created_at)
VALUES
 ('00000000-0000-7000-8000-000000278101', 'meeting', '2026-11-01T15:05:00Z', 'Grace',
  'The team moves', '[0:05] Grace: The team moves in May.',
  '{"meetingId": "00000000-0000-7000-8000-000000278001"}', '00000000-0000-7000-8000-000000278a02',
  '2026-11-01T16:00:00Z'),
 ('00000000-0000-7000-8000-000000278102', 'meeting', '2026-11-02T15:05:00Z', 'Ada',
  'The launch', '[0:05] Ada: The launch is Friday.',
  '{"meetingId": "00000000-0000-7000-8000-000000278002"}', '00000000-0000-7000-8000-000000278a01',
  '2026-11-02T16:00:00Z'),
 ('00000000-0000-7000-8000-000000278108', 'outlook', '2026-11-03T09:00:00Z', 'Lin', 'Lunch', '',
  '{}', '00000000-0000-7000-8000-000000278a03', '2026-11-03T09:00:00Z');
INSERT INTO timeline_record_projects (timeline_record_id, project_id, confidence, attributed_by,
                                      created_at)
VALUES
 ('00000000-0000-7000-8000-000000278101', '00000000-0000-7000-8000-000000278b01', 0.9, 'elysia',
  '2026-11-01T16:00:00Z'),
 ('00000000-0000-7000-8000-000000278102', '00000000-0000-7000-8000-000000278b01', 0.9, 'elysia',
  '2026-11-02T16:00:00Z');

INSERT INTO project_managers (project_id, created_by) VALUES
 ('00000000-0000-7000-8000-000000278b01', '00000000-0000-0000-0000-000000000004');
INSERT INTO project_manager_turns (id, project_id, status, summary, log, created_by, started_at,
                                   finished_at)
VALUES
 ('00000000-0000-7000-8000-000000278201', '00000000-0000-7000-8000-000000278b01', 'succeeded',
  'Planned the move', 'Called mcp__elysium_project_manager__manager_submit_batch',
  '00000000-0000-0000-0000-000000000004', '2026-11-01T17:00:00Z', '2026-11-01T17:05:00Z'),
 -- Before the weekly sync's excerpt was on the project, after the one-to-one's.
 ('00000000-0000-7000-8000-000000278202', '00000000-0000-7000-8000-000000278b01', 'succeeded',
  'Read the timeline', 'Called mcp__elysium_project_manager__manager_timeline',
  '00000000-0000-0000-0000-000000000004', '2026-11-01T18:00:00Z', '2026-11-01T18:05:00Z'),
 ('00000000-0000-7000-8000-000000278203', '00000000-0000-7000-8000-000000278b01', 'succeeded',
  'Resolved the move', '', '00000000-0000-0000-0000-000000000004', '2026-11-04T17:00:00Z',
  '2026-11-04T17:05:00Z');

INSERT INTO timeline_records (id, source_kind, occurred_at, actor, title, body, metadata,
                              created_by)
VALUES
 ('00000000-0000-7000-8000-000000278103', 'project_manager', '2026-11-01T17:05:00Z',
  'Project Manager', 'Planned the move', '',
  '{"kind": "turn", "turnId": "00000000-0000-7000-8000-000000278201"}',
  '00000000-0000-0000-0000-000000000004'),
 ('00000000-0000-7000-8000-000000278104', 'project_manager', '2026-11-01T17:03:00Z',
  'Project Manager', 'Move the team', 'Move the team in May.',
  '{"kind": "batch", "turnId": "00000000-0000-7000-8000-000000278201"}',
  '00000000-0000-0000-0000-000000000004'),
 ('00000000-0000-7000-8000-000000278105', 'project_manager', '2026-11-01T17:04:00Z',
  'Project Manager', 'Created an item: Move the team', 'The one-to-one asked for it.',
  '{"kind": "write", "causeRecordId": "00000000-0000-7000-8000-000000278101"}',
  '00000000-0000-0000-0000-000000000004');
UPDATE project_manager_turns SET timeline_record_id = '00000000-0000-7000-8000-000000278103'
WHERE id = '00000000-0000-7000-8000-000000278201';

INSERT INTO work_batches (id, project_id, title, goal, priority, state, turn_id,
                          source_record_ids, created_by, assigned_at, started_at, finished_at, result,
                          origin, generation)
VALUES
 ('00000000-0000-7000-8000-000000278c01', '00000000-0000-7000-8000-000000278b01', 'Move the team',
  'Move the team in May.', 'normal', 'succeeded', '00000000-0000-7000-8000-000000278201',
  '["00000000-0000-7000-8000-000000278104"]', '00000000-0000-0000-0000-000000000004',
  '2026-11-02T08:00:00Z', '2026-11-02T09:00:00Z', '2026-11-03T09:00:00Z', 'Moved.', 'human_rooted', 0);
INSERT INTO timeline_records (id, source_kind, occurred_at, actor, title, body, metadata,
                              created_by)
VALUES
 ('00000000-0000-7000-8000-000000278106', 'merged_pull_request', '2026-11-04T09:00:00Z', 'Elysium',
  'Merged #12: Move the team', '',
  '{"kind": "merged", "batchId": "00000000-0000-7000-8000-000000278c01"}',
  '00000000-0000-0000-0000-000000000003');

INSERT INTO project_manager_wakes (id, project_id, kind, timeline_record_id, turn_id, created_by)
VALUES
 (uuidv7(), '00000000-0000-7000-8000-000000278b01', 'record',
  '00000000-0000-7000-8000-000000278101', '00000000-0000-7000-8000-000000278201',
  '00000000-0000-7000-8000-000000278a02'),
 (uuidv7(), '00000000-0000-7000-8000-000000278b01', 'record',
  '00000000-0000-7000-8000-000000278106', '00000000-0000-7000-8000-000000278203',
  '00000000-0000-0000-0000-000000000003');

INSERT INTO elysia_turns (id, trigger_kind, trigger_detail, status, summary, created_by,
                          started_at, finished_at)
VALUES
 ('00000000-0000-7000-8000-000000278301', 'meeting-comment',
  '{"meetingId": "00000000-0000-7000-8000-000000278001"}', 'succeeded', 'Answered the thread.',
  '00000000-0000-7000-8000-000000278a02', now(), now()),
 ('00000000-0000-7000-8000-000000278302', 'idle', '{}', 'succeeded', 'Revisited her work.',
  '00000000-0000-0000-0000-000000000001', now(), now());

INSERT INTO knowledge_documents (id, source, timeline_record_id, title, occurred_at,
                                 content_sha256, created_by, access_meeting_id)
VALUES
 ('00000000-0000-7000-8000-000000278d01', 'meeting', '00000000-0000-7000-8000-000000278101',
  'The team moves', '2026-11-01T15:05:00Z', repeat('a', 64), '00000000-0000-7000-8000-000000278a02',
  '00000000-0000-7000-8000-000000278001');

INSERT INTO coding_sessions (id, title, project_id, created_by) VALUES
 (27801, 'Reorg notes', '00000000-0000-7000-8000-000000278b01',
  '00000000-0000-7000-8000-000000278a01'),
 (27802, 'Launch notes', '00000000-0000-7000-8000-000000278b01',
  '00000000-0000-7000-8000-000000278a03');
INSERT INTO session_turns (id, session_id, prompt, status, created_by, queued_at, started_at,
                           finished_at)
VALUES
 ('00000000-0000-7000-8000-000000278401', 27801, 'Open the one-to-one.', 'finished',
  '00000000-0000-7000-8000-000000278a01', '2026-11-05T09:00:00Z', '2026-11-05T09:00:00Z',
  '2026-11-05T09:01:00Z'),
 ('00000000-0000-7000-8000-000000278402', 27801, 'Write it up.', 'finished',
  '00000000-0000-7000-8000-000000278a01', '2026-11-05T10:00:00Z', '2026-11-05T10:00:00Z',
  '2026-11-05T10:01:00Z'),
 ('00000000-0000-7000-8000-000000278403', 27802, 'Find the launch.', 'finished',
  '00000000-0000-7000-8000-000000278a03', '2026-11-05T11:00:00Z', '2026-11-05T11:00:00Z',
  '2026-11-05T11:01:00Z');
INSERT INTO session_events (session_id, sequence, occurred_at, turn_id, payload) VALUES
 (27801, 1, '2026-11-05T09:00:10Z', '00000000-0000-7000-8000-000000278401',
  '{"kind": "toolStarted", "toolCallId": "1", "toolName": "mcp__elysium_knowledge__knowledge_get",
    "input": {"documentId": "00000000-0000-7000-8000-000000278d01"}}'),
 (27802, 1, '2026-11-05T11:00:10Z', '00000000-0000-7000-8000-000000278403',
  '{"kind": "toolStarted", "toolCallId": "1",
    "toolName": "mcp__elysium_knowledge__knowledge_search", "input": {"query": "launch"}}');
INSERT INTO turn_summaries (session_id, turn_id, summary) VALUES
 (27801, '00000000-0000-7000-8000-000000278402', 'Wrote up the move.');
INSERT INTO timeline_records (id, source_kind, occurred_at, actor, title, body, metadata,
                              created_by, turn_summary_session_id, turn_summary_turn_id)
VALUES
 ('00000000-0000-7000-8000-000000278107', 'turn_summary', '2026-11-05T10:01:00Z', 'Session 27801',
  'Wrote up the move.', '', '{}', '00000000-0000-0000-0000-000000000003', 27801,
  '00000000-0000-7000-8000-000000278402');
"#;

/// How many migrations come after [`WRITTEN_FROM`], so a test can stand just after it.
fn after_written_from() -> u32 {
    let names: Vec<String> =
        diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .expect("the migrations are embedded")
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
    let position = names
        .iter()
        .position(|name| name == WRITTEN_FROM)
        .expect("the migration is embedded");
    u32::try_from(names.len() - position - 1).expect("a handful of migrations")
}

/// Applies the migrations up to [`WRITTEN_FROM`] and no further.
async fn up(url: &SecretString) {
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("the migration applies");
    let after = after_written_from();
    if after > 0 {
        migrations::execute(url.clone(), MigrationCommand::Revert { count: after })
            .await
            .expect("the migrations after it roll back");
    }
}

/// Rolls back [`WRITTEN_FROM`], the newest migration [`up`] leaves applied.
async fn down(url: &SecretString) {
    migrations::execute(url.clone(), MigrationCommand::Revert { count: 1 })
        .await
        .expect("the migration rolls back");
}

/// A database holding [`BEFORE`], just before the migration, and a connection to it.
async fn laid_out_as_production() -> (SecretString, AsyncPgConnection) {
    let url = empty_database().await;
    up(&url).await;
    down(&url).await;
    let mut connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("connects");
    connection
        .batch_execute(BEFORE)
        .await
        .expect("the layout is stored");
    (url, connection)
}

/// One pair a table holds: what read or names a meeting, and the meeting.
#[derive(QueryableByName)]
struct Pair {
    #[diesel(sql_type = SqlUuid)]
    owner: Uuid,
    #[diesel(sql_type = SqlUuid)]
    meeting: Uuid,
}

/// Every pair `table` holds, its owner in `column`.
async fn pairs(
    connection: &mut AsyncPgConnection,
    table: &str,
    column: &str,
) -> BTreeSet<(Uuid, Uuid)> {
    diesel::sql_query(format!(
        "SELECT {column} AS owner, meeting_id AS meeting FROM {table}"
    ))
    .load::<Pair>(connection)
    .await
    .unwrap_or_else(|error| panic!("{table}: {error}"))
    .into_iter()
    .map(|pair| (pair.owner, pair.meeting))
    .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn what_agents_wrote_before_is_held_to_what_can_be_told_of_it() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;

    // The records: each excerpt its own meeting, and what agents wrote from what they read, the
    // merge carried through the batch it came from, and the second session turn's summary from
    // the first turn it went on from, which opened the one-to-one's excerpt. The mail names none.
    let records = BTreeSet::from([
        (PRIVATE_EXCERPT, ONE_TO_ONE),
        (PUBLIC_EXCERPT, WEEKLY),
        (TURN_RECORD, ONE_TO_ONE),
        (BATCH_RECORD, ONE_TO_ONE),
        (WRITE_RECORD, ONE_TO_ONE),
        (MERGE_RECORD, ONE_TO_ONE),
        (SUMMARY_RECORD, ONE_TO_ONE),
    ]);
    let named = pairs(
        &mut connection,
        "timeline_record_meetings",
        "timeline_record_id",
    )
    .await;
    assert_eq!(named, records);
    assert!(!named.iter().any(|(record, _meeting)| *record == MAIL));

    // The Project Manager's turns: the woken one read the excerpt; the one whose log shows it read
    // its timeline is held to every meeting on it then (the one-to-one alone, the weekly sync
    // arriving after it finished); and the one the merge woke read the merge, which carried the
    // one-to-one, learned on a later pass.
    assert_eq!(
        pairs(&mut connection, "project_manager_turn_meetings", "turn_id").await,
        BTreeSet::from([
            (WOKEN, ONE_TO_ONE),
            (BROWSED, ONE_TO_ONE),
            (MERGED, ONE_TO_ONE),
        ])
    );

    // Elysia read the thread's meeting, and nothing in an idle turn.
    let elysia_read = pairs(&mut connection, "elysia_turn_meetings", "turn_id").await;
    assert_eq!(elysia_read, BTreeSet::from([(THREAD_TURN, ONE_TO_ONE)]));
    assert!(
        !elysia_read
            .iter()
            .any(|(turn, _meeting)| *turn == IDLE_TURN)
    );

    // The session that opened the excerpt's document read its meeting, and its next turn carries
    // it; the one that searched is held to every meeting its starter, a member, may read: the
    // public weekly sync, and not Grace's one-to-one.
    assert_eq!(
        pairs(&mut connection, "session_turn_meetings", "turn_id").await,
        BTreeSet::from([
            (OPENED, ONE_TO_ONE),
            (WENT_ON, ONE_TO_ONE),
            (SEARCHED, WEEKLY),
        ])
    );

    // Down drops it all, and up applies again to the same answer.
    down(&url).await;
    up(&url).await;
    assert_eq!(
        pairs(
            &mut connection,
            "timeline_record_meetings",
            "timeline_record_id"
        )
        .await,
        records
    );
}
