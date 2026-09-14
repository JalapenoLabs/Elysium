// Copyright © 2026 Jalapeno Labs

//! Arsox satellites Elysium can run coding sessions on.
//!
//! The bearer secret never exists in Postgres as plaintext. It is sealed here, bound
//! to the row's id, before the insert or update leaves the process. Callers hand in a
//! [`SecretString`] and only ever get one back from [`Satellite::secret`].

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::satellites;

/// A stored satellite. `secret_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = satellites, check_for_backend(diesel::pg::Pg))]
pub struct Satellite {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub url: String,
    pub secret_encrypted: Vec<u8>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for a new satellite, with the secret still in plaintext.
#[derive(Debug)]
pub struct NewSatellite {
    pub name: String,
    pub description: String,
    pub url: String,
    pub secret: SecretString,
    pub is_active: bool,
}

/// A partial update. `None` leaves a column untouched.
#[derive(Debug, Default)]
pub struct SatelliteChanges {
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub secret: Option<SecretString>,
    pub is_active: Option<bool>,
}

impl SatelliteChanges {
    /// True when applying these changes would not touch any column.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.url.is_none()
            && self.secret.is_none()
            && self.is_active.is_none()
    }
}

/// Row-shaped insert, holding the already sealed secret.
#[derive(Insertable)]
#[diesel(table_name = satellites)]
struct SatelliteRow<'a> {
    id: Uuid,
    name: &'a str,
    description: &'a str,
    url: &'a str,
    secret_encrypted: Vec<u8>,
    is_active: bool,
}

/// Row-shaped update, holding the already sealed secret if one was supplied.
#[derive(AsChangeset)]
#[diesel(table_name = satellites)]
struct SatelliteChangeset<'a> {
    name: Option<&'a str>,
    description: Option<&'a str>,
    url: Option<&'a str>,
    secret_encrypted: Option<Vec<u8>>,
    is_active: Option<bool>,
}

/// Associated data binding a sealed secret to its row, so a ciphertext copied into
/// another row refuses to open.
fn secret_context(id: Uuid) -> Vec<u8> {
    let mut context = b"satellites.secret:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl Satellite {
    /// Decrypts this satellite's bearer secret.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or
    /// copied from another row.
    pub fn secret(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(&self.secret_encrypted, &secret_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Every satellite, alphabetically.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Satellite>> {
    satellites::table
        .order(satellites::name.asc())
        .select(Satellite::as_select())
        .load(connection)
        .await
}

/// One satellite by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Satellite> {
    satellites::table
        .find(id)
        .select(Satellite::as_select())
        .first(connection)
        .await
}

/// Seals the secret and inserts the satellite with a new `UUIDv7` id.
///
/// # Errors
/// Propagates database errors, including a unique violation on `name`.
pub async fn create(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_satellite: &NewSatellite,
) -> QueryResult<Satellite> {
    let id = Uuid::now_v7();
    let row = SatelliteRow {
        id,
        name: &new_satellite.name,
        description: &new_satellite.description,
        url: &new_satellite.url,
        secret_encrypted: cipher.seal(
            new_satellite.secret.expose_secret().as_bytes(),
            &secret_context(id),
        ),
        is_active: new_satellite.is_active,
    };

    diesel::insert_into(satellites::table)
        .values(row)
        .returning(Satellite::as_returning())
        .get_result(connection)
        .await
}

/// Applies `changes`, re-sealing the secret under this row's id if one is given.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any
/// other database error, including a unique violation on `name`.
pub async fn update(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    id: Uuid,
    changes: &SatelliteChanges,
) -> QueryResult<Satellite> {
    let changeset = SatelliteChangeset {
        name: changes.name.as_deref(),
        description: changes.description.as_deref(),
        url: changes.url.as_deref(),
        secret_encrypted: changes
            .secret
            .as_ref()
            .map(|secret| cipher.seal(secret.expose_secret().as_bytes(), &secret_context(id))),
        is_active: changes.is_active,
    };

    diesel::update(satellites::table.find(id))
        .set(changeset)
        .returning(Satellite::as_returning())
        .get_result(connection)
        .await
}

/// Deletes one satellite, and with it every coding session recorded against it.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(satellites::table.find(id))
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

    fn new_satellite(name: &str) -> NewSatellite {
        NewSatellite {
            name: name.to_owned(),
            description: "test satellite".to_owned(),
            url: "http://arsox:8080".to_owned(),
            secret: SecretString::from(format!("bearer-{name}")),
            is_active: true,
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn created_satellites_store_only_ciphertext_and_decrypt_back() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let created = create(&mut connection, &cipher, &new_satellite("orbit"))
            .await
            .expect("insert");
        let fetched = find(&mut connection, created.id).await.expect("select");

        assert_eq!(fetched.url, "http://arsox:8080");
        assert_eq!(fetched.id.get_version_num(), 7);
        let plaintext = b"bearer-orbit";
        assert!(
            !fetched
                .secret_encrypted
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );
        assert_eq!(
            fetched.secret(&cipher).expect("decrypts").expose_secret(),
            "bearer-orbit"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_secret_copied_into_another_row_refuses_to_open() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let victim = create(&mut connection, &cipher, &new_satellite("victim"))
            .await
            .expect("insert");
        let mut attacker = create(&mut connection, &cipher, &new_satellite("attacker"))
            .await
            .expect("insert");

        attacker
            .secret_encrypted
            .clone_from(&victim.secret_encrypted);
        assert_eq!(
            attacker.secret(&cipher).unwrap_err(),
            OpenError::Inauthentic
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn update_reseals_secrets_and_names_stay_unique() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(&mut connection, &cipher, &new_satellite("rotating"))
            .await
            .expect("insert");
        create(&mut connection, &cipher, &new_satellite("taken"))
            .await
            .expect("insert");

        let changes = SatelliteChanges {
            secret: Some(SecretString::from("bearer-rotated")),
            is_active: Some(false),
            ..SatelliteChanges::default()
        };
        let updated = update(&mut connection, &cipher, original.id, &changes)
            .await
            .expect("update");
        assert!(!updated.is_active);
        assert_eq!(updated.url, original.url, "absent fields are untouched");
        assert_eq!(
            updated.secret(&cipher).expect("decrypts").expose_secret(),
            "bearer-rotated"
        );

        let rename = SatelliteChanges {
            name: Some("taken".to_owned()),
            ..SatelliteChanges::default()
        };
        let duplicate = update(&mut connection, &cipher, original.id, &rename)
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(duplicate), ApiError::Conflict(_)));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_database_refuses_urls_without_an_http_scheme() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let mut schemeless = new_satellite("schemeless");
        schemeless.url = "arsox:8080".to_owned();
        create(&mut connection, &cipher, &schemeless)
            .await
            .expect_err("satellites_url_scheme rejects a URL without http(s)://");
    }
}
