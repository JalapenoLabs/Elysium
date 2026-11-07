// Copyright © 2026 Jalapeno Labs

//! Projects: what coding sessions are grouped under.
//!
//! Every project has a brief, a knowledge base page created with it and renamed with it
//! (`crate::models::project_brief`), so [`create`] and [`update`] write pages too.
//!
//! A project may have a cover image, stored compressed in `cover_image`. [`Project`]
//! leaves the bytes out, so listing projects never loads images; [`find_cover`] reads
//! them for the one request that serves a cover.
//!
//! Removing a cover hides it ([`remove_cover`]) and keeps the image until a new cover
//! replaces it, so [`restore_cover`] can bring it back: removing a cover can be undone.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::database::schema::projects;
use crate::models::page::PageError;
use crate::models::project_brief;
use crate::search::{self, SearchTerms};

/// Which projects something applies to: every project, including ones added later, or
/// only the listed ones.
///
/// Serialized as `"*"` for every project, or an array of project ids. Ids arrive sorted
/// and without duplicates, so each one can be stored once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectScope {
    All,
    Only(Vec<Uuid>),
}

/// The wildcard that stands for every project.
const ALL_PROJECTS: &str = "*";

impl Serialize for ProjectScope {
    fn serialize<Target: Serializer>(
        &self,
        serializer: Target,
    ) -> Result<Target::Ok, Target::Error> {
        match self {
            Self::All => serializer.serialize_str(ALL_PROJECTS),
            Self::Only(ids) => ids.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ProjectScope {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Ids(Vec<Uuid>),
            Text(String),
        }

        match Wire::deserialize(deserializer) {
            Ok(Wire::Ids(mut ids)) => {
                ids.sort_unstable();
                ids.dedup();
                Ok(Self::Only(ids))
            }
            Ok(Wire::Text(text)) if text == ALL_PROJECTS => Ok(Self::All),
            _ => Err(serde::de::Error::custom(
                "projects must be \"*\" or a list of project ids",
            )),
        }
    }
}

/// How a cover sits in its frame.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ProjectCoverFit"]
#[serde(rename_all = "kebab-case")]
pub enum ProjectCoverFit {
    /// The whole image, centered over a blurred copy of itself: logos and odd shapes.
    Fit,
    /// The image covers the frame, cropping what does not fit: photos.
    Fill,
}

/// How a project picks the GitHub token its sessions start with.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::GithubAccess"]
#[serde(rename_all = "kebab-case")]
pub enum GithubAccess {
    /// Follows the workspace's default token, whichever that is when a session starts.
    Default,
    /// Sessions get no GitHub token, even when the workspace has a default.
    None,
    /// Sessions get the project's own token. A project whose token was deleted keeps this
    /// access with no token, and follows the workspace default until it chooses again.
    Specific,
}

/// A stored project.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = projects, check_for_backend(diesel::pg::Pg))]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When the stored cover last changed; `None` when nothing is stored. A removed cover
    /// is still stored, so read [`Project::cover_updated_at`] for the one that shows.
    pub cover_image_updated_at: Option<DateTime<Utc>>,
    /// When the stored cover was removed, or `None` while it shows.
    pub cover_removed_at: Option<DateTime<Utc>>,
    pub cover_fit: ProjectCoverFit,
    pub github_access: GithubAccess,
    /// The project's own token, for `Specific` access only.
    pub github_credential_id: Option<Uuid>,
    /// Who created it.
    pub created_by: Uuid,
    /// Whether its Project Manager's changes apply as it makes them. Off, the default, it
    /// proposes each as a changeset a person approves first.
    pub manager_writes_directly: bool,
}

impl Project {
    /// When the cover that shows last changed, or `None` when none shows.
    pub fn cover_updated_at(&self) -> Option<DateTime<Utc>> {
        self.cover_image_updated_at
            .filter(|_updated_at| self.cover_removed_at.is_none())
    }

    /// The token this project's sessions start with, given the workspace default.
    pub const fn github_credential(&self, workspace_default: Option<Uuid>) -> Option<Uuid> {
        match self.github_access {
            GithubAccess::Default => workspace_default,
            GithubAccess::None => None,
            // A deleted token leaves no id behind, and the project follows the default.
            GithubAccess::Specific => match self.github_credential_id {
                Some(id) => Some(id),
                None => workspace_default,
            },
        }
    }
}

/// Fields for a new project.
#[derive(Debug)]
pub struct NewProject {
    pub name: String,
    pub description: String,
    /// Who is creating it.
    pub created_by: Uuid,
}

/// A partial update. `None` leaves a column untouched.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = projects)]
pub struct ProjectChanges {
    pub name: Option<String>,
    pub description: Option<String>,
    pub cover_fit: Option<ProjectCoverFit>,
    /// Changed together with `github_credential_id`: the database allows a token only for
    /// `Specific` access.
    pub github_access: Option<GithubAccess>,
    #[expect(
        clippy::option_option,
        reason = "Diesel's changeset encoding: outer None skips, Some(None) writes NULL"
    )]
    pub github_credential_id: Option<Option<Uuid>>,
    pub manager_writes_directly: Option<bool>,
}

impl ProjectChanges {
    /// True when applying these changes would not touch any column.
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.cover_fit.is_none()
            && self.github_access.is_none()
            && self.manager_writes_directly.is_none()
    }
}

#[derive(Insertable)]
#[diesel(table_name = projects)]
struct ProjectRow<'a> {
    id: Uuid,
    name: &'a str,
    description: &'a str,
    created_by: Uuid,
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

/// The `limit` projects whose names match `terms` most closely, as id and name. Every
/// project is searched, as the list shows every one. See `crate::search` for how names match
/// and rank.
///
/// # Errors
/// Propagates any database error.
pub async fn search(
    connection: &mut AsyncPgConnection,
    terms: &SearchTerms,
    limit: i64,
) -> QueryResult<Vec<(Uuid, String)>> {
    let mut query = projects::table.into_boxed();
    for pattern in terms.word_patterns() {
        query = query.filter(search::matches(projects::name, pattern));
    }

    query
        .order((
            search::matches(projects::name, terms.title_start_pattern()).desc(),
            search::similarity(projects::name, terms.text().to_owned()).desc(),
            projects::name.asc(),
        ))
        .limit(limit)
        .select((projects::id, projects::name))
        .load(connection)
        .await
}

/// The projects among `ids`, alphabetically. Ids with no project are left out.
///
/// # Errors
/// Propagates any database error.
pub async fn find_many(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
) -> QueryResult<Vec<Project>> {
    projects::table
        .filter(projects::id.eq_any(ids))
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

/// Share-locks a project's row until the transaction ends, for a write that links
/// something to it: deleting the project waits for that write.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn lock_shared(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Uuid> {
    projects::table
        .find(id)
        .for_share()
        .select(projects::id)
        .first(connection)
        .await
}

/// Locks a project's row for its deletion until the transaction ends, so nothing links to
/// it in the meantime.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn lock_for_delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Uuid> {
    projects::table
        .find(id)
        .for_update()
        .select(projects::id)
        .first(connection)
        .await
}

/// Inserts a project with a new `UUIDv7` id, and its brief, in one transaction.
///
/// # Errors
/// Propagates database errors, including a unique violation on `name`.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_project: &NewProject,
) -> Result<Project, PageError> {
    let row = ProjectRow {
        id: Uuid::now_v7(),
        name: &new_project.name,
        description: &new_project.description,
        created_by: new_project.created_by,
    };

    connection
        .transaction(async move |connection| {
            let project = diesel::insert_into(projects::table)
                .values(row)
                .returning(Project::as_returning())
                .get_result(connection)
                .await?;
            project_brief::create(connection, &project).await?;
            Ok(project)
        })
        .await
}

/// Applies `changes`, renaming the project's brief with it in the same transaction.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any
/// other database error, including a unique violation on `name`.
pub async fn update(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    changes: &ProjectChanges,
) -> Result<Project, PageError> {
    connection
        .transaction(async move |connection| {
            let project = diesel::update(projects::table.find(id))
                .set(changes)
                .returning(Project::as_returning())
                .get_result(connection)
                .await?;
            if changes.name.is_some() {
                project_brief::retitle(connection, &project).await?;
            }
            Ok(project)
        })
        .await
}

/// A project's cover as stored, and when it last changed, or `None` when none shows.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no project has that id.
pub async fn find_cover(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Option<(Vec<u8>, DateTime<Utc>)>> {
    let (image, updated_at, removed_at) = projects::table
        .find(id)
        .select((
            projects::cover_image,
            projects::cover_image_updated_at,
            projects::cover_removed_at,
        ))
        .first::<(
            Option<Vec<u8>>,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
        )>(connection)
        .await?;
    if removed_at.is_some() {
        return Ok(None);
    }
    Ok(image.zip(updated_at))
}

/// Replaces the cover with `image`, already compressed. It shows at once, and a removed
/// cover it replaces is gone for good.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id.
pub async fn set_cover(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    image: Vec<u8>,
) -> QueryResult<Project> {
    diesel::update(projects::table.find(id))
        .set((
            projects::cover_image.eq(image),
            projects::cover_image_updated_at.eq(Utc::now()),
            projects::cover_removed_at.eq(None::<DateTime<Utc>>),
        ))
        .returning(Project::as_returning())
        .get_result(connection)
        .await
}

/// Hides the cover that shows, keeping its image so [`restore_cover`] can bring it back.
/// A project with no cover showing is returned as it is.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id.
pub async fn remove_cover(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    now: DateTime<Utc>,
) -> QueryResult<Project> {
    let removed = diesel::update(
        projects::table
            .find(id)
            .filter(projects::cover_image.is_not_null())
            .filter(projects::cover_removed_at.is_null()),
    )
    .set(projects::cover_removed_at.eq(now))
    .returning(Project::as_returning())
    .get_result(connection)
    .await
    .optional()?;
    match removed {
        Some(project) => Ok(project),
        None => find(connection, id).await,
    }
}

/// Shows the removed cover again. A project without one is returned as it is, so a cover
/// uploaded since the removal is never replaced by the one removed.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id.
pub async fn restore_cover(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Project> {
    diesel::update(projects::table.find(id))
        .set(projects::cover_removed_at.eq(None::<DateTime<Utc>>))
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
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    #[test]
    fn project_scopes_are_a_wildcard_or_sorted_unique_ids() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();

        let all: ProjectScope = serde_json::from_value(serde_json::json!("*")).expect("parses");
        assert_eq!(all, ProjectScope::All);
        assert_eq!(serde_json::to_value(&all).expect("serializes"), "*");

        let only: ProjectScope =
            serde_json::from_value(serde_json::json!([second, first, second])).expect("parses");
        assert_eq!(only, ProjectScope::Only(vec![first, second]));

        for refused in [
            serde_json::json!("all"),
            serde_json::json!(["not-a-uuid"]),
            serde_json::json!(4),
        ] {
            serde_json::from_value::<ProjectScope>(refused).expect_err("refused");
        }
    }

    fn new_project(name: &str) -> NewProject {
        NewProject {
            created_by: TEST_PERSON_ID,
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

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn covers_are_set_read_and_removed() {
        let (_url, mut connection) = migrated_database().await;
        let project = create(&mut connection, &new_project("Covered"))
            .await
            .expect("insert");
        assert_eq!(project.cover_image_updated_at, None);
        assert_eq!(
            project.cover_fit,
            ProjectCoverFit::Fit,
            "covers fit by default"
        );

        let filled = update(
            &mut connection,
            project.id,
            &ProjectChanges {
                cover_fit: Some(ProjectCoverFit::Fill),
                ..ProjectChanges::default()
            },
        )
        .await
        .expect("update");
        assert_eq!(filled.cover_fit, ProjectCoverFit::Fill);
        assert_eq!(
            find_cover(&mut connection, project.id).await.expect("read"),
            None
        );

        let covered = set_cover(&mut connection, project.id, b"RIFF-webp".to_vec())
            .await
            .expect("set");
        let (image, updated_at) = find_cover(&mut connection, project.id)
            .await
            .expect("read")
            .expect("a cover");
        assert_eq!(image, b"RIFF-webp");
        assert_eq!(covered.cover_updated_at(), Some(updated_at));

        let uncovered = remove_cover(&mut connection, project.id, Utc::now())
            .await
            .expect("remove");
        assert_eq!(
            uncovered.cover_updated_at(),
            None,
            "a removed cover does not show"
        );
        assert_eq!(
            find_cover(&mut connection, project.id).await.expect("read"),
            None,
            "a removed cover is not served"
        );
        let removed_at = uncovered.cover_removed_at;
        let removed_again = remove_cover(&mut connection, project.id, Utc::now())
            .await
            .expect("remove again");
        assert_eq!(
            removed_again.cover_removed_at, removed_at,
            "removing it again keeps when it was removed"
        );

        let restored = restore_cover(&mut connection, project.id)
            .await
            .expect("restore");
        assert_eq!(
            restored.cover_updated_at(),
            Some(updated_at),
            "the same cover shows again, so its cached URL still holds"
        );
        let (image, _updated_at) = find_cover(&mut connection, project.id)
            .await
            .expect("read")
            .expect("the restored cover");
        assert_eq!(image, b"RIFF-webp");

        find_cover(&mut connection, Uuid::now_v7())
            .await
            .expect_err("an unknown project has no cover to read");
        remove_cover(&mut connection, Uuid::now_v7(), Utc::now())
            .await
            .expect_err("an unknown project has no cover to remove");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_cover_uploaded_after_a_removal_is_never_replaced_by_the_removed_one() {
        let (_url, mut connection) = migrated_database().await;
        let project = create(&mut connection, &new_project("Recovered"))
            .await
            .expect("insert");

        let bare = remove_cover(&mut connection, project.id, Utc::now())
            .await
            .expect("removing nothing answers the project");
        assert_eq!(bare.cover_removed_at, None, "nothing was there to remove");

        set_cover(&mut connection, project.id, b"first".to_vec())
            .await
            .expect("set");
        remove_cover(&mut connection, project.id, Utc::now())
            .await
            .expect("remove");
        let replaced = set_cover(&mut connection, project.id, b"second".to_vec())
            .await
            .expect("replace");
        assert_eq!(replaced.cover_removed_at, None, "a new cover shows at once");

        let restored = restore_cover(&mut connection, project.id)
            .await
            .expect("a late undo");
        assert_eq!(restored.cover_updated_at(), replaced.cover_updated_at());
        let (image, _updated_at) = find_cover(&mut connection, project.id)
            .await
            .expect("read")
            .expect("a cover");
        assert_eq!(image, b"second", "the newer cover stays");
    }
}
