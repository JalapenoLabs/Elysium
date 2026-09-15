// Copyright © 2026 Jalapeno Labs

//! Mail domains the bundled mail server hosts.
//!
//! A row mirrors a domain in Stalwart, which is the source of its DNS records and DKIM
//! keys; the row keeps Stalwart's id so the domain can be removed there, and lets
//! self-hosted mailboxes reference their domain.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::{mail_accounts, mail_domains};

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = mail_domains, check_for_backend(diesel::pg::Pg))]
pub struct MailDomain {
    pub id: Uuid,
    pub name: String,
    pub stalwart_id: String,
    /// The domain the server was created with, which cannot be removed.
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = mail_domains)]
struct MailDomainRow<'a> {
    id: Uuid,
    name: String,
    stalwart_id: &'a str,
    is_default: bool,
}

/// Every domain, alphabetically.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<MailDomain>> {
    mail_domains::table
        .order(mail_domains::name.asc())
        .select(MailDomain::as_select())
        .load(connection)
        .await
}

/// One domain by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<MailDomain> {
    mail_domains::table
        .find(id)
        .select(MailDomain::as_select())
        .first(connection)
        .await
}

/// Records a domain Stalwart now hosts, with a new `UUIDv7` id. The name is lowercased.
/// `is_default` is set only for the domain the server was created with.
///
/// # Errors
/// Propagates database errors, including a unique violation on `name`, or on a second
/// default domain.
pub async fn create(
    connection: &mut AsyncPgConnection,
    name: &str,
    stalwart_id: &str,
    is_default: bool,
) -> QueryResult<MailDomain> {
    diesel::insert_into(mail_domains::table)
        .values(MailDomainRow {
            id: Uuid::now_v7(),
            name: name.to_lowercase(),
            stalwart_id,
            is_default,
        })
        .returning(MailDomain::as_returning())
        .get_result(connection)
        .await
}

/// How many mailboxes live on the domain. A domain with any cannot be removed.
///
/// # Errors
/// Propagates any database error.
pub async fn mailbox_count(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<i64> {
    mail_accounts::table
        .filter(mail_accounts::mail_domain_id.eq(id))
        .count()
        .get_result(connection)
        .await
}

/// Deletes one domain row. Removing the domain from Stalwart is the caller's job, done
/// first.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id, and a foreign
/// key violation when a mailbox still references it.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(mail_domains::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;
    use crate::models::mail_account::{self, MailAccountKind, NewMailAccount};
    use crate::test_support::{cipher, migrated_database};

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_domain_with_mailboxes_cannot_be_deleted() {
        let (_url, mut connection) = migrated_database().await;
        let domain = create(&mut connection, "Alpha.Test", "b", true)
            .await
            .expect("insert");
        assert_eq!(domain.name, "alpha.test");
        create(&mut connection, "alpha.test", "c", false)
            .await
            .expect_err("names are unique regardless of case");
        create(&mut connection, "beta.test", "c", true)
            .await
            .expect_err("only one domain is the default");

        mail_account::create(
            &mut connection,
            &cipher(),
            &NewMailAccount {
                kind: MailAccountKind::SelfHosted,
                address: "agent@alpha.test".to_owned(),
                display_name: String::new(),
                credential: SecretString::from("password"),
                external_id: Some("d".to_owned()),
                mail_domain_id: Some(domain.id),
            },
        )
        .await
        .expect("mailbox");

        assert_eq!(
            mailbox_count(&mut connection, domain.id)
                .await
                .expect("count"),
            1
        );
        delete(&mut connection, domain.id)
            .await
            .expect_err("the mailbox still references the domain");
    }
}
