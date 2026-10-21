// Copyright © 2026 Jalapeno Labs

//! The one or two sentence summary of each finished turn (`crate::sessions::summary`).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::on_constraint;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::turn_summaries;

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = turn_summaries)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TurnSummary {
    pub session_id: i64,
    pub turn_id: Uuid,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = turn_summaries)]
struct NewTurnSummary<'summary> {
    session_id: i64,
    turn_id: Uuid,
    summary: &'summary str,
}

/// Records `summary` for the turn, replacing one it already had.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for a session that was
/// deleted meanwhile.
pub async fn record(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    turn_id: Uuid,
    summary: &str,
) -> QueryResult<TurnSummary> {
    diesel::insert_into(turn_summaries::table)
        .values(NewTurnSummary {
            session_id,
            turn_id,
            summary,
        })
        .on_conflict(on_constraint("turn_summaries_pkey"))
        .do_update()
        .set(turn_summaries::summary.eq(summary))
        .returning(TurnSummary::as_returning())
        .get_result(connection)
        .await
}

/// Every summary of a session's turns.
///
/// # Errors
/// Propagates any database error.
pub async fn for_session(
    connection: &mut AsyncPgConnection,
    session_id: i64,
) -> QueryResult<Vec<TurnSummary>> {
    turn_summaries::table
        .filter(turn_summaries::session_id.eq(session_id))
        .order(turn_summaries::created_at.asc())
        .select(TurnSummary::as_select())
        .load(connection)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::coding_session::{self, NewCodingSession};
    use crate::models::project::{self, NewProject};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_turns_summary_is_kept_replaced_and_deleted_with_its_session() {
        let (_url, mut connection) = migrated_database().await;
        let project = project::create(
            &mut connection,
            &NewProject {
                created_by: TEST_PERSON_ID,
                name: "Elysium".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("a project");
        let id = coding_session::reserve_id(&mut connection)
            .await
            .expect("an id");
        let session = coding_session::create(
            &mut connection,
            &NewCodingSession {
                created_by: TEST_PERSON_ID,
                id,
                project_id: Some(project.id),
                title: "Summaries".to_owned(),
                github_credential_id: None,
                action_item_id: None,
                studio_item_id: None,
            },
        )
        .await
        .expect("a session");
        let turn_id = Uuid::now_v7();

        record(
            &mut connection,
            session.id,
            turn_id,
            "Fixed the login form.",
        )
        .await
        .expect("recorded");
        let replaced = record(&mut connection, session.id, turn_id, "Fixed the form.")
            .await
            .expect("replaced");
        assert_eq!(replaced.summary, "Fixed the form.");
        let kept = for_session(&mut connection, session.id)
            .await
            .expect("listed");
        assert_eq!(kept.len(), 1);

        coding_session::delete(&mut connection, session.id)
            .await
            .expect("deleted");
        let left = for_session(&mut connection, session.id)
            .await
            .expect("listed");
        assert!(left.is_empty());
    }
}
