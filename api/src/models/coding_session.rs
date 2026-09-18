// Copyright © 2026 Jalapeno Labs

//! Coding sessions: Elysium's record of an Arsox thread on a satellite.
//!
//! Only the pointer is stored. The thread's state, turns, and event history live on
//! the satellite and are read from it; see `crate::fleet`.

use chrono::{DateTime, Utc};
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::BigInt;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::coding_sessions;

/// A stored session.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Insertable)]
#[diesel(table_name = coding_sessions, check_for_backend(diesel::pg::Pg))]
pub struct CodingSession {
    /// The session's number: 1, 2, 3, ... in the order sessions were started.
    pub id: i64,
    pub satellite_id: Uuid,
    pub thread_id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub project_id: Uuid,
    /// The GitHub token the thread was started with, or `None` for no token or one that
    /// has since been deleted.
    pub github_credential_id: Option<Uuid>,
    /// The action item the session was started from, if any.
    pub action_item_id: Option<Uuid>,
}

/// Fields for a new session. The id comes from [`reserve_id`], because the thread is
/// opened before the row exists and carries the number in its metadata.
#[derive(Debug, Insertable)]
#[diesel(table_name = coding_sessions)]
pub struct NewCodingSession {
    pub id: i64,
    pub project_id: Uuid,
    pub satellite_id: Uuid,
    pub thread_id: String,
    pub title: String,
    pub github_credential_id: Option<Uuid>,
    pub action_item_id: Option<Uuid>,
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

/// Takes the next session number for a session that is about to be created.
///
/// A number is taken for good, even when the session is never recorded, so a create
/// that fails after this leaves a gap in the numbering. Sequences never hand a number
/// out twice, which is what lets the thread carry it before the row exists.
///
/// # Errors
/// Propagates any database error.
pub async fn reserve_id(connection: &mut AsyncPgConnection) -> QueryResult<i64> {
    diesel::select(sql::<BigInt>(
        "nextval(pg_get_serial_sequence('coding_sessions', 'id'))",
    ))
    .get_result(connection)
    .await
}

/// One session by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: i64) -> QueryResult<CodingSession> {
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
    id: i64,
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
pub async fn delete(connection: &mut AsyncPgConnection, id: i64) -> QueryResult<()> {
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

    /// Records a session the way the create route does: reserve a number, then insert.
    async fn record(
        connection: &mut AsyncPgConnection,
        project_id: Uuid,
        satellite_id: Uuid,
        thread_id: &str,
    ) -> QueryResult<CodingSession> {
        let new_session = NewCodingSession {
            id: reserve_id(connection).await?,
            project_id,
            satellite_id,
            thread_id: thread_id.to_owned(),
            title: format!("Session on {thread_id}"),
            github_credential_id: None,
            action_item_id: None,
        };
        create(connection, &new_session).await
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn sessions_are_numbered_from_one_and_a_reserved_number_is_never_reused() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let orbit = satellite_named(&mut connection, "orbit").await;

        let first = record(&mut connection, project, orbit, "thread-a")
            .await
            .expect("insert");
        assert_eq!(first.id, 1);

        // A create that fails after reserving, such as a satellite refusing the thread.
        let abandoned = reserve_id(&mut connection).await.expect("reserve");
        assert_eq!(abandoned, 2);

        let next = record(&mut connection, project, orbit, "thread-b")
            .await
            .expect("insert");
        assert_eq!(next.id, 3);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn sessions_list_newest_first_and_filter_by_satellite() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let orbit = satellite_named(&mut connection, "orbit").await;
        let lagrange = satellite_named(&mut connection, "lagrange").await;

        let older = record(&mut connection, project, orbit, "thread-a")
            .await
            .expect("insert");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let newer = record(&mut connection, project, lagrange, "thread-b")
            .await
            .expect("insert");

        let ids: Vec<i64> = list(&mut connection)
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

        record(&mut connection, project, orbit, "thread-a")
            .await
            .expect("insert");
        let duplicate = record(&mut connection, project, orbit, "thread-a")
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(duplicate), ApiError::Conflict(_)));

        record(&mut connection, project, Uuid::now_v7(), "thread-b")
            .await
            .expect_err("the foreign key refuses a satellite that does not exist");
        record(&mut connection, Uuid::now_v7(), orbit, "thread-c")
            .await
            .expect_err("the foreign key refuses a project that does not exist");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn deleting_a_satellite_forgets_its_sessions() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let orbit = satellite_named(&mut connection, "orbit").await;
        let session = record(&mut connection, project, orbit, "thread-a")
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
        let session = record(&mut connection, project, orbit, "thread-a")
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

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_session_keeps_the_item_it_started_from_and_outlives_the_item_row() {
        use crate::action_items::Actor;
        use crate::database::schema::action_items;
        use crate::models::action_item::{
            self, ActionItemPriority, ActionItemState, NewActionItem, Owner,
        };

        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let orbit = satellite_named(&mut connection, "orbit").await;
        let new_item = NewActionItem {
            title: "Fix the login bug".to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: vec![project],
            initiative_ids: Vec::new(),
        };
        let item = action_item::create(&mut connection, new_item, Actor::User, chrono::Utc::now())
            .await
            .expect("item")
            .record;

        let new_session = NewCodingSession {
            id: reserve_id(&mut connection).await.expect("reserve"),
            project_id: project,
            satellite_id: orbit,
            thread_id: "thread-a".to_owned(),
            title: "Fix the login bug".to_owned(),
            github_credential_id: None,
            action_item_id: Some(item.id),
        };
        let session = create(&mut connection, &new_session).await.expect("insert");
        assert_eq!(session.action_item_id, Some(item.id));

        // Items are deleted softly; a hard delete only happens outside the API, and must
        // not take the session with it.
        diesel::delete(action_items::table.find(item.id))
            .execute(&mut connection)
            .await
            .expect("delete the item row");
        let kept = find(&mut connection, session.id).await.expect("kept");
        assert_eq!(kept.action_item_id, None);
    }
}
