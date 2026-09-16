// Copyright © 2026 Jalapeno Labs

//! Global environment variables, passed into every coding session's satellite thread.
//!
//! Every value is sealed here, bound to the row's id, before the insert or update leaves the
//! process, whether the variable is secret or not. One path for every row keeps the table
//! uniform and means a variable can become secret without its value moving. Callers hand
//! in a [`SecretString`] and only ever get one back, from [`EnvironmentVariable::value`] or
//! [`thread_environment`].
//!
//! Which keys may be stored is decided by [`crate::environment::key_refusal`], before a
//! request reaches these queries.

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::environment_variables;

/// A stored variable. `value_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = environment_variables, check_for_backend(diesel::pg::Pg))]
pub struct EnvironmentVariable {
    pub id: Uuid,
    pub key: String,
    pub value_encrypted: Vec<u8>,
    /// Hidden from every response, and scrubbed from thread output by the satellite. The
    /// agent can still read it.
    pub is_secret: bool,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for a new variable, with the value still in plaintext.
#[derive(Debug)]
pub struct NewEnvironmentVariable {
    pub key: String,
    pub value: SecretString,
    pub is_secret: bool,
    pub description: String,
}

/// A partial update. `None` leaves a column untouched.
#[derive(Debug, Default)]
pub struct EnvironmentVariableChanges {
    pub key: Option<String>,
    pub value: Option<SecretString>,
    pub is_secret: Option<bool>,
    pub description: Option<String>,
}

impl EnvironmentVariableChanges {
    /// True when applying these changes would not touch any column.
    pub const fn is_empty(&self) -> bool {
        self.key.is_none()
            && self.value.is_none()
            && self.is_secret.is_none()
            && self.description.is_none()
    }
}

/// One variable as a satellite thread receives it, decrypted.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "read when a coding session opens its thread; remove once that caller lands"
    )
)]
#[derive(Debug)]
pub struct ThreadVariable {
    pub key: String,
    pub value: SecretString,
    /// Whether the satellite scrubs the value from what the thread outputs: its logs,
    /// events, and rendered commands. It does **not** hide the value from the agent, which
    /// runs with every variable in its environment and can print, use, or send it anywhere
    /// its tools reach. A value that must stay hidden from the agent does not belong here.
    pub is_secret: bool,
}

/// Row-shaped insert, holding the already sealed value.
#[derive(Insertable)]
#[diesel(table_name = environment_variables)]
struct EnvironmentVariableRow<'a> {
    id: Uuid,
    key: &'a str,
    value_encrypted: Vec<u8>,
    is_secret: bool,
    description: &'a str,
}

/// Row-shaped update.
#[derive(AsChangeset)]
#[diesel(table_name = environment_variables)]
struct EnvironmentVariableChangeset<'a> {
    key: Option<&'a str>,
    value_encrypted: Option<Vec<u8>>,
    is_secret: Option<bool>,
    description: Option<&'a str>,
}

/// Associated data binding a sealed value to its row, so a ciphertext copied into another
/// row refuses to open. The id never changes, so renaming a key keeps the value readable.
fn value_context(id: Uuid) -> Vec<u8> {
    let mut context = b"environment_variables.value:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl EnvironmentVariable {
    /// Decrypts this variable's value.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or
    /// copied from another row.
    pub fn value(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(&self.value_encrypted, &value_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Every variable, by key.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<EnvironmentVariable>> {
    environment_variables::table
        .order(environment_variables::key.asc())
        .select(EnvironmentVariable::as_select())
        .load(connection)
        .await
}

/// One variable by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<EnvironmentVariable> {
    environment_variables::table
        .find(id)
        .select(EnvironmentVariable::as_select())
        .first(connection)
        .await
}

/// Every variable, decrypted and ordered by key, ready to hand to a satellite thread.
///
/// Secret only means the satellite scrubs a value from the thread's output; see
/// [`ThreadVariable::is_secret`]. The agent reads every value.
///
/// # Errors
/// Propagates database errors, and fails naming the key of any value that will not open,
/// which only a changed encryption key or altered row can cause. The value itself is never
/// part of the error.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "read when a coding session opens its thread; remove once that caller lands"
    )
)]
pub async fn thread_environment(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
) -> anyhow::Result<Vec<ThreadVariable>> {
    let variables = list(connection)
        .await
        .context("failed to load environment variables")?;

    variables
        .into_iter()
        .map(|variable| {
            let value = variable.value(cipher).with_context(|| {
                format!(
                    "environment variable {} could not be decrypted",
                    variable.key
                )
            })?;
            Ok(ThreadVariable {
                key: variable.key,
                value,
                is_secret: variable.is_secret,
            })
        })
        .collect()
}

/// Seals the value and inserts the variable with a new `UUIDv7` id.
///
/// # Errors
/// Propagates database errors, including a unique violation on `key`.
pub async fn create(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_variable: &NewEnvironmentVariable,
) -> QueryResult<EnvironmentVariable> {
    let id = Uuid::now_v7();
    let row = EnvironmentVariableRow {
        id,
        key: &new_variable.key,
        value_encrypted: cipher.seal(
            new_variable.value.expose_secret().as_bytes(),
            &value_context(id),
        ),
        is_secret: new_variable.is_secret,
        description: &new_variable.description,
    };

    diesel::insert_into(environment_variables::table)
        .values(row)
        .returning(EnvironmentVariable::as_returning())
        .get_result(connection)
        .await
}

/// Applies `changes`, re-sealing the value under this row's id if one is given.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any other
/// database error, including a unique violation on `key`.
pub async fn update(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    id: Uuid,
    changes: &EnvironmentVariableChanges,
) -> QueryResult<EnvironmentVariable> {
    let changeset = EnvironmentVariableChangeset {
        key: changes.key.as_deref(),
        value_encrypted: changes
            .value
            .as_ref()
            .map(|value| cipher.seal(value.expose_secret().as_bytes(), &value_context(id))),
        is_secret: changes.is_secret,
        description: changes.description.as_deref(),
    };

    diesel::update(environment_variables::table.find(id))
        .set(changeset)
        .returning(EnvironmentVariable::as_returning())
        .get_result(connection)
        .await
}

/// Deletes one variable.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(environment_variables::table.find(id))
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

    fn new_variable(key: &str, value: &str, is_secret: bool) -> NewEnvironmentVariable {
        NewEnvironmentVariable {
            key: key.to_owned(),
            value: SecretString::from(value),
            is_secret,
            description: String::new(),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn every_value_is_stored_only_as_ciphertext_and_decrypts_back() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        for (key, is_secret) in [("NPM_TOKEN", true), ("NODE_ENV", false)] {
            let created = create(
                &mut connection,
                &cipher,
                &new_variable(key, "plaintext-value", is_secret),
            )
            .await
            .expect("insert");
            let fetched = find(&mut connection, created.id).await.expect("select");

            assert_eq!(fetched.key, key);
            assert_eq!(fetched.is_secret, is_secret);
            assert_eq!(fetched.id.get_version_num(), 7);

            let plaintext = b"plaintext-value";
            assert!(
                !fetched
                    .value_encrypted
                    .windows(plaintext.len())
                    .any(|window| window == plaintext)
            );
            assert_eq!(
                fetched.value(&cipher).expect("decrypts").expose_secret(),
                "plaintext-value"
            );
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_empty_value_seals_to_the_smallest_envelope() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let created = create(&mut connection, &cipher, &new_variable("EMPTY", "", false))
            .await
            .expect("an empty value satisfies environment_variables_value_sealed");

        assert_eq!(
            created.value(&cipher).expect("decrypts").expose_secret(),
            ""
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_value_copied_into_another_row_refuses_to_open() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let victim = create(&mut connection, &cipher, &new_variable("VICTIM", "a", true))
            .await
            .expect("insert");
        let mut attacker = create(
            &mut connection,
            &cipher,
            &new_variable("ATTACKER", "b", true),
        )
        .await
        .expect("insert");

        attacker.value_encrypted.clone_from(&victim.value_encrypted);
        assert_eq!(attacker.value(&cipher).unwrap_err(), OpenError::Inauthentic);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn updates_touch_only_the_columns_they_name() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(
            &mut connection,
            &cipher,
            &new_variable("REGISTRY_TOKEN", "first", false),
        )
        .await
        .expect("insert");

        let flipped = update(
            &mut connection,
            &cipher,
            original.id,
            &EnvironmentVariableChanges {
                key: Some("NPM_TOKEN".to_owned()),
                is_secret: Some(true),
                ..EnvironmentVariableChanges::default()
            },
        )
        .await
        .expect("update");

        assert_eq!(flipped.key, "NPM_TOKEN");
        assert!(flipped.is_secret);
        assert_eq!(flipped.value_encrypted, original.value_encrypted);
        assert_eq!(
            flipped.value(&cipher).expect("decrypts").expose_secret(),
            "first"
        );

        let revalued = update(
            &mut connection,
            &cipher,
            original.id,
            &EnvironmentVariableChanges {
                value: Some(SecretString::from("second")),
                description: Some("The registry token".to_owned()),
                ..EnvironmentVariableChanges::default()
            },
        )
        .await
        .expect("update");

        assert_eq!(revalued.description, "The registry token");
        assert_eq!(
            revalued.value(&cipher).expect("decrypts").expose_secret(),
            "second"
        );

        let missing = update(
            &mut connection,
            &cipher,
            Uuid::now_v7(),
            &EnvironmentVariableChanges {
                is_secret: Some(false),
                ..EnvironmentVariableChanges::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(ApiError::from(missing), ApiError::NotFound));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_thread_environment_is_every_variable_decrypted_by_key() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        for (key, value, is_secret) in [
            ("ZED_SETTING", "last", false),
            ("NPM_TOKEN", "npm_secret", true),
            ("CI", "true", false),
        ] {
            create(
                &mut connection,
                &cipher,
                &new_variable(key, value, is_secret),
            )
            .await
            .expect("insert");
        }

        let environment = thread_environment(&mut connection, &cipher)
            .await
            .expect("every value opens");
        let received: Vec<(&str, &str, bool)> = environment
            .iter()
            .map(|variable| {
                (
                    variable.key.as_str(),
                    variable.value.expose_secret(),
                    variable.is_secret,
                )
            })
            .collect();

        assert_eq!(
            received,
            [
                ("CI", "true", false),
                ("NPM_TOKEN", "npm_secret", true),
                ("ZED_SETTING", "last", false),
            ]
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_value_that_will_not_open_fails_the_thread_environment_by_key() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        create(
            &mut connection,
            &cipher,
            &new_variable("NPM_TOKEN", "npm_secret", true),
        )
        .await
        .expect("insert");

        let error = thread_environment(&mut connection, &crate::test_support::cipher())
            .await
            .expect_err("another key cannot open the value");
        let message = format!("{error:#}");

        assert!(message.contains("NPM_TOKEN"));
        assert!(!message.contains("npm_secret"));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn keys_are_unique_and_deletes_report_unknown_ids() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let first = create(&mut connection, &cipher, &new_variable("CI", "true", false))
            .await
            .expect("insert");
        let duplicate = create(
            &mut connection,
            &cipher,
            &new_variable("CI", "false", false),
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

        for key in ["", "1PASSWORD", "HAS SPACE", &"A".repeat(129)] {
            create(&mut connection, &cipher, &new_variable(key, "value", false))
                .await
                .expect_err("environment_variables_key_shape and _length refuse the key");
        }

        let mut described = new_variable("DESCRIBED", "value", false);
        described.description = "d".repeat(501);
        create(&mut connection, &cipher, &described)
            .await
            .expect_err("environment_variables_description_length refuses 501 characters");
    }
}
