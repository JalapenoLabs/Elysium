// Copyright © 2026 Jalapeno Labs

//! How a project readies its workers' containers: the repositories it declares, each with an
//! optional setup script, and one script for the tools its work needs. See `docs/engine.md`,
//! Workers.
//!
//! A worker runs the tools script as root before anything is cloned, then clones each
//! repository its batch names and runs that repository's setup script as the agent in its
//! directory. A batch may name a repository its project does not declare; it is cloned with no
//! setup script.
//!
//! The declarations are replaced whole ([`replace`]), in one transaction under the project's
//! row lock, so two saves never interleave their lists.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::{project_repositories, projects};

/// The most repositories a project declares, as a batch clones at most this many.
pub const REPOSITORIES_MAX: usize = 16;

/// The longest setup script, matching `project_repositories.setup_script` and
/// `projects.tools_setup_script`.
pub const SCRIPT_MAX_CHARACTERS: usize = 20_000;

/// One repository a project declares.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = project_repositories, check_for_backend(diesel::pg::Pg))]
pub struct ProjectRepository {
    pub id: Uuid,
    pub project_id: Uuid,
    pub position: i32,
    pub url: String,
    /// The branch work starts from; the remote's default when `None`.
    pub base_branch: Option<String>,
    /// Bash, run as the agent in the repository's directory once it is cloned.
    pub setup_script: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// A project's whole worker setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSetup {
    /// Bash, run as root in every worker's container before its repositories are cloned.
    pub tools_setup_script: Option<String>,
    /// In the order the project lists them.
    pub repositories: Vec<ProjectRepository>,
}

/// One repository as a project declares it, already checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub url: String,
    pub base_branch: Option<String>,
    pub setup_script: Option<String>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = project_repositories)]
struct NewProjectRepository<'a> {
    id: Uuid,
    project_id: Uuid,
    position: i32,
    url: &'a str,
    base_branch: Option<&'a str>,
    setup_script: Option<&'a str>,
    created_by: Uuid,
}

/// Project `project_id`'s worker setup.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] for an unknown project; any other database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<WorkerSetup> {
    let tools_setup_script = projects::table
        .find(project_id)
        .select(projects::tools_setup_script)
        .first(connection)
        .await?;
    let repositories = project_repositories::table
        .filter(project_repositories::project_id.eq(project_id))
        .select(ProjectRepository::as_select())
        .order(project_repositories::position)
        .load(connection)
        .await?;
    Ok(WorkerSetup {
        tools_setup_script,
        repositories,
    })
}

/// Replaces project `project_id`'s worker setup with `tools_setup_script` and `repositories`, in
/// that order, as `by`, and answers it as stored.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] for an unknown project; any other database error,
/// including a field outside its column's checks.
pub async fn replace(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    tools_setup_script: Option<&str>,
    repositories: &[Declaration],
    by: Uuid,
) -> QueryResult<WorkerSetup> {
    connection
        .transaction(async |connection| {
            diesel::update(projects::table.find(project_id))
                .set(projects::tools_setup_script.eq(tools_setup_script))
                .returning(projects::id)
                .get_result::<Uuid>(connection)
                .await?;
            diesel::delete(
                project_repositories::table.filter(project_repositories::project_id.eq(project_id)),
            )
            .execute(connection)
            .await?;
            let rows: Vec<NewProjectRepository<'_>> = repositories
                .iter()
                .zip(0..)
                .map(|(repository, position)| NewProjectRepository {
                    id: Uuid::now_v7(),
                    project_id,
                    position,
                    url: &repository.url,
                    base_branch: repository.base_branch.as_deref(),
                    setup_script: repository.setup_script.as_deref(),
                    created_by: by,
                })
                .collect();
            if !rows.is_empty() {
                diesel::insert_into(project_repositories::table)
                    .values(rows)
                    .execute(connection)
                    .await?;
            }
            find(connection, project_id).await
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::project::{self, NewProject};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    fn declared(url: &str, setup_script: Option<&str>) -> Declaration {
        Declaration {
            url: url.to_owned(),
            base_branch: None,
            setup_script: setup_script.map(str::to_owned),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_setup_is_replaced_whole_and_read_in_its_order() {
        let (_url, mut connection) = migrated_database().await;
        let project_id = project::create(
            &mut connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project")
        .id;
        let empty = find(&mut connection, project_id).await.expect("reads");
        assert_eq!(empty.tools_setup_script, None);
        assert!(empty.repositories.is_empty());

        let first = replace(
            &mut connection,
            project_id,
            Some("apt-get install -y ripgrep"),
            &[
                declared("https://github.com/o/web.git", Some("yarn install")),
                declared("https://github.com/o/api.git", None),
            ],
            TEST_PERSON_ID,
        )
        .await
        .expect("replaced");
        let urls: Vec<&str> = first
            .repositories
            .iter()
            .map(|repository| repository.url.as_str())
            .collect();
        assert_eq!(
            urls,
            [
                "https://github.com/o/web.git",
                "https://github.com/o/api.git"
            ]
        );
        assert_eq!(
            first.repositories[0].setup_script.as_deref(),
            Some("yarn install")
        );

        // Swapping two repositories' places is one save, not a clash of positions.
        let swapped = replace(
            &mut connection,
            project_id,
            None,
            &[
                declared("https://github.com/o/api.git", None),
                declared("https://github.com/o/web.git", None),
            ],
            TEST_PERSON_ID,
        )
        .await
        .expect("replaced");
        assert_eq!(swapped.tools_setup_script, None);
        assert_eq!(swapped.repositories[0].url, "https://github.com/o/api.git");
        assert_eq!(swapped.repositories[1].setup_script, None);

        let unknown = replace(&mut connection, Uuid::now_v7(), None, &[], TEST_PERSON_ID)
            .await
            .expect_err("no such project");
        assert!(matches!(unknown, diesel::result::Error::NotFound));
    }
}
