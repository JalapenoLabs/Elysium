// Copyright © 2026 Jalapeno Labs

//! Coding sessions: Elysium's record of an Arsox thread on a satellite.
//!
//! Only the pointer is stored. The thread's state, turns, and event history live on
//! the satellite and are read from it; see `crate::fleet`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::coding_sessions;

/// A stored session.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Insertable)]
#[diesel(table_name = coding_sessions, check_for_backend(diesel::pg::Pg))]
pub struct CodingSession {
    pub id: Uuid,
    pub satellite_id: Uuid,
    pub thread_id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub project_id: Uuid,
}

/// Fields for a new session. The id is chosen by the caller because it doubles as the
/// satellite's idempotency key, which must exist before the thread does.
#[derive(Debug, Insertable)]
#[diesel(table_name = coding_sessions)]
pub struct NewCodingSession {
    pub id: Uuid,
    pub project_id: Uuid,
    pub satellite_id: Uuid,
    pub thread_id: String,
    pub title: String,
}

/// Every session, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<CodingSession>> {
    coding_sessions::table
        .order(coding_sessions::created_at.desc())
        .select(CodingSession::as_select())
        .load(connection)
        .await
}

/// Every session recorded against one satellite.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_satellite(
    connection: &mut AsyncPgConnection,
    satellite_id: Uuid,
) -> QueryResult<Vec<CodingSession>> {
    coding_sessions::table
        .filter(coding_sessions::satellite_id.eq(satellite_id))
        .select(CodingSession::as_select())
        .load(connection)
        .await
}

/// One session by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<CodingSession> {
    coding_sessions::table
        .find(id)
        .select(CodingSession::as_select())
        .first(connection)
        .await
}

/// Records a session.
///
/// # Errors
/// Propagates database errors, including a foreign key violation for an unknown
/// project or satellite and a unique violation for a thread that is already recorded.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_session: &NewCodingSession,
) -> QueryResult<CodingSession> {
    diesel::insert_into(coding_sessions::table)
        .values(new_session)
        .returning(CodingSession::as_returning())
        .get_result(connection)
        .await
}

/// Renames a session.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn rename(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    title: &str,
) -> QueryResult<CodingSession> {
    diesel::update(coding_sessions::table.find(id))
        .set(coding_sessions::title.eq(title))
        .returning(CodingSession::as_returning())
        .get_result(connection)
        .await
}

/// Forgets one session.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(coding_sessions::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;
    use crate::errors::ApiError;
    use crate::models::project::{self, NewProject};
    use crate::models::satellite::{self, NewSatellite};
    use crate::test_support::{cipher, migrated_database};

    async fn satellite_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
        let new_satellite = NewSatellite {
            name: name.to_owned(),
            description: String::new(),
            url: "http://arsox:8080".to_owned(),
            secret: SecretString::from("bearer"),
            is_active: true,
        };
        satellite::create(connection, &cipher(), &new_satellite)
            .await
            .expect("insert satellite")
            .id
    }

    async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
        let new_project = NewProject {
            name: name.to_owned(),
            description: String::new(),
        };
        project::create(connection, &new_project)
            .await
            .expect("insert project")
            .id
    }

    fn new_session(project_id: Uuid, satellite_id: Uuid, thread_id: &str) -> NewCodingSession {
        NewCodingSession {
            id: Uuid::now_v7(),
            project_id,
            satellite_id,
            thread_id: thread_id.to_owned(),
            title: format!("Session on {thread_id}"),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn sessions_list_newest_first_and_filter_by_satellite() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let orbit = satellite_named(&mut connection, "orbit").await;
        let lagrange = satellite_named(&mut connection, "lagrange").await;

        let older = create(&mut connection, &new_session(project, orbit, "thread-a"))
            .await
            .expect("insert");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let newer = create(&mut connection, &new_session(project, lagrange, "thread-b"))
            .await
            .expect("insert");

        let ids: Vec<Uuid> = list(&mut connection)
            .await
            .expect("list")
            .into_iter()
            .map(|session| session.id)
            .collect();
        assert_eq!(ids, vec![newer.id, older.id]);

        let on_orbit = list_for_satellite(&mut connection, orbit)
            .await
            .expect("list");
        assert_eq!(on_orbit.len(), 1);
        assert_eq!(on_orbit[0].thread_id, "thread-a");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_thread_is_recorded_once_and_unknown_parents_are_refused() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let orbit = satellite_named(&mut connection, "orbit").await;

        create(&mut connection, &new_session(project, orbit, "thread-a"))
            .await
            .expect("insert");
        let duplicate = create(&mut connection, &new_session(project, orbit, "thread-a"))
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(duplicate), ApiError::Conflict(_)));

        create(
            &mut connection,
            &new_session(project, Uuid::now_v7(), "thread-b"),
        )
        .await
        .expect_err("the foreign key refuses a satellite that does not exist");
        create(
            &mut connection,
            &new_session(Uuid::now_v7(), orbit, "thread-c"),
        )
        .await
        .expect_err("the foreign key refuses a project that does not exist");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn deleting_a_satellite_forgets_its_sessions() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let orbit = satellite_named(&mut connection, "orbit").await;
        let session = create(&mut connection, &new_session(project, orbit, "thread-a"))
            .await
            .expect("insert");

        let renamed = rename(&mut connection, session.id, "Renamed")
            .await
            .expect("rename");
        assert_eq!(renamed.title, "Renamed");

        satellite::delete(&mut connection, orbit)
            .await
            .expect("delete satellite");
        assert!(matches!(
            ApiError::from(find(&mut connection, session.id).await.unwrap_err()),
            ApiError::NotFound
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_project_with_sessions_cannot_be_deleted() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let orbit = satellite_named(&mut connection, "orbit").await;
        let session = create(&mut connection, &new_session(project, orbit, "thread-a"))
            .await
            .expect("insert");

        let refused = project::delete(&mut connection, project).await.unwrap_err();
        assert!(matches!(
            refused,
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::ForeignKeyViolation,
                _
            )
        ));

        delete(&mut connection, session.id)
            .await
            .expect("delete session");
        project::delete(&mut connection, project)
            .await
            .expect("an empty project deletes");
    }
}
