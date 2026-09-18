// Copyright © 2026 Jalapeno Labs

//! Jira Cloud credentials, and the projects and boards each one is allowed to touch.
//!
//! The token never exists in Postgres as plaintext. It is sealed here, bound to the row's
//! id, before the insert or update leaves the process. Callers hand in a [`SecretString`]
//! and only ever get one back from [`JiraCredential::token`].
//!
//! A token is only ever stored alongside what Jira said about it, which is why writes take
//! a [`VerifiedToken`]: the handler checks the token with Jira first, so every row names
//! the account it acts as and the site it acts on.
//!
//! What a credential may reach is stored the way a storage location's projects are: `"*"`
//! is the `all_projects` or `all_boards` column and no link rows, and an explicit list is
//! link rows and the column false. The rows carry each entry's display name so the settings
//! page renders a selection without calling Jira, and each project's key so a bounded
//! search can name it in JQL.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use serde::{Serialize, Serializer};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::{jira_credential_boards, jira_credential_projects, jira_credentials};
use crate::jira::Account;

/// The wildcard that stands for everything of a kind, including what is added later.
const EVERYTHING: &str = "*";

/// Everything of a kind, or the explicit list a credential was given.
///
/// Serializes as `"*"` or as the list itself, the shape clients send and receive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Allowlist<Item> {
    All,
    Only(Vec<Item>),
}

impl<Item: Serialize> Serialize for Allowlist<Item> {
    fn serialize<Target: Serializer>(
        &self,
        serializer: Target,
    ) -> Result<Target::Ok, Target::Error> {
        match self {
            Self::All => serializer.serialize_str(EVERYTHING),
            Self::Only(items) => items.serialize(serializer),
        }
    }
}

/// One project a credential may touch, as Jira spells it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Queryable, Selectable)]
#[diesel(table_name = jira_credential_projects, check_for_backend(diesel::pg::Pg))]
#[serde(rename_all = "camelCase")]
pub struct AllowedProject {
    /// Jira's own project id, a number as a string.
    #[diesel(column_name = project_id)]
    pub id: String,
    /// The project key issues are named after, such as `ELY` in `ELY-12`.
    #[diesel(column_name = project_key)]
    pub key: String,
    pub name: String,
}

/// One board a credential may touch, as Jira spells it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Queryable, Selectable)]
#[diesel(table_name = jira_credential_boards, check_for_backend(diesel::pg::Pg))]
#[serde(rename_all = "camelCase")]
pub struct AllowedBoard {
    #[diesel(column_name = board_id)]
    pub id: i64,
    pub name: String,
    /// The board's project, when it has exactly one.
    pub project_key: Option<String>,
}

/// What one credential is allowed to touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowed {
    pub projects: Allowlist<AllowedProject>,
    pub boards: Allowlist<AllowedBoard>,
}

impl Allowlist<AllowedProject> {
    /// Whether issues in `project_key` are inside this allowlist.
    ///
    /// Jira writes project keys in uppercase but accepts any case in a URL, so keys are
    /// compared case insensitively.
    pub fn allows(&self, project_key: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(projects) => projects
                .iter()
                .any(|project| project.key.eq_ignore_ascii_case(project_key)),
        }
    }
}

/// A stored credential. `token_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = jira_credentials, check_for_backend(diesel::pg::Pg))]
pub struct JiraCredential {
    pub id: Uuid,
    pub name: String,
    /// The site's origin, such as `https://acme.atlassian.net`.
    pub site_url: String,
    /// The Atlassian account the token belongs to, the username half of basic auth.
    pub account_email: String,
    pub token_encrypted: Vec<u8>,
    /// The account Jira reported when the token was last checked.
    pub account_id: String,
    /// That account's name, as Jira reported it.
    pub display_name: String,
    /// Every project, including ones added later. Otherwise only the projects linked in
    /// `jira_credential_projects`.
    pub all_projects: bool,
    /// Every board, the same way.
    pub all_boards: bool,
    /// When Jira last confirmed the token, which is every save and every test.
    pub checked_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A token Jira has just accepted, with the site and account it was checked against.
///
/// The three travel together because they are only ever true together: a token belongs to
/// one account on one site.
#[derive(Debug)]
pub struct VerifiedToken {
    pub site_url: String,
    pub account_email: String,
    pub token: SecretString,
    pub account: Account,
}

/// Fields for a new credential, with the token still in plaintext.
#[derive(Debug)]
pub struct NewJiraCredential {
    pub name: String,
    pub verified: VerifiedToken,
    pub allowed: Allowed,
}

/// A partial update. `None` leaves a column or a selection untouched. A token always
/// arrives with what Jira said about it, so the row never describes a token it no longer
/// holds.
#[derive(Debug, Default)]
pub struct JiraCredentialChanges {
    pub name: Option<String>,
    pub verified: Option<VerifiedToken>,
    pub projects: Option<Allowlist<AllowedProject>>,
    pub boards: Option<Allowlist<AllowedBoard>>,
}

impl JiraCredentialChanges {
    /// True when applying these changes would not touch any column or any link.
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.verified.is_none()
            && self.projects.is_none()
            && self.boards.is_none()
    }
}

/// Row-shaped insert, holding the already sealed token.
#[derive(Insertable)]
#[diesel(table_name = jira_credentials)]
struct JiraCredentialRow<'a> {
    id: Uuid,
    name: &'a str,
    site_url: &'a str,
    account_email: &'a str,
    token_encrypted: Vec<u8>,
    account_id: &'a str,
    display_name: &'a str,
    all_projects: bool,
    all_boards: bool,
    checked_at: DateTime<Utc>,
}

/// Row-shaped update. The token's columns move together, or not at all.
#[derive(AsChangeset)]
#[diesel(table_name = jira_credentials)]
struct JiraCredentialChangeset<'a> {
    name: Option<&'a str>,
    site_url: Option<&'a str>,
    account_email: Option<&'a str>,
    token_encrypted: Option<Vec<u8>>,
    account_id: Option<&'a str>,
    display_name: Option<&'a str>,
    all_projects: Option<bool>,
    all_boards: Option<bool>,
    checked_at: Option<DateTime<Utc>>,
}

/// One link between a credential and a project it may touch.
#[derive(Insertable)]
#[diesel(table_name = jira_credential_projects)]
struct ProjectLinkRow<'a> {
    jira_credential_id: Uuid,
    project_id: &'a str,
    project_key: &'a str,
    name: &'a str,
}

/// One link between a credential and a board it may touch.
#[derive(Insertable)]
#[diesel(table_name = jira_credential_boards)]
struct BoardLinkRow<'a> {
    jira_credential_id: Uuid,
    board_id: i64,
    name: &'a str,
    project_key: Option<&'a str>,
}

/// Replaces the credential's project links. Every project needs no links: the credential's
/// `all_projects` column says so instead.
async fn replace_project_links(
    connection: &mut AsyncPgConnection,
    credential_id: Uuid,
    projects: &Allowlist<AllowedProject>,
) -> QueryResult<()> {
    diesel::delete(
        jira_credential_projects::table
            .filter(jira_credential_projects::jira_credential_id.eq(credential_id)),
    )
    .execute(connection)
    .await?;

    let Allowlist::Only(projects) = projects else {
        return Ok(());
    };
    let links: Vec<ProjectLinkRow<'_>> = projects
        .iter()
        .map(|project| ProjectLinkRow {
            jira_credential_id: credential_id,
            project_id: &project.id,
            project_key: &project.key,
            name: &project.name,
        })
        .collect();
    diesel::insert_into(jira_credential_projects::table)
        .values(links)
        .execute(connection)
        .await?;
    Ok(())
}

/// Replaces the credential's board links, the same way.
async fn replace_board_links(
    connection: &mut AsyncPgConnection,
    credential_id: Uuid,
    boards: &Allowlist<AllowedBoard>,
) -> QueryResult<()> {
    diesel::delete(
        jira_credential_boards::table
            .filter(jira_credential_boards::jira_credential_id.eq(credential_id)),
    )
    .execute(connection)
    .await?;

    let Allowlist::Only(boards) = boards else {
        return Ok(());
    };
    let links: Vec<BoardLinkRow<'_>> = boards
        .iter()
        .map(|board| BoardLinkRow {
            jira_credential_id: credential_id,
            board_id: board.id,
            name: &board.name,
            project_key: board.project_key.as_deref(),
        })
        .collect();
    diesel::insert_into(jira_credential_boards::table)
        .values(links)
        .execute(connection)
        .await?;
    Ok(())
}

/// Associated data binding a sealed token to its row, so a ciphertext copied into another
/// row refuses to open.
fn token_context(id: Uuid) -> Vec<u8> {
    let mut context = b"jira_credentials.token:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl JiraCredential {
    /// Decrypts this credential's token.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or copied
    /// from another row.
    pub fn token(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(&self.token_encrypted, &token_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Every credential by name, each with what it is allowed to touch.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
) -> QueryResult<Vec<(JiraCredential, Allowed)>> {
    let credentials: Vec<JiraCredential> = jira_credentials::table
        .order(jira_credentials::name.asc())
        .select(JiraCredential::as_select())
        .load(connection)
        .await?;
    let project_links: Vec<(Uuid, AllowedProject)> = jira_credential_projects::table
        .order((
            jira_credential_projects::jira_credential_id,
            jira_credential_projects::project_key,
        ))
        .select((
            jira_credential_projects::jira_credential_id,
            AllowedProject::as_select(),
        ))
        .load(connection)
        .await?;
    let board_links: Vec<(Uuid, AllowedBoard)> = jira_credential_boards::table
        .order((
            jira_credential_boards::jira_credential_id,
            jira_credential_boards::board_id,
        ))
        .select((
            jira_credential_boards::jira_credential_id,
            AllowedBoard::as_select(),
        ))
        .load(connection)
        .await?;

    let mut projects_by_credential: HashMap<Uuid, Vec<AllowedProject>> = HashMap::new();
    for (credential_id, project) in project_links {
        projects_by_credential
            .entry(credential_id)
            .or_default()
            .push(project);
    }
    let mut boards_by_credential: HashMap<Uuid, Vec<AllowedBoard>> = HashMap::new();
    for (credential_id, board) in board_links {
        boards_by_credential
            .entry(credential_id)
            .or_default()
            .push(board);
    }

    Ok(credentials
        .into_iter()
        .map(|credential| {
            let allowed = Allowed {
                projects: if credential.all_projects {
                    Allowlist::All
                } else {
                    Allowlist::Only(
                        projects_by_credential
                            .remove(&credential.id)
                            .unwrap_or_default(),
                    )
                },
                boards: if credential.all_boards {
                    Allowlist::All
                } else {
                    Allowlist::Only(
                        boards_by_credential
                            .remove(&credential.id)
                            .unwrap_or_default(),
                    )
                },
            };
            (credential, allowed)
        })
        .collect())
}

/// What `credential` is allowed to touch.
///
/// # Errors
/// Propagates any database error.
pub async fn allowed_of(
    connection: &mut AsyncPgConnection,
    credential: &JiraCredential,
) -> QueryResult<Allowed> {
    let projects = if credential.all_projects {
        Allowlist::All
    } else {
        Allowlist::Only(
            jira_credential_projects::table
                .filter(jira_credential_projects::jira_credential_id.eq(credential.id))
                .order(jira_credential_projects::project_key)
                .select(AllowedProject::as_select())
                .load(connection)
                .await?,
        )
    };
    let boards = if credential.all_boards {
        Allowlist::All
    } else {
        Allowlist::Only(
            jira_credential_boards::table
                .filter(jira_credential_boards::jira_credential_id.eq(credential.id))
                .order(jira_credential_boards::board_id)
                .select(AllowedBoard::as_select())
                .load(connection)
                .await?,
        )
    };
    Ok(Allowed { projects, boards })
}

/// One credential by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<JiraCredential> {
    jira_credentials::table
        .find(id)
        .select(JiraCredential::as_select())
        .first(connection)
        .await
}

/// Seals the token and inserts the credential with a new `UUIDv7` id, linked to what it may
/// touch.
///
/// # Errors
/// Propagates database errors, including a unique violation on `name`.
pub async fn create(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_credential: &NewJiraCredential,
) -> QueryResult<JiraCredential> {
    let id = Uuid::now_v7();
    let verified = &new_credential.verified;
    let allowed = &new_credential.allowed;
    let row = JiraCredentialRow {
        id,
        name: &new_credential.name,
        site_url: &verified.site_url,
        account_email: &verified.account_email,
        token_encrypted: cipher.seal(
            verified.token.expose_secret().as_bytes(),
            &token_context(id),
        ),
        account_id: &verified.account.account_id,
        display_name: &verified.account.display_name,
        all_projects: allowed.projects == Allowlist::All,
        all_boards: allowed.boards == Allowlist::All,
        checked_at: Utc::now(),
    };

    connection
        .transaction(async move |connection| {
            let credential = diesel::insert_into(jira_credentials::table)
                .values(row)
                .returning(JiraCredential::as_returning())
                .get_result(connection)
                .await?;
            replace_project_links(connection, id, &allowed.projects).await?;
            replace_board_links(connection, id, &allowed.boards).await?;
            Ok(credential)
        })
        .await
}

/// Applies `changes`, re-sealing the token under this row's id if one is given and
/// replacing the links for each selection that is.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any other
/// database error, including a unique violation on `name`.
pub async fn update(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    id: Uuid,
    changes: &JiraCredentialChanges,
) -> QueryResult<JiraCredential> {
    let verified = changes.verified.as_ref();
    let changeset = JiraCredentialChangeset {
        name: changes.name.as_deref(),
        site_url: verified.map(|verified| verified.site_url.as_str()),
        account_email: verified.map(|verified| verified.account_email.as_str()),
        token_encrypted: verified.map(|verified| {
            cipher.seal(
                verified.token.expose_secret().as_bytes(),
                &token_context(id),
            )
        }),
        account_id: verified.map(|verified| verified.account.account_id.as_str()),
        display_name: verified.map(|verified| verified.account.display_name.as_str()),
        all_projects: changes
            .projects
            .as_ref()
            .map(|projects| *projects == Allowlist::All),
        all_boards: changes
            .boards
            .as_ref()
            .map(|boards| *boards == Allowlist::All),
        checked_at: verified.map(|_verified| Utc::now()),
    };

    connection
        .transaction(async move |connection| {
            let credential = diesel::update(jira_credentials::table.find(id))
                .set(changeset)
                .returning(JiraCredential::as_returning())
                .get_result(connection)
                .await?;
            if let Some(projects) = &changes.projects {
                replace_project_links(connection, id, projects).await?;
            }
            if let Some(boards) = &changes.boards {
                replace_board_links(connection, id, boards).await?;
            }
            Ok(credential)
        })
        .await
}

/// Records what Jira answered about a stored token, without touching the token itself.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id, and propagates any
/// other database error.
pub async fn record_check(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    account: &Account,
) -> QueryResult<JiraCredential> {
    let changeset = JiraCredentialChangeset {
        name: None,
        site_url: None,
        account_email: None,
        token_encrypted: None,
        account_id: Some(account.account_id.as_str()),
        display_name: Some(account.display_name.as_str()),
        all_projects: None,
        all_boards: None,
        checked_at: Some(Utc::now()),
    };

    diesel::update(jira_credentials::table.find(id))
        .set(changeset)
        .returning(JiraCredential::as_returning())
        .get_result(connection)
        .await
}

/// Deletes one credential and its links. The token itself stays valid on Jira.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(jira_credentials::table.find(id))
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
    use crate::test_support::{cipher, migrated_database};

    fn project(id: &str, key: &str) -> AllowedProject {
        AllowedProject {
            id: id.to_owned(),
            key: key.to_owned(),
            name: format!("{key} project"),
        }
    }

    fn board(id: i64, project_key: Option<&str>) -> AllowedBoard {
        AllowedBoard {
            id,
            name: format!("board {id}"),
            project_key: project_key.map(str::to_owned),
        }
    }

    fn verified(account_id: &str) -> VerifiedToken {
        VerifiedToken {
            site_url: "https://acme.atlassian.net".to_owned(),
            account_email: "alex@example.com".to_owned(),
            token: SecretString::from(format!("token-{account_id}")),
            account: Account {
                account_id: account_id.to_owned(),
                display_name: "Alex Navarro".to_owned(),
                email: Some("alex@example.com".to_owned()),
            },
        }
    }

    fn new_credential(name: &str, allowed: Allowed) -> NewJiraCredential {
        NewJiraCredential {
            name: name.to_owned(),
            verified: verified("5b10a2844c20165700ede21g"),
            allowed,
        }
    }

    fn only(projects: Vec<AllowedProject>, boards: Vec<AllowedBoard>) -> Allowed {
        Allowed {
            projects: Allowlist::Only(projects),
            boards: Allowlist::Only(boards),
        }
    }

    #[test]
    fn an_allowlist_serializes_as_the_wildcard_or_its_entries() {
        let everything: Allowlist<AllowedProject> = Allowlist::All;
        assert_eq!(
            serde_json::to_value(&everything).expect("serializes"),
            serde_json::json!("*")
        );

        let listed = Allowlist::Only(vec![project("10001", "ELY")]);
        assert_eq!(
            serde_json::to_value(&listed).expect("serializes"),
            serde_json::json!([{ "id": "10001", "key": "ELY", "name": "ELY project" }])
        );

        let boards = Allowlist::Only(vec![board(12, None)]);
        assert_eq!(
            serde_json::to_value(&boards).expect("serializes"),
            serde_json::json!([{ "id": 12, "name": "board 12", "projectKey": null }])
        );
    }

    #[test]
    fn a_projects_allowlist_answers_for_any_case_and_everything() {
        let listed = Allowlist::Only(vec![project("10001", "ELY")]);
        assert!(listed.allows("ELY"));
        assert!(listed.allows("ely"), "Jira accepts a key in any case");
        assert!(!listed.allows("OPS"));
        assert!(!listed.allows(""));

        assert!(Allowlist::All.allows("ANYTHING"));
        assert!(!Allowlist::Only(Vec::new()).allows("ELY"));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn created_credentials_store_only_ciphertext_and_decrypt_back() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let created = create(
            &mut connection,
            &cipher,
            &new_credential(
                "Work",
                only(vec![project("10001", "ELY")], vec![board(12, Some("ELY"))]),
            ),
        )
        .await
        .expect("insert");
        let fetched = find(&mut connection, created.id).await.expect("select");

        assert_eq!(fetched.name, "Work");
        assert_eq!(fetched.site_url, "https://acme.atlassian.net");
        assert_eq!(fetched.account_id, "5b10a2844c20165700ede21g");
        assert_eq!(fetched.display_name, "Alex Navarro");
        assert_eq!(fetched.id.get_version_num(), 7);
        assert!(!fetched.all_projects && !fetched.all_boards);

        let plaintext = b"token-5b10a2844c20165700ede21g";
        assert!(
            !fetched
                .token_encrypted
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );
        assert_eq!(
            fetched.token(&cipher).expect("decrypts").expose_secret(),
            "token-5b10a2844c20165700ede21g"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_token_copied_into_another_row_refuses_to_open() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let victim = create(
            &mut connection,
            &cipher,
            &new_credential("Victim", only(Vec::new(), Vec::new())),
        )
        .await
        .expect("insert");
        let mut attacker = create(
            &mut connection,
            &cipher,
            &new_credential("Attacker", only(Vec::new(), Vec::new())),
        )
        .await
        .expect("insert");

        attacker.token_encrypted.clone_from(&victim.token_encrypted);
        assert_eq!(attacker.token(&cipher).unwrap_err(), OpenError::Inauthentic);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn selections_are_linked_widened_narrowed_and_forgotten_with_the_credential() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let listed = only(
            vec![project("10001", "ELY"), project("10002", "OPS")],
            vec![board(12, Some("ELY")), board(13, None)],
        );
        let credential = create(
            &mut connection,
            &cipher,
            &new_credential("Work", listed.clone()),
        )
        .await
        .expect("insert");
        assert_eq!(
            allowed_of(&mut connection, &credential)
                .await
                .expect("links"),
            listed,
            "entries come back ordered by key and by board id"
        );

        let widened = update(
            &mut connection,
            &cipher,
            credential.id,
            &JiraCredentialChanges {
                projects: Some(Allowlist::All),
                ..JiraCredentialChanges::default()
            },
        )
        .await
        .expect("update");
        assert!(widened.all_projects, "the wildcard is a column, not rows");
        let allowed = allowed_of(&mut connection, &widened).await.expect("links");
        assert_eq!(allowed.projects, Allowlist::All);
        assert_eq!(
            allowed.boards, listed.boards,
            "widening projects leaves the boards alone"
        );

        let narrowed = update(
            &mut connection,
            &cipher,
            credential.id,
            &JiraCredentialChanges {
                projects: Some(Allowlist::Only(vec![project("10002", "OPS")])),
                ..JiraCredentialChanges::default()
            },
        )
        .await
        .expect("update");
        assert!(!narrowed.all_projects);
        assert_eq!(
            allowed_of(&mut connection, &narrowed)
                .await
                .expect("links")
                .projects,
            Allowlist::Only(vec![project("10002", "OPS")]),
            "the wildcard's rows were replaced, not added to"
        );

        delete(&mut connection, credential.id)
            .await
            .expect("delete");
        assert_eq!(
            jira_credential_projects::table
                .count()
                .get_result::<i64>(&mut connection)
                .await
                .expect("count"),
            0,
            "deleting a credential cascades to its projects"
        );
        assert_eq!(
            jira_credential_boards::table
                .count()
                .get_result::<i64>(&mut connection)
                .await
                .expect("count"),
            0,
            "and to its boards"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn listing_pairs_every_credential_with_what_it_may_touch() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        create(
            &mut connection,
            &cipher,
            &new_credential(
                "Everything",
                Allowed {
                    projects: Allowlist::All,
                    boards: Allowlist::All,
                },
            ),
        )
        .await
        .expect("insert");
        create(
            &mut connection,
            &cipher,
            &new_credential(
                "Bounded",
                only(vec![project("10001", "ELY")], vec![board(12, Some("ELY"))]),
            ),
        )
        .await
        .expect("insert");

        let listed = list(&mut connection).await.expect("list");
        let names: Vec<&str> = listed
            .iter()
            .map(|(credential, _allowed)| credential.name.as_str())
            .collect();
        assert_eq!(names, ["Bounded", "Everything"], "listed by name");
        assert_eq!(
            listed[0].1.projects,
            Allowlist::Only(vec![project("10001", "ELY")])
        );
        assert_eq!(listed[1].1.projects, Allowlist::All);
        assert_eq!(listed[1].1.boards, Allowlist::All);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_new_token_replaces_the_site_the_account_and_the_check() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(
            &mut connection,
            &cipher,
            &new_credential("Rotating", only(Vec::new(), Vec::new())),
        )
        .await
        .expect("insert");

        let moved = update(
            &mut connection,
            &cipher,
            original.id,
            &JiraCredentialChanges {
                name: Some("Renamed".to_owned()),
                verified: Some(VerifiedToken {
                    site_url: "https://other.atlassian.net".to_owned(),
                    account_email: "sam@example.com".to_owned(),
                    token: SecretString::from("rotated"),
                    account: Account {
                        account_id: "62a1".to_owned(),
                        display_name: "Sam".to_owned(),
                        email: None,
                    },
                }),
                ..JiraCredentialChanges::default()
            },
        )
        .await
        .expect("update");

        assert_eq!(moved.name, "Renamed");
        assert_eq!(moved.site_url, "https://other.atlassian.net");
        assert_eq!(moved.account_email, "sam@example.com");
        assert_eq!(moved.account_id, "62a1");
        assert!(moved.checked_at > original.checked_at);
        assert_eq!(
            moved.token(&cipher).expect("decrypts").expose_secret(),
            "rotated"
        );

        let renamed = update(
            &mut connection,
            &cipher,
            original.id,
            &JiraCredentialChanges {
                name: Some("Renamed again".to_owned()),
                ..JiraCredentialChanges::default()
            },
        )
        .await
        .expect("update");
        assert_eq!(
            renamed.token_encrypted, moved.token_encrypted,
            "a rename leaves the token alone"
        );
        assert_eq!(renamed.checked_at, moved.checked_at);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_check_refreshes_the_account_without_touching_the_token() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(
            &mut connection,
            &cipher,
            &new_credential("Work", only(Vec::new(), Vec::new())),
        )
        .await
        .expect("insert");

        let checked = record_check(
            &mut connection,
            original.id,
            &Account {
                account_id: "5b10a2844c20165700ede21g".to_owned(),
                display_name: "Alex N".to_owned(),
                email: None,
            },
        )
        .await
        .expect("update");

        assert_eq!(checked.display_name, "Alex N");
        assert!(checked.checked_at > original.checked_at);
        assert_eq!(checked.token_encrypted, original.token_encrypted);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn names_are_unique_and_deletes_report_unknown_ids() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let first = create(
            &mut connection,
            &cipher,
            &new_credential("Work", only(Vec::new(), Vec::new())),
        )
        .await
        .expect("insert");
        let duplicate = create(
            &mut connection,
            &cipher,
            &new_credential("Work", only(Vec::new(), Vec::new())),
        )
        .await
        .unwrap_err();
        assert!(matches!(ApiError::from(duplicate), ApiError::Conflict(_)));

        delete(&mut connection, first.id).await.expect("delete");
        assert!(matches!(
            ApiError::from(delete(&mut connection, first.id).await.unwrap_err()),
            ApiError::NotFound
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn database_constraints_back_up_request_validation() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let mut blank = new_credential("", only(Vec::new(), Vec::new()));
        blank.name = String::new();
        create(&mut connection, &cipher, &blank)
            .await
            .expect_err("jira_credentials_name_length rejects an empty name");

        let mut elsewhere = new_credential("Elsewhere", only(Vec::new(), Vec::new()));
        elsewhere.verified.site_url = "https://jira.example.com".to_owned();
        create(&mut connection, &cipher, &elsewhere)
            .await
            .expect_err("jira_credentials_site_url_shape rejects a site off atlassian.net");

        let mut pathed = new_credential("Pathed", only(Vec::new(), Vec::new()));
        pathed.verified.site_url = "https://acme.atlassian.net/jira".to_owned();
        create(&mut connection, &cipher, &pathed)
            .await
            .expect_err("jira_credentials_site_url_shape rejects a path");

        let mut addressless = new_credential("Addressless", only(Vec::new(), Vec::new()));
        addressless.verified.account_email = "not an address".to_owned();
        create(&mut connection, &cipher, &addressless)
            .await
            .expect_err("jira_credentials_account_email_shape rejects a name with no @");

        let lowercase = new_credential(
            "Lowercase key",
            only(vec![project("10001", "ely")], Vec::new()),
        );
        create(&mut connection, &cipher, &lowercase)
            .await
            .expect_err("jira_credential_projects_project_key_shape rejects a lowercase key");

        let worded = new_credential("Worded id", only(vec![project("first", "ELY")], Vec::new()));
        create(&mut connection, &cipher, &worded).await.expect_err(
            "jira_credential_projects_project_id_shape rejects an id that is not a number",
        );
    }
}
