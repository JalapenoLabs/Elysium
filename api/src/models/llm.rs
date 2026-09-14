// Copyright © 2026 Jalapeno Labs

//! LLM provider credentials.
//!
//! The secret token never exists in Postgres as plaintext. It is sealed here, bound
//! to the row's id, before the insert or update leaves the process. Callers hand in
//! a [`SecretString`] and only ever get one back from [`Llm::secret_token`].

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::llms;

/// How a credential authenticates, and against which provider.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::LlmType"]
#[serde(rename_all = "kebab-case")]
pub enum LlmType {
    ChatgptOauth,
    ChatgptApiToken,
    ClaudeApiToken,
    ClaudeCodeOauth,
}

/// A stored credential. `secret_token_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = llms, check_for_backend(diesel::pg::Pg))]
pub struct Llm {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub type_: LlmType,
    pub secret_token_encrypted: Vec<u8>,
    pub priority: i32,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for a new credential, with the token still in plaintext.
#[derive(Debug)]
pub struct NewLlm {
    pub name: String,
    pub description: String,
    pub type_: LlmType,
    pub secret_token: SecretString,
    pub priority: i32,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

/// A partial update. `None` leaves a column untouched; for `expires_at`,
/// `Some(None)` clears it.
#[derive(Debug, Default)]
pub struct LlmChanges {
    pub name: Option<String>,
    pub description: Option<String>,
    pub type_: Option<LlmType>,
    pub secret_token: Option<SecretString>,
    pub priority: Option<i32>,
    pub is_active: Option<bool>,
    #[expect(
        clippy::option_option,
        reason = "absent, clear, and set are three distinct requests"
    )]
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

impl LlmChanges {
    /// True when applying these changes would not touch any column.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.type_.is_none()
            && self.secret_token.is_none()
            && self.priority.is_none()
            && self.is_active.is_none()
            && self.expires_at.is_none()
    }
}

/// Row-shaped insert, holding the already sealed token.
#[derive(Insertable)]
#[diesel(table_name = llms)]
struct LlmRow<'a> {
    id: Uuid,
    name: &'a str,
    description: &'a str,
    type_: LlmType,
    secret_token_encrypted: Vec<u8>,
    priority: i32,
    is_active: bool,
    expires_at: Option<DateTime<Utc>>,
}

/// Row-shaped update, holding the already sealed token if one was supplied.
#[derive(AsChangeset)]
#[diesel(table_name = llms)]
struct LlmChangeset<'a> {
    name: Option<&'a str>,
    description: Option<&'a str>,
    type_: Option<LlmType>,
    secret_token_encrypted: Option<Vec<u8>>,
    priority: Option<i32>,
    is_active: Option<bool>,
    #[expect(
        clippy::option_option,
        reason = "Diesel's changeset encoding: outer None skips, Some(None) writes NULL"
    )]
    expires_at: Option<Option<DateTime<Utc>>>,
}

/// Associated data binding a sealed token to its row, so a ciphertext copied into
/// another row refuses to open.
fn secret_token_context(id: Uuid) -> Vec<u8> {
    let mut context = b"llms.secret_token:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl Llm {
    /// Decrypts this credential's token.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or
    /// copied from another row.
    pub fn secret_token(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext =
            cipher.open(&self.secret_token_encrypted, &secret_token_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Every credential, in the order they should be tried: priority, then age.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Llm>> {
    llms::table
        .order((llms::priority.asc(), llms::created_at.asc()))
        .select(Llm::as_select())
        .load(connection)
        .await
}

/// One credential by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Llm> {
    llms::table
        .find(id)
        .select(Llm::as_select())
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
    new_llm: &NewLlm,
) -> QueryResult<Llm> {
    let id = Uuid::now_v7();
    let row = LlmRow {
        id,
        name: &new_llm.name,
        description: &new_llm.description,
        type_: new_llm.type_,
        secret_token_encrypted: cipher.seal(
            new_llm.secret_token.expose_secret().as_bytes(),
            &secret_token_context(id),
        ),
        priority: new_llm.priority,
        is_active: new_llm.is_active,
        expires_at: new_llm.expires_at,
    };

    diesel::insert_into(llms::table)
        .values(row)
        .returning(Llm::as_returning())
        .get_result(connection)
        .await
}

/// Applies `changes`, re-sealing the token under this row's id if one is given.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any
/// other database error, including a unique violation on `name`.
pub async fn update(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    id: Uuid,
    changes: &LlmChanges,
) -> QueryResult<Llm> {
    let changeset = LlmChangeset {
        name: changes.name.as_deref(),
        description: changes.description.as_deref(),
        type_: changes.type_,
        secret_token_encrypted: changes
            .secret_token
            .as_ref()
            .map(|token| cipher.seal(token.expose_secret().as_bytes(), &secret_token_context(id))),
        priority: changes.priority,
        is_active: changes.is_active,
        expires_at: changes.expires_at,
    };

    diesel::update(llms::table.find(id))
        .set(changeset)
        .returning(Llm::as_returning())
        .get_result(connection)
        .await
}

/// Deletes one credential.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(llms::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use crate::errors::ApiError;
    use crate::test_support::{cipher, migrated_database};

    fn new_llm(name: &str, priority: i32) -> NewLlm {
        NewLlm {
            name: name.to_owned(),
            description: "test credential".to_owned(),
            type_: LlmType::ClaudeApiToken,
            secret_token: SecretString::from(format!("sk-ant-{name}")),
            priority,
            is_active: true,
            expires_at: Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn created_credentials_store_only_ciphertext_and_decrypt_back() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let created = create(&mut connection, &cipher, &new_llm("primary", 1))
            .await
            .expect("insert");
        let fetched = find(&mut connection, created.id).await.expect("select");

        assert_eq!(fetched.name, "primary");
        assert_eq!(fetched.type_, LlmType::ClaudeApiToken);
        assert_eq!(
            fetched.expires_at,
            Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap())
        );
        assert_eq!(fetched.id.get_version_num(), 7);

        let plaintext = b"sk-ant-primary";
        assert!(
            !fetched
                .secret_token_encrypted
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );
        assert_eq!(
            fetched
                .secret_token(&cipher)
                .expect("decrypts")
                .expose_secret(),
            "sk-ant-primary"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_token_copied_into_another_row_refuses_to_open() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let victim = create(&mut connection, &cipher, &new_llm("victim", 1))
            .await
            .expect("insert");
        let mut attacker = create(&mut connection, &cipher, &new_llm("attacker", 2))
            .await
            .expect("insert");

        attacker
            .secret_token_encrypted
            .clone_from(&victim.secret_token_encrypted);
        assert_eq!(
            attacker.secret_token(&cipher).unwrap_err(),
            OpenError::Inauthentic
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn update_touches_only_supplied_fields_and_reseals_tokens() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(&mut connection, &cipher, &new_llm("rotating", 5))
            .await
            .expect("insert");

        // Postgres timestamps have microsecond precision; make sure the trigger's
        // new updated_at is observably later.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let changes = LlmChanges {
            is_active: Some(false),
            secret_token: Some(SecretString::from("sk-ant-rotated")),
            expires_at: Some(None),
            ..LlmChanges::default()
        };
        let updated = update(&mut connection, &cipher, original.id, &changes)
            .await
            .expect("update");

        assert!(!updated.is_active);
        assert_eq!(updated.expires_at, None, "Some(None) clears the column");
        assert_eq!(updated.name, original.name, "absent fields are untouched");
        assert_eq!(updated.priority, original.priority);
        assert_eq!(updated.created_at, original.created_at);
        assert!(
            updated.updated_at > original.updated_at,
            "trigger bumps updated_at"
        );
        assert_ne!(
            updated.secret_token_encrypted,
            original.secret_token_encrypted
        );
        assert_eq!(
            updated
                .secret_token(&cipher)
                .expect("decrypts")
                .expose_secret(),
            "sk-ant-rotated"
        );

        let missing = update(&mut connection, &cipher, Uuid::now_v7(), &changes)
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(missing), ApiError::NotFound));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn list_orders_by_priority_then_age_and_names_are_unique() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let low = create(&mut connection, &cipher, &new_llm("fallback", 10))
            .await
            .expect("insert");
        let first = create(&mut connection, &cipher, &new_llm("first", 0))
            .await
            .expect("insert");
        let second = create(&mut connection, &cipher, &new_llm("second", 0))
            .await
            .expect("insert");

        let ids: Vec<Uuid> = list(&mut connection)
            .await
            .expect("list")
            .into_iter()
            .map(|llm| llm.id)
            .collect();
        assert_eq!(ids, vec![first.id, second.id, low.id]);

        let duplicate = create(&mut connection, &cipher, &new_llm("first", 3))
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(duplicate), ApiError::Conflict(_)));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn delete_removes_the_row_and_reports_unknown_ids() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let doomed = create(&mut connection, &cipher, &new_llm("doomed", 1))
            .await
            .expect("insert");

        delete(&mut connection, doomed.id).await.expect("delete");
        assert!(matches!(
            ApiError::from(find(&mut connection, doomed.id).await.unwrap_err()),
            ApiError::NotFound
        ));
        assert!(matches!(
            ApiError::from(delete(&mut connection, doomed.id).await.unwrap_err()),
            ApiError::NotFound
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn database_constraints_back_up_request_validation() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let mut blank = new_llm("", 1);
        blank.name = String::new();
        create(&mut connection, &cipher, &blank)
            .await
            .expect_err("llms_name_length rejects an empty name");

        let expiry = Utc::now() + Duration::days(30);
        let mut future = new_llm("future", 1);
        future.expires_at = Some(expiry);
        let stored = create(&mut connection, &cipher, &future)
            .await
            .expect("insert");
        assert_eq!(
            stored.expires_at.expect("present").timestamp_micros(),
            expiry.timestamp_micros()
        );
    }
}
