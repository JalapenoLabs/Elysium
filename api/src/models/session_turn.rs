// Copyright © 2026 Jalapeno Labs

//! The turns coding and Studio sessions are given, and the queue the engine runs them from.
//!
//! The API queues a turn ([`queue`]); the engine takes each session's oldest queued turn once the
//! session has none running ([`take_next`]), so a session's turns run one at a time and each
//! resumes the Claude session the one before it left. What a session is doing is read from these
//! rows ([`activities`]), so it is the same in every process and survives a restart of either.
//! See `crate::sessions`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::session_turns;
use crate::sessions::views::{ActivityState, SessionActivity};

/// Where a turn stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum)]
#[ExistingTypePath = "crate::database::schema::sql_types::SessionTurnStatus"]
pub enum SessionTurnStatus {
    /// Waiting for the engine.
    Queued,
    /// The engine took it: waiting for a slot on the runner, or running once `started_at` is set.
    Running,
    /// Over, however it ended.
    Finished,
}

/// A stored turn.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = session_turns, check_for_backend(diesel::pg::Pg))]
pub struct SessionTurn {
    pub id: Uuid,
    pub session_id: i64,
    pub prompt: String,
    pub system_prompt_append: Option<String>,
    pub status: SessionTurnStatus,
    pub stop_requested: bool,
    pub queued_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
}

/// A turn to queue.
#[derive(Debug, Insertable)]
#[diesel(table_name = session_turns)]
pub struct NewSessionTurn<'a> {
    pub id: Uuid,
    pub session_id: i64,
    pub prompt: &'a str,
    pub system_prompt_append: Option<&'a str>,
    pub created_by: Uuid,
}

/// Queues a turn behind any the session has unfinished.
///
/// # Errors
/// Propagates any database error, such as a session that no longer exists.
pub async fn queue(
    connection: &mut AsyncPgConnection,
    turn: &NewSessionTurn<'_>,
) -> QueryResult<SessionTurn> {
    diesel::insert_into(session_turns::table)
        .values(turn)
        .returning(SessionTurn::as_returning())
        .get_result(connection)
        .await
}

/// What each of `session_ids` is doing. A session with no turns is idle.
///
/// # Errors
/// Propagates any database error.
pub async fn activities(
    connection: &mut AsyncPgConnection,
    session_ids: &[i64],
) -> QueryResult<HashMap<i64, SessionActivity>> {
    let unfinished: Vec<SessionTurn> = session_turns::table
        .filter(session_turns::session_id.eq_any(session_ids))
        .filter(session_turns::status.ne(SessionTurnStatus::Finished))
        .select(SessionTurn::as_select())
        .load(connection)
        .await?;
    // Each turn's latest moment is when it finished, else started, else was queued.
    let latest: Vec<(i64, Option<DateTime<Utc>>)> = session_turns::table
        .filter(session_turns::session_id.eq_any(session_ids))
        .group_by(session_turns::session_id)
        .select((
            session_turns::session_id,
            diesel::dsl::max(diesel::dsl::sql::<diesel::sql_types::Timestamptz>(
                "coalesce(finished_at, started_at, queued_at)",
            )),
        ))
        .load(connection)
        .await?;

    let mut by_session: HashMap<i64, Vec<SessionTurn>> = HashMap::new();
    for turn in unfinished {
        by_session.entry(turn.session_id).or_default().push(turn);
    }
    Ok(latest
        .into_iter()
        .map(|(session_id, last_activity_at)| {
            let turns = by_session.remove(&session_id).unwrap_or_default();
            (session_id, activity_of(&turns, last_activity_at))
        })
        .collect())
}

/// What a session whose unfinished turns are `unfinished` is doing.
fn activity_of(
    unfinished: &[SessionTurn],
    last_activity_at: Option<DateTime<Utc>>,
) -> SessionActivity {
    let mut current_turn_id = None;
    let mut queue_depth: u32 = 0;
    for turn in unfinished {
        if turn.status == SessionTurnStatus::Running && turn.started_at.is_some() {
            current_turn_id = Some(turn.id);
        } else {
            queue_depth += 1;
        }
    }
    let state = match (current_turn_id, queue_depth) {
        (Some(_), _) => ActivityState::Running,
        (None, 0) => ActivityState::Idle,
        (None, _) => ActivityState::Queued,
    };
    SessionActivity {
        state,
        queue_depth,
        current_turn_id,
        last_activity_at,
    }
}

/// Takes the oldest queued turn of every session that has none running, for the engine to run.
/// A turn someone asked to stop is never taken.
///
/// Only the engine takes turns, and one engine runs, so nothing else races this but a stop,
/// which the update's own conditions exclude.
///
/// # Errors
/// Propagates any database error.
pub async fn take_next(connection: &mut AsyncPgConnection) -> QueryResult<Vec<SessionTurn>> {
    let busy: Vec<i64> = session_turns::table
        .filter(session_turns::status.eq(SessionTurnStatus::Running))
        .select(session_turns::session_id)
        .load(connection)
        .await?;
    let queued: Vec<(Uuid, i64)> = session_turns::table
        .filter(session_turns::status.eq(SessionTurnStatus::Queued))
        .filter(session_turns::stop_requested.eq(false))
        .filter(session_turns::session_id.ne_all(&busy))
        .order((
            session_turns::session_id,
            session_turns::queued_at,
            session_turns::id,
        ))
        .select((session_turns::id, session_turns::session_id))
        .load(connection)
        .await?;

    let mut oldest = Vec::new();
    let mut previous_session = None;
    for (id, session_id) in queued {
        if previous_session != Some(session_id) {
            oldest.push(id);
            previous_session = Some(session_id);
        }
    }
    if oldest.is_empty() {
        return Ok(Vec::new());
    }
    diesel::update(
        session_turns::table
            .filter(session_turns::id.eq_any(&oldest))
            .filter(session_turns::status.eq(SessionTurnStatus::Queued))
            .filter(session_turns::stop_requested.eq(false)),
    )
    .set(session_turns::status.eq(SessionTurnStatus::Running))
    .returning(SessionTurn::as_returning())
    .get_results(connection)
    .await
}

/// Records that turn `id`'s CLI started.
///
/// # Errors
/// Propagates any database error.
pub async fn record_started(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    diesel::update(
        session_turns::table
            .find(id)
            .filter(session_turns::status.eq(SessionTurnStatus::Running)),
    )
    .set(session_turns::started_at.eq(diesel::dsl::now))
    .execute(connection)
    .await
    .map(drop)
}

/// Records that turn `id` is over. A turn whose session was deleted meanwhile has no row.
///
/// # Errors
/// Propagates any database error.
pub async fn finish(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    diesel::update(
        session_turns::table
            .find(id)
            .filter(session_turns::status.ne(SessionTurnStatus::Finished)),
    )
    .set((
        session_turns::status.eq(SessionTurnStatus::Finished),
        session_turns::finished_at.eq(diesel::dsl::now),
    ))
    .execute(connection)
    .await
    .map(drop)
}

/// Asks every unfinished turn of session `session_id` to stop. A queued turn is finished at
/// once, never having run; one the engine holds is marked, and the engine cancels it. Answers
/// whether the engine holds any, and so must be told.
///
/// # Errors
/// Propagates any database error.
pub async fn request_stop(
    connection: &mut AsyncPgConnection,
    session_id: i64,
) -> QueryResult<bool> {
    diesel::update(
        session_turns::table
            .filter(session_turns::session_id.eq(session_id))
            .filter(session_turns::status.eq(SessionTurnStatus::Queued)),
    )
    .set((
        session_turns::stop_requested.eq(true),
        session_turns::status.eq(SessionTurnStatus::Finished),
        session_turns::finished_at.eq(diesel::dsl::now),
    ))
    .execute(connection)
    .await?;
    let held = diesel::update(
        session_turns::table
            .filter(session_turns::session_id.eq(session_id))
            .filter(session_turns::status.eq(SessionTurnStatus::Running)),
    )
    .set(session_turns::stop_requested.eq(true))
    .execute(connection)
    .await?;
    Ok(held > 0)
}

/// Of `held`, the turns the engine must cancel: asked to stop, finished elsewhere, or gone with
/// their session.
///
/// # Errors
/// Propagates any database error.
pub async fn to_cancel(
    connection: &mut AsyncPgConnection,
    held: &[Uuid],
) -> QueryResult<Vec<Uuid>> {
    let still_wanted: Vec<Uuid> = session_turns::table
        .filter(session_turns::id.eq_any(held))
        .filter(session_turns::status.eq(SessionTurnStatus::Running))
        .filter(session_turns::stop_requested.eq(false))
        .select(session_turns::id)
        .load(connection)
        .await?;
    Ok(held
        .iter()
        .copied()
        .filter(|id| !still_wanted.contains(id))
        .collect())
}

/// Settles what an engine that stopped left: a turn that had started was cut short and is
/// finished, and one still waiting for a slot is queued again, to run as if never taken.
/// Answers the sessions whose activity changed.
///
/// # Errors
/// Propagates any database error.
pub async fn settle_after_restart(connection: &mut AsyncPgConnection) -> QueryResult<Vec<i64>> {
    let mut sessions: Vec<i64> = diesel::update(
        session_turns::table
            .filter(session_turns::status.eq(SessionTurnStatus::Running))
            .filter(session_turns::started_at.is_not_null()),
    )
    .set((
        session_turns::status.eq(SessionTurnStatus::Finished),
        session_turns::finished_at.eq(diesel::dsl::now),
    ))
    .returning(session_turns::session_id)
    .get_results(connection)
    .await?;
    let requeued: Vec<i64> = diesel::update(
        session_turns::table.filter(session_turns::status.eq(SessionTurnStatus::Running)),
    )
    .set(session_turns::status.eq(SessionTurnStatus::Queued))
    .returning(session_turns::session_id)
    .get_results(connection)
    .await?;
    sessions.extend(requeued);
    sessions.sort_unstable();
    sessions.dedup();
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn turn(status: SessionTurnStatus, started: bool) -> SessionTurn {
        let at = Utc
            .with_ymd_and_hms(2026, 11, 5, 9, 0, 0)
            .single()
            .expect("a time");
        SessionTurn {
            id: Uuid::now_v7(),
            session_id: 1,
            prompt: "Do it".to_owned(),
            system_prompt_append: None,
            status,
            stop_requested: false,
            queued_at: at,
            started_at: started.then_some(at),
            finished_at: None,
            created_by: Uuid::nil(),
        }
    }

    #[test]
    fn a_session_runs_the_turn_whose_cli_started_and_counts_the_rest_as_queued() {
        let running = turn(SessionTurnStatus::Running, true);
        let waiting = turn(SessionTurnStatus::Queued, false);
        let activity = activity_of(&[running.clone(), waiting], None);
        assert_eq!(activity.state, ActivityState::Running);
        assert_eq!(activity.current_turn_id, Some(running.id));
        assert_eq!(activity.queue_depth, 1);
    }

    #[test]
    fn a_turn_the_engine_took_but_that_waits_for_a_slot_is_still_queued() {
        let activity = activity_of(&[turn(SessionTurnStatus::Running, false)], None);
        assert_eq!(activity.state, ActivityState::Queued);
        assert_eq!(activity.current_turn_id, None);
        assert_eq!(activity.queue_depth, 1);
    }

    #[test]
    fn a_session_without_unfinished_turns_is_idle() {
        assert_eq!(activity_of(&[], None).state, ActivityState::Idle);
    }

    /// A Coding session for a new project, as the create route records one.
    async fn session(connection: &mut AsyncPgConnection) -> i64 {
        use crate::models::coding_session::{self, NewCodingSession};
        use crate::models::project::{self, NewProject};
        use crate::test_support::TEST_PERSON_ID;

        let project_id = project::create(
            connection,
            &NewProject {
                created_by: TEST_PERSON_ID,
                name: format!("Queue {}", Uuid::now_v7()),
                description: String::new(),
            },
        )
        .await
        .expect("a project")
        .id;
        let id = coding_session::reserve_id(connection)
            .await
            .expect("a number");
        coding_session::create(
            connection,
            &NewCodingSession {
                created_by: TEST_PERSON_ID,
                id,
                project_id: Some(project_id),
                title: "Queue".to_owned(),
                github_credential_id: None,
                action_item_id: None,
                studio_item_id: None,
            },
        )
        .await
        .expect("a session")
        .id
    }

    async fn queued(connection: &mut AsyncPgConnection, session_id: i64, prompt: &str) -> Uuid {
        queue(
            connection,
            &NewSessionTurn {
                id: Uuid::now_v7(),
                session_id,
                prompt,
                system_prompt_append: None,
                created_by: crate::test_support::TEST_PERSON_ID,
            },
        )
        .await
        .expect("the turn queues")
        .id
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_engine_takes_one_turn_per_session_in_order_and_the_next_once_it_finished() {
        let (_url, mut connection) = crate::test_support::migrated_database().await;
        let first_session = session(&mut connection).await;
        let second_session = session(&mut connection).await;
        let first = queued(&mut connection, first_session, "one").await;
        let second = queued(&mut connection, first_session, "two").await;
        let other = queued(&mut connection, second_session, "elsewhere").await;

        let mut taken: Vec<Uuid> = take_next(&mut connection)
            .await
            .expect("taken")
            .iter()
            .map(|turn| turn.id)
            .collect();
        taken.sort_unstable();
        let mut expected = vec![first, other];
        expected.sort_unstable();
        assert_eq!(taken, expected, "the oldest of each session, side by side");
        assert!(
            take_next(&mut connection).await.expect("taken").is_empty(),
            "nothing more while each session runs one"
        );

        finish(&mut connection, first).await.expect("finished");
        let next = take_next(&mut connection).await.expect("taken");
        assert_eq!(next.len(), 1);
        assert_eq!(
            next[0].id, second,
            "the session's next turn, once its first finished"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_stop_finishes_what_waits_and_marks_what_the_engine_runs() {
        let (_url, mut connection) = crate::test_support::migrated_database().await;
        let session_id = session(&mut connection).await;
        let running = queued(&mut connection, session_id, "running").await;
        let waiting = queued(&mut connection, session_id, "waiting").await;
        take_next(&mut connection).await.expect("taken");
        record_started(&mut connection, running)
            .await
            .expect("started");

        let activity = activities(&mut connection, &[session_id])
            .await
            .expect("read")
            .remove(&session_id)
            .expect("the session has turns");
        assert_eq!(activity.state, ActivityState::Running);
        assert_eq!(activity.current_turn_id, Some(running));
        assert_eq!(activity.queue_depth, 1);

        assert!(
            request_stop(&mut connection, session_id)
                .await
                .expect("stopped"),
            "the engine holds one, so it must be told"
        );
        assert_eq!(
            to_cancel(&mut connection, &[running]).await.expect("read"),
            vec![running]
        );
        assert!(
            take_next(&mut connection).await.expect("taken").is_empty(),
            "the waiting turn never runs"
        );
        let stopped = session_turns::table
            .find(waiting)
            .select(SessionTurn::as_select())
            .first(&mut connection)
            .await
            .expect("read");
        assert_eq!(stopped.status, SessionTurnStatus::Finished);
        assert!(stopped.started_at.is_none());
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_restarted_engine_finishes_what_ran_and_runs_again_what_only_waited_for_a_slot() {
        let (_url, mut connection) = crate::test_support::migrated_database().await;
        let first_session = session(&mut connection).await;
        let second_session = session(&mut connection).await;
        let started = queued(&mut connection, first_session, "started").await;
        let waiting = queued(&mut connection, second_session, "waiting").await;
        take_next(&mut connection).await.expect("taken");
        record_started(&mut connection, started)
            .await
            .expect("started");

        let mut settled = settle_after_restart(&mut connection)
            .await
            .expect("settled");
        settled.sort_unstable();
        assert_eq!(settled, vec![first_session, second_session]);

        let again = take_next(&mut connection).await.expect("taken");
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].id, waiting, "taken again, as if it never was");
        let idle = activities(&mut connection, &[first_session])
            .await
            .expect("read")
            .remove(&first_session)
            .expect("the session has turns");
        assert_eq!(idle.state, ActivityState::Idle);
        assert!(idle.last_activity_at.is_some());
    }
}
