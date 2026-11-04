// Copyright © 2026 Jalapeno Labs

//! Secrets a migration copied into new rows, re-sealed for those rows when the API starts. See
//! `docs/secrets.md`.
//!
//! Every sealed secret is bound to the row it belongs to, and SQL cannot seal: the key is
//! the API's alone. So a migration that splits one row's secret into two (an Atlassian account
//! shared by two connectors, say) copies the envelope as it is, still bound to the row it came
//! from, and lists the copy in `secret_copies`. [`reseal_all`] runs once at startup, before
//! anything reads a secret, and re-seals each copy for its own row.

use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use tracing::{Level, event};
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::database::schema::secret_copies;
use crate::models::jira_credential;

/// A secret a migration copied, and the row it is still sealed for.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = secret_copies, check_for_backend(diesel::pg::Pg))]
pub struct SecretCopy {
    /// Where the copy lives, as `<table>.<secret>`.
    pub target: String,
    pub row_id: Uuid,
    pub sealed_for: Uuid,
}

/// The secrets a copy can be of. The `secret_copies.target` check lists the same ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    JiraToken,
}

impl Target {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "jira_credentials.token" => Some(Self::JiraToken),
            _ => None,
        }
    }
}

/// Re-seals every listed copy for its own row and removes it from the list, each in a
/// transaction of its own. Answers how many were re-sealed.
///
/// A copy that cannot be re-sealed is logged and left listed, so it is tried again at the next
/// start; its row keeps refusing to open until then, as any secret sealed under another key
/// does. A copy whose row was removed since is simply forgotten.
///
/// # Errors
/// Propagates a database error reading the list.
pub async fn reseal_all(connection: &mut AsyncPgConnection, cipher: &Cipher) -> QueryResult<usize> {
    let copies: Vec<SecretCopy> = secret_copies::table
        .select(SecretCopy::as_select())
        .load(connection)
        .await?;

    let mut resealed = 0;
    for copy in copies {
        let Some(target) = Target::parse(&copy.target) else {
            event!(
                name: "secrets.copy.unknown_target",
                Level::WARN,
                secret.target = %copy.target,
                "a copied secret names a target this build does not know; leaving it listed",
            );
            continue;
        };
        let outcome = connection
            .transaction(async |connection| {
                let found = match target {
                    Target::JiraToken => {
                        jira_credential::reseal_token(
                            connection,
                            cipher,
                            copy.row_id,
                            copy.sealed_for,
                        )
                        .await?
                    }
                };
                diesel::delete(secret_copies::table.find((&copy.target, copy.row_id)))
                    .execute(connection)
                    .await?;
                anyhow::Ok(found)
            })
            .await;
        match outcome {
            Ok(true) => resealed += 1,
            Ok(false) => event!(
                name: "secrets.copy.row_gone",
                Level::INFO,
                secret.target = %copy.target,
                db.row.id = %copy.row_id,
                "a copied secret's row was removed before it was re-sealed",
            ),
            Err(error) => event!(
                name: "secrets.copy.reseal_failed",
                Level::ERROR,
                secret.target = %copy.target,
                db.row.id = %copy.row_id,
                error.message = %error,
                "a copied secret could not be re-sealed; it is tried again at the next start",
            ),
        }
    }
    Ok(resealed)
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;
    use crate::jira::Account;
    use crate::models::jira_credential::{Allowed, Allowlist, NewJiraCredential, VerifiedToken};
    use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_copied_token_opens_for_its_own_row_once_resealed() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = jira_credential::create(
            &mut connection,
            &cipher,
            &NewJiraCredential {
                name: "Work".to_owned(),
                verified: VerifiedToken {
                    site_url: "https://acme.atlassian.net".to_owned(),
                    account_email: "ada@acme.test".to_owned(),
                    token: "atlassian-token".into(),
                    account: Account {
                        account_id: "acc-1".to_owned(),
                        display_name: "Ada".to_owned(),
                        email: None,
                    },
                },
                allowed: Allowed {
                    projects: Allowlist::All,
                    boards: Allowlist::All,
                },
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("the original is stored");

        // What the migration does: the envelope copied as it is, and the copy listed.
        let copy_id = Uuid::now_v7();
        diesel::sql_query(
            "INSERT INTO jira_credentials (id, name, site_url, account_email, token_encrypted, \
             account_id, display_name, created_by) \
             SELECT $1, 'Work (Confluence)', site_url, account_email, token_encrypted, account_id, \
             display_name, created_by FROM jira_credentials WHERE id = $2",
        )
        .bind::<diesel::sql_types::Uuid, _>(copy_id)
        .bind::<diesel::sql_types::Uuid, _>(original.id)
        .execute(&mut connection)
        .await
        .expect("the copy is stored");
        diesel::insert_into(secret_copies::table)
            .values((
                secret_copies::target.eq("jira_credentials.token"),
                secret_copies::row_id.eq(copy_id),
                secret_copies::sealed_for.eq(original.id),
            ))
            .execute(&mut connection)
            .await
            .expect("the copy is listed");

        let copy = jira_credential::find(&mut connection, copy_id)
            .await
            .expect("found");
        copy.token(&cipher)
            .expect_err("a copy refuses to open before it is re-sealed");

        assert_eq!(reseal_all(&mut connection, &cipher).await.expect("runs"), 1);
        let copy = jira_credential::find(&mut connection, copy_id)
            .await
            .expect("found");
        assert_eq!(
            copy.token(&cipher).expect("opens").expose_secret(),
            "atlassian-token"
        );
        assert_eq!(
            reseal_all(&mut connection, &cipher).await.expect("runs"),
            0,
            "nothing is left listed"
        );
    }
}
