// Copyright © 2026 Jalapeno Labs

//! Coding sessions: an agent's directory in the workspace and the conversation its turns
//! continue.
//!
//! The row is the session; its turns run on the runner (`crate::sessions`), its files live in
//! the workspace under `sessions/<id>`, and its events are kept beside the row
//! (`session_event`).

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
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Always set for a Coding session; a Studio session has its item's, which is optional.
    pub project_id: Option<Uuid>,
    /// The GitHub token the session works with, or `None` for no token or one that has since
    /// been deleted.
    pub github_credential_id: Option<Uuid>,
    /// The action item the session was started from, if any.
    pub action_item_id: Option<Uuid>,
    /// The Studio item the session works on; `None` for a Coding session.
    pub studio_item_id: Option<Uuid>,
    /// Who created it.
    pub created_by: Uuid,
    /// Claude Code's id for the conversation, which the next turn resumes; `None` until the
    /// first turn reports it.
    pub harness_session_id: Option<String>,
}

/// Fields for a new session. The id comes from [`reserve_id`], so the session's directory can
/// be named before the row exists.
#[derive(Debug, Insertable)]
#[diesel(table_name = coding_sessions)]
pub struct NewCodingSession {
    pub id: i64,
    pub project_id: Option<Uuid>,
    pub title: String,
    pub github_credential_id: Option<Uuid>,
    pub action_item_id: Option<Uuid>,
    pub studio_item_id: Option<Uuid>,
    /// Who is creating it.
    pub created_by: Uuid,
}

/// The Coding area's sessions, newest first: every session that is not a Studio item's.
///
/// # Errors
/// Propagates any database error.
pub async fn list_coding(connection: &mut AsyncPgConnection) -> QueryResult<Vec<CodingSession>> {
    coding_sessions::table
        .filter(coding_sessions::studio_item_id.is_null())
        .order(coding_sessions::created_at.desc())
        .select(CodingSession::as_select())
        .load(connection)
        .await
}

/// The sessions of the given Studio items, oldest first, which is the order an item's
/// conversation reads in.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_studio_items(
    connection: &mut AsyncPgConnection,
    studio_item_ids: &[Uuid],
) -> QueryResult<Vec<CodingSession>> {
    coding_sessions::table
        .filter(coding_sessions::studio_item_id.eq_any(studio_item_ids))
        .order((coding_sessions::created_at.asc(), coding_sessions::id.asc()))
        .select(CodingSession::as_select())
        .load(connection)
        .await
}

/// Takes the next session number for a session that is about to be created.
///
/// A number is taken for good, even when the session is never recorded, so a create
/// that fails after this leaves a gap in the numbering. Sequences never hand a number
/// out twice.
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
/// Propagates database errors, including a foreign key violation for an unknown project.
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

/// Records the Claude session a session's turns continue.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn set_harness_session_id(
    connection: &mut AsyncPgConnection,
    id: i64,
    harness_session_id: &str,
) -> QueryResult<CodingSession> {
    diesel::update(coding_sessions::table.find(id))
        .set(coding_sessions::harness_session_id.eq(Some(harness_session_id)))
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
    use super::*;
    use crate::models::project::{self, NewProject};
    use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, cipher, migrated_database};

    async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
        let new_project = NewProject {
            created_by: TEST_PERSON_ID,
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
        title: &str,
    ) -> QueryResult<CodingSession> {
        let new_session = NewCodingSession {
            created_by: TEST_PERSON_ID,
            id: reserve_id(connection).await?,
            project_id: Some(project_id),
            title: title.to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
        };
        create(connection, &new_session).await
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn sessions_are_numbered_from_one_and_a_reserved_number_is_never_reused() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;

        let first = record(&mut connection, project, "First")
            .await
            .expect("insert");
        assert_eq!(first.id, 1);

        // A create that fails after reserving, such as a repository that would not clone.
        let abandoned = reserve_id(&mut connection).await.expect("reserve");
        assert_eq!(abandoned, 2);

        let next = record(&mut connection, project, "Second")
            .await
            .expect("insert");
        assert_eq!(next.id, 3);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn sessions_list_newest_first() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;

        let older = record(&mut connection, project, "First")
            .await
            .expect("insert");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let newer = record(&mut connection, project, "Second")
            .await
            .expect("insert");

        let ids: Vec<i64> = list_coding(&mut connection)
            .await
            .expect("list")
            .into_iter()
            .map(|session| session.id)
            .collect();
        assert_eq!(ids, vec![newer.id, older.id]);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_unknown_project_is_refused() {
        let (_url, mut connection) = migrated_database().await;
        record(&mut connection, Uuid::now_v7(), "Orphan")
            .await
            .expect_err("the foreign key refuses a project that does not exist");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_session_is_renamed_and_keeps_the_claude_session_it_continues() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let session = record(&mut connection, project, "First")
            .await
            .expect("insert");
        assert_eq!(session.harness_session_id, None);

        let renamed = rename(&mut connection, session.id, "Renamed")
            .await
            .expect("rename");
        assert_eq!(renamed.title, "Renamed");
        let resumed = set_harness_session_id(&mut connection, session.id, "0f8d-session")
            .await
            .expect("set");
        assert_eq!(resumed.harness_session_id.as_deref(), Some("0f8d-session"));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn only_a_studio_session_may_have_no_project() {
        use crate::models::studio_item::{self, NewStudioItem};

        let (_url, mut connection) = migrated_database().await;
        let location = crate::models::storage_location::create(
            &mut connection,
            &cipher(),
            &crate::test_support::location_for_every_project("assets"),
        )
        .await
        .expect("location");
        let item = studio_item::create(
            &mut connection,
            &NewStudioItem {
                created_by: crate::test_support::TEST_PERSON_ID,
                id: Uuid::now_v7(),
                title: "Banana".to_owned(),
                prompt: "Model me a banana".to_owned(),
                project_id: None,
                storage_location_id: location.id,
            },
        )
        .await
        .expect("item");

        let mut coding = NewCodingSession {
            created_by: crate::test_support::TEST_PERSON_ID,
            id: reserve_id(&mut connection).await.expect("reserve"),
            project_id: None,
            title: "No project".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
        };
        create(&mut connection, &coding)
            .await
            .expect_err("a Coding session needs a project");

        coding.id = reserve_id(&mut connection).await.expect("reserve");
        coding.studio_item_id = Some(item.id);
        let studio = create(&mut connection, &coding)
            .await
            .expect("a Studio session may have none");

        assert!(
            list_coding(&mut connection).await.expect("list").is_empty(),
            "Coding lists only its own sessions"
        );
        let of_item = list_for_studio_items(&mut connection, &[item.id])
            .await
            .expect("list");
        assert_eq!(of_item.len(), 1);
        assert_eq!(of_item[0].id, studio.id);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_project_with_sessions_cannot_be_deleted() {
        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
        let session = record(&mut connection, project, "First")
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
        use crate::database::schema::action_items;
        use crate::models::action_item::{
            self, ActionItemPriority, ActionItemState, NewActionItem, Owner,
        };

        let (_url, mut connection) = migrated_database().await;
        let project = project_named(&mut connection, "Elysium").await;
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
        let item = action_item::create(&mut connection, new_item, TEST_PERSON, chrono::Utc::now())
            .await
            .expect("item")
            .record;

        let new_session = NewCodingSession {
            created_by: TEST_PERSON_ID,
            id: reserve_id(&mut connection).await.expect("reserve"),
            project_id: Some(project),
            title: "Fix the login bug".to_owned(),
            github_credential_id: None,
            action_item_id: Some(item.id),
            studio_item_id: None,
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
