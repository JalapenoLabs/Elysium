// Copyright © 2026 Jalapeno Labs

//! The name a person gives their authenticator app, such as "Bitwarden".
//!
//! Kratos holds the app, one per identity, and has nowhere to name it, so the name is kept
//! here, one row per account. Kratos says whether the app exists; this only names it, so a
//! name with no app behind it is never shown. It is removed with the app. See `docs/auth.md`.

use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::authenticator_nicknames;

/// The longest nickname `authenticator_nicknames_length` allows, in characters.
pub const NICKNAME_MAX_CHARACTERS: usize = 50;

/// The person's nickname for their authenticator app, if they gave one.
pub async fn find(
    connection: &mut AsyncPgConnection,
    user_id: Uuid,
) -> QueryResult<Option<String>> {
    authenticator_nicknames::table
        .find(user_id)
        .select(authenticator_nicknames::nickname)
        .first(connection)
        .await
        .optional()
}

/// Names the person's authenticator app, replacing any name it had. `nickname` must already
/// be trimmed and within [`NICKNAME_MAX_CHARACTERS`].
pub async fn set(
    connection: &mut AsyncPgConnection,
    user_id: Uuid,
    nickname: &str,
) -> QueryResult<()> {
    diesel::insert_into(authenticator_nicknames::table)
        .values((
            authenticator_nicknames::user_id.eq(user_id),
            authenticator_nicknames::nickname.eq(nickname),
        ))
        .on_conflict(authenticator_nicknames::user_id)
        .do_update()
        .set(authenticator_nicknames::nickname.eq(nickname))
        .execute(connection)
        .await
        .map(drop)
}

/// Forgets the name, as when the app is removed. Nothing to forget is not an error.
pub async fn clear(connection: &mut AsyncPgConnection, user_id: Uuid) -> QueryResult<()> {
    diesel::delete(authenticator_nicknames::table.find(user_id))
        .execute(connection)
        .await
        .map(drop)
}

#[cfg(test)]
mod tests {
    use diesel_async::AsyncConnection;
    use secrecy::ExposeSecret;

    use super::*;
    use crate::database::migrations::{self, MigrationCommand};
    use crate::models::user::{self, Profile};
    use crate::test_support::empty_database;

    /// A migrated database holding one person, and their id.
    async fn database_with_a_person() -> (AsyncPgConnection, Uuid) {
        let url = empty_database().await;
        migrations::execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("migrations apply");
        let mut connection = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("connects");
        let profile = Profile::new(Uuid::now_v7(), "ada@example.com", "Ada Lovelace");
        let ada = user::provision(&mut connection, &profile)
            .await
            .expect("provisions");
        (connection, ada.id)
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_nickname_is_set_replaced_and_cleared() {
        let (mut connection, ada) = database_with_a_person().await;

        assert_eq!(find(&mut connection, ada).await.expect("reads"), None);
        set(&mut connection, ada, "Bitwarden").await.expect("sets");
        set(&mut connection, ada, "1Password")
            .await
            .expect("replaces");
        assert_eq!(
            find(&mut connection, ada).await.expect("reads").as_deref(),
            Some("1Password"),
        );

        clear(&mut connection, ada).await.expect("clears");
        clear(&mut connection, ada)
            .await
            .expect("clearing nothing is fine");
        assert_eq!(find(&mut connection, ada).await.expect("reads"), None);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_table_refuses_what_the_route_would() {
        let (mut connection, ada) = database_with_a_person().await;

        assert!(set(&mut connection, ada, "").await.is_err());
        assert!(set(&mut connection, ada, " Bitwarden").await.is_err());
        let too_long = "x".repeat(NICKNAME_MAX_CHARACTERS + 1);
        assert!(set(&mut connection, ada, &too_long).await.is_err());
    }
}
