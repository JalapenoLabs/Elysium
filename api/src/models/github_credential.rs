// Copyright © 2026 Jalapeno Labs

//! GitHub tokens.
//!
//! The token never exists in Postgres as plaintext. It is sealed here, bound to the row's
//! id, before the insert or update leaves the process. Callers hand in a [`SecretString`]
//! and only ever get one back from [`GithubCredential::token`].
//!
//! A token is only ever stored alongside what GitHub said about it, which is why writes
//! take a [`VerifiedToken`]: the handler checks the token with GitHub first, so every row
//! names the account it acts as.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::github_credentials;
use crate::github::Account;

/// How a token was issued, which decides where it is created and what it can be given.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::GithubTokenKind"]
#[serde(rename_all = "kebab-case")]
pub enum GithubTokenKind {
    /// A classic personal access token, `ghp_…`, carrying account-wide scopes.
    Classic,
    /// A fine-grained personal access token, `github_pat_…`, carrying per-repository
    /// permissions.
    FineGrained,
}

/// A stored credential. `token_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = github_credentials, check_for_backend(diesel::pg::Pg))]
pub struct GithubCredential {
    pub id: Uuid,
    pub name: String,
    pub kind: GithubTokenKind,
    pub token_encrypted: Vec<u8>,
    /// The account the token acts as, as GitHub reported it when it was last checked.
    pub login: String,
    /// The token's scopes, comma separated as GitHub lists them; empty for a fine-grained
    /// token.
    pub scopes: String,
    pub token_expires_at: Option<DateTime<Utc>>,
    /// When GitHub last confirmed the token, which is every save and every test.
    pub checked_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A token GitHub has just accepted, with what it answered about it.
#[derive(Debug)]
pub struct VerifiedToken {
    pub kind: GithubTokenKind,
    pub token: SecretString,
    pub account: Account,
}

/// Fields for a new credential, with the token still in plaintext.
#[derive(Debug)]
pub struct NewGithubCredential {
    pub name: String,
    pub verified: VerifiedToken,
}

/// A partial update. `None` leaves a column untouched. A token always arrives with what
/// GitHub said about it, so the row never describes a token it no longer holds.
#[derive(Debug, Default)]
pub struct GithubCredentialChanges {
    pub name: Option<String>,
    pub verified: Option<VerifiedToken>,
}

impl GithubCredentialChanges {
    /// True when applying these changes would not touch any column.
    pub const fn is_empty(&self) -> bool {
        self.name.is_none() && self.verified.is_none()
    }
}

/// Row-shaped insert, holding the already sealed token.
#[derive(Insertable)]
#[diesel(table_name = github_credentials)]
struct GithubCredentialRow<'a> {
    id: Uuid,
    name: &'a str,
    kind: GithubTokenKind,
    token_encrypted: Vec<u8>,
    login: &'a str,
    scopes: String,
    token_expires_at: Option<DateTime<Utc>>,
    checked_at: DateTime<Utc>,
}

/// Row-shaped update. The token's columns move together, or not at all.
#[derive(AsChangeset)]
#[diesel(table_name = github_credentials)]
struct GithubCredentialChangeset<'a> {
    name: Option<&'a str>,
    kind: Option<GithubTokenKind>,
    token_encrypted: Option<Vec<u8>>,
    login: Option<&'a str>,
    scopes: Option<String>,
    #[expect(
        clippy::option_option,
        reason = "Diesel's changeset encoding: outer None skips, Some(None) writes NULL"
    )]
    token_expires_at: Option<Option<DateTime<Utc>>>,
    checked_at: Option<DateTime<Utc>>,
}

/// Associated data binding a sealed token to its row, so a ciphertext copied into another
/// row refuses to open.
fn token_context(id: Uuid) -> Vec<u8> {
    let mut context = b"github_credentials.token:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl GithubCredential {
    /// Decrypts this credential's token.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or
    /// copied from another row.
    pub fn token(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(&self.token_encrypted, &token_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Every credential, by name.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<GithubCredential>> {
    github_credentials::table
        .order(github_credentials::name.asc())
        .select(GithubCredential::as_select())
        .load(connection)
        .await
}

/// One credential by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<GithubCredential> {
    github_credentials::table
        .find(id)
        .select(GithubCredential::as_select())
        .first(connection)
        .await
}

/// Seals the token and inserts the credential with a new `UUIDv7` id.
///
/// # Errors
/// Propagates database errors, including a unique violation on `name`.
pub async fn create(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_credential: &NewGithubCredential,
) -> QueryResult<GithubCredential> {
    let id = Uuid::now_v7();
    let verified = &new_credential.verified;
    let row = GithubCredentialRow {
        id,
        name: &new_credential.name,
        kind: verified.kind,
        token_encrypted: cipher.seal(
            verified.token.expose_secret().as_bytes(),
            &token_context(id),
        ),
        login: &verified.account.login,
        scopes: verified.account.scopes.join(","),
        token_expires_at: verified.account.token_expires_at,
        checked_at: Utc::now(),
    };

    diesel::insert_into(github_credentials::table)
        .values(row)
        .returning(GithubCredential::as_returning())
        .get_result(connection)
        .await
}

/// Applies `changes`, re-sealing the token under this row's id if one is given.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any other
/// database error, including a unique violation on `name`.
pub async fn update(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    id: Uuid,
    changes: &GithubCredentialChanges,
) -> QueryResult<GithubCredential> {
    let verified = changes.verified.as_ref();
    let changeset = GithubCredentialChangeset {
        name: changes.name.as_deref(),
        kind: verified.map(|verified| verified.kind),
        token_encrypted: verified.map(|verified| {
            cipher.seal(
                verified.token.expose_secret().as_bytes(),
                &token_context(id),
            )
        }),
        login: verified.map(|verified| verified.account.login.as_str()),
        scopes: verified.map(|verified| verified.account.scopes.join(",")),
        token_expires_at: verified.map(|verified| verified.account.token_expires_at),
        checked_at: verified.map(|_verified| Utc::now()),
    };

    diesel::update(github_credentials::table.find(id))
        .set(changeset)
        .returning(GithubCredential::as_returning())
        .get_result(connection)
        .await
}

/// Records what GitHub answered about a stored token, without touching the token itself.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id, and propagates
/// any other database error.
pub async fn record_check(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    account: &Account,
) -> QueryResult<GithubCredential> {
    let changeset = GithubCredentialChangeset {
        name: None,
        kind: None,
        token_encrypted: None,
        login: Some(account.login.as_str()),
        scopes: Some(account.scopes.join(",")),
        token_expires_at: Some(account.token_expires_at),
        checked_at: Some(Utc::now()),
    };

    diesel::update(github_credentials::table.find(id))
        .set(changeset)
        .returning(GithubCredential::as_returning())
        .get_result(connection)
        .await
}

/// Deletes one credential.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(github_credentials::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::errors::ApiError;
    use crate::test_support::{cipher, migrated_database};

    fn verified(login: &str, scopes: &[&str]) -> VerifiedToken {
        VerifiedToken {
            kind: GithubTokenKind::Classic,
            token: SecretString::from(format!("ghp_{login}")),
            account: Account {
                login: login.to_owned(),
                scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
                token_expires_at: Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()),
            },
        }
    }

    fn new_credential(name: &str, login: &str) -> NewGithubCredential {
        NewGithubCredential {
            name: name.to_owned(),
            verified: verified(login, &["repo", "read:org"]),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn created_credentials_store_only_ciphertext_and_decrypt_back() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let created = create(&mut connection, &cipher, &new_credential("Work", "octocat"))
            .await
            .expect("insert");
        let fetched = find(&mut connection, created.id).await.expect("select");

        assert_eq!(fetched.name, "Work");
        assert_eq!(fetched.kind, GithubTokenKind::Classic);
        assert_eq!(fetched.login, "octocat");
        assert_eq!(fetched.scopes, "repo,read:org");
        assert_eq!(fetched.id.get_version_num(), 7);

        let plaintext = b"ghp_octocat";
        assert!(
            !fetched
                .token_encrypted
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );
        assert_eq!(
            fetched.token(&cipher).expect("decrypts").expose_secret(),
            "ghp_octocat"
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
            &new_credential("Victim", "octocat"),
        )
        .await
        .expect("insert");
        let mut attacker = create(
            &mut connection,
            &cipher,
            &new_credential("Attacker", "hubot"),
        )
        .await
        .expect("insert");

        attacker.token_encrypted.clone_from(&victim.token_encrypted);
        assert_eq!(attacker.token(&cipher).unwrap_err(), OpenError::Inauthentic);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_new_token_replaces_every_fact_about_the_old_one() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(
            &mut connection,
            &cipher,
            &new_credential("Rotating", "octocat"),
        )
        .await
        .expect("insert");

        let changes = GithubCredentialChanges {
            name: Some("Renamed".to_owned()),
            verified: Some(VerifiedToken {
                kind: GithubTokenKind::FineGrained,
                token: SecretString::from("github_pat_rotated"),
                account: Account {
                    login: "hubot".to_owned(),
                    scopes: Vec::new(),
                    token_expires_at: None,
                },
            }),
        };
        let updated = update(&mut connection, &cipher, original.id, &changes)
            .await
            .expect("update");

        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.kind, GithubTokenKind::FineGrained);
        assert_eq!(updated.login, "hubot");
        assert_eq!(updated.scopes, "");
        assert_eq!(updated.token_expires_at, None);
        assert!(updated.checked_at > original.checked_at);
        assert_eq!(
            updated.token(&cipher).expect("decrypts").expose_secret(),
            "github_pat_rotated"
        );

        let missing = update(&mut connection, &cipher, Uuid::now_v7(), &changes)
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(missing), ApiError::NotFound));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_rename_leaves_the_token_and_its_account_alone() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(&mut connection, &cipher, &new_credential("Work", "octocat"))
            .await
            .expect("insert");

        let updated = update(
            &mut connection,
            &cipher,
            original.id,
            &GithubCredentialChanges {
                name: Some("Personal".to_owned()),
                ..GithubCredentialChanges::default()
            },
        )
        .await
        .expect("update");

        assert_eq!(updated.name, "Personal");
        assert_eq!(updated.login, original.login);
        assert_eq!(updated.checked_at, original.checked_at);
        assert_eq!(updated.token_encrypted, original.token_encrypted);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_check_refreshes_what_github_said_without_touching_the_token() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(&mut connection, &cipher, &new_credential("Work", "octocat"))
            .await
            .expect("insert");

        let checked = record_check(
            &mut connection,
            original.id,
            &Account {
                login: "octocat".to_owned(),
                scopes: vec!["repo".to_owned()],
                token_expires_at: None,
            },
        )
        .await
        .expect("update");

        assert_eq!(checked.scopes, "repo");
        assert_eq!(checked.token_expires_at, None);
        assert!(checked.checked_at > original.checked_at);
        assert_eq!(checked.token_encrypted, original.token_encrypted);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn names_are_unique_and_deletes_report_unknown_ids() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let first = create(&mut connection, &cipher, &new_credential("Work", "octocat"))
            .await
            .expect("insert");
        let duplicate = create(&mut connection, &cipher, &new_credential("Work", "hubot"))
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

        let mut blank = new_credential("", "octocat");
        blank.name = String::new();
        create(&mut connection, &cipher, &blank)
            .await
            .expect_err("github_credentials_name_length rejects an empty name");

        let mut spaced = new_credential("Spaced scopes", "octocat");
        spaced.verified.account.scopes = vec!["repo, read:org".to_owned()];
        create(&mut connection, &cipher, &spaced)
            .await
            .expect_err("github_credentials_scopes_shape rejects a scope with a space");

        let mut invalid_login = new_credential("Invalid login", "not a login");
        invalid_login.verified.account.login = "not a login".to_owned();
        create(&mut connection, &cipher, &invalid_login)
            .await
            .expect_err("github_credentials_login_shape rejects a login with spaces");
    }
}
