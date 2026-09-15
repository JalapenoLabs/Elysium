// Copyright © 2026 Jalapeno Labs

//! Projects: what coding sessions are grouped under.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::projects;

/// A stored project.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = projects, check_for_backend(diesel::pg::Pg))]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for a new project.
#[derive(Debug)]
pub struct NewProject {
    pub name: String,
    pub description: String,
}

/// A partial update. `None` leaves a column untouched.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = projects)]
pub struct ProjectChanges {
    pub name: Option<String>,
    pub description: Option<String>,
}

impl ProjectChanges {
    /// True when applying these changes would not touch any column.
    pub const fn is_empty(&self) -> bool {
        self.name.is_none() && self.description.is_none()
    }
}

#[derive(Insertable)]
#[diesel(table_name = projects)]
struct ProjectRow<'a> {
    id: Uuid,
    name: &'a str,
    description: &'a str,
}

/// Every project, alphabetically.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Project>> {
    projects::table
        .order(projects::name.asc())
        .select(Project::as_select())
        .load(connection)
        .await
}

/// One project by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Project> {
    projects::table
        .find(id)
        .select(Project::as_select())
        .first(connection)
        .await
}

/// Inserts a project with a new `UUIDv7` id.
///
/// # Errors
/// Propagates database errors, including a unique violation on `name`.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_project: &NewProject,
) -> QueryResult<Project> {
    let row = ProjectRow {
        id: Uuid::now_v7(),
        name: &new_project.name,
        description: &new_project.description,
    };

    diesel::insert_into(projects::table)
        .values(row)
        .returning(Project::as_returning())
        .get_result(connection)
        .await
}

/// Applies `changes`.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any
/// other database error, including a unique violation on `name`.
pub async fn update(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    changes: &ProjectChanges,
) -> QueryResult<Project> {
    diesel::update(projects::table.find(id))
        .set(changes)
        .returning(Project::as_returning())
        .get_result(connection)
        .await
}

/// Deletes one project.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id, and a foreign
/// key violation while any coding session still belongs to it.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(projects::table.find(id))
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
    use crate::errors::ApiError;
    use crate::test_support::migrated_database;

    fn new_project(name: &str) -> NewProject {
        NewProject {
            name: name.to_owned(),
            description: String::new(),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn projects_list_alphabetically_and_names_stay_unique() {
        let (_url, mut connection) = migrated_database().await;

        let zebra = create(&mut connection, &new_project("Zebra"))
            .await
            .expect("insert");
        create(&mut connection, &new_project("Aardvark"))
            .await
            .expect("insert");
        assert_eq!(zebra.id.get_version_num(), 7);

        let names: Vec<String> = list(&mut connection)
            .await
            .expect("list")
            .into_iter()
            .map(|project| project.name)
            .collect();
        assert_eq!(names, vec!["Aardvark", "Zebra"]);

        let rename = ProjectChanges {
            name: Some("Aardvark".to_owned()),
            ..ProjectChanges::default()
        };
        let duplicate = update(&mut connection, zebra.id, &rename)
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(duplicate), ApiError::Conflict(_)));

        let redescribe = ProjectChanges {
            description: Some("Stripes".to_owned()),
            ..ProjectChanges::default()
        };
        let updated = update(&mut connection, zebra.id, &redescribe)
            .await
            .expect("update");
        assert_eq!(updated.description, "Stripes");
        assert_eq!(updated.name, "Zebra", "absent fields are untouched");
    }
}
