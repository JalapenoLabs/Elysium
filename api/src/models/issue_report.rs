// Copyright © 2026 Jalapeno Labs

//! The problems people reported to Elysium's GitHub repository from the app, and where each
//! went. See `crate::issue_reports` and `docs/issue-reporting.md`.
//!
//! A row is written only once GitHub accepted the issue or the comment, so every row names
//! something that exists on GitHub. Nothing that was posted is kept: GitHub holds it.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

use crate::database::schema::{
    github_credentials, issue_reports, jira_credentials, mail_accounts, token_accounts, users,
};

/// How a report reached GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, diesel_derive_enum::DbEnum, Serialize)]
#[ExistingTypePath = "crate::database::schema::sql_types::IssueReportFiling"]
#[serde(rename_all = "snake_case")]
pub enum Filing {
    /// It opened a new issue: nobody had reported the problem, or its issue was closed.
    Issue,
    /// It was added to the open issue another report of the same problem opened.
    Comment,
}

/// One report, as stored.
#[derive(Debug, Clone, Queryable, Selectable, Insertable)]
#[diesel(table_name = issue_reports, check_for_backend(diesel::pg::Pg))]
pub struct IssueReport {
    pub id: Uuid,
    /// The problem's fingerprint, the same for everyone who hits it.
    pub fingerprint: String,
    pub issue_number: i64,
    pub issue_url: String,
    pub filed_as: Filing,
    /// Where this report reads: the issue it opened, or its comment.
    pub url: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Stores a report GitHub accepted.
///
/// # Errors
/// Propagates any database error, such as a fingerprint that is not 16 hex characters.
pub async fn record(
    connection: &mut AsyncPgConnection,
    report: &IssueReport,
) -> QueryResult<IssueReport> {
    diesel::insert_into(issue_reports::table)
        .values(report.clone())
        .returning(IssueReport::as_returning())
        .get_result(connection)
        .await
}

/// How many reports `person` made since `since`, and when the oldest of them was made, which
/// is when the first of them stops counting against the limit.
///
/// # Errors
/// Propagates any database error.
pub async fn made_since(
    connection: &mut AsyncPgConnection,
    person: Uuid,
    since: DateTime<Utc>,
) -> QueryResult<(i64, Option<DateTime<Utc>>)> {
    issue_reports::table
        .filter(issue_reports::created_by.eq(person))
        .filter(issue_reports::created_at.gt(since))
        .select((
            diesel::dsl::count_star(),
            diesel::dsl::min(issue_reports::created_at),
        ))
        .get_result(connection)
        .await
}

/// The newest report of `fingerprint` by anyone, which names the issue the problem last went to.
///
/// # Errors
/// Propagates any database error.
pub async fn latest_of(
    connection: &mut AsyncPgConnection,
    fingerprint: &str,
) -> QueryResult<Option<IssueReport>> {
    issue_reports::table
        .filter(issue_reports::fingerprint.eq(fingerprint))
        .order(issue_reports::created_at.desc())
        .select(IssueReport::as_select())
        .first(connection)
        .await
        .optional()
}

/// The newest report of `fingerprint` that `person` made, if any.
///
/// # Errors
/// Propagates any database error.
pub async fn latest_by(
    connection: &mut AsyncPgConnection,
    person: Uuid,
    fingerprint: &str,
) -> QueryResult<Option<IssueReport>> {
    issue_reports::table
        .filter(issue_reports::created_by.eq(person))
        .filter(issue_reports::fingerprint.eq(fingerprint))
        .order(issue_reports::created_at.desc())
        .select(IssueReport::as_select())
        .first(connection)
        .await
        .optional()
}

/// Every value the workspace holds that names a person or the accounts they sign in as: people's
/// names and addresses, GitHub logins, Atlassian accounts and sites, token accounts' identities
/// and sites, and mailboxes. A report redacts each of them wherever an error repeats one, since
/// a pattern cannot tell a name from any other word.
///
/// Labels people give their own accounts, such as a token named `Work`, are left out: they name
/// no one, and redacting a common word everywhere it appears would hide the error itself.
///
/// # Errors
/// Propagates any database error.
pub async fn personal_values(connection: &mut AsyncPgConnection) -> QueryResult<Vec<String>> {
    let mut values: Vec<String> = Vec::new();

    // People alone: the seeded machines are named `Elysium`, `Elysia`, and `Coding agent`, and
    // redacting those would hide which part of Elysium failed.
    let people: Vec<(String, Option<String>)> = users::table
        .filter(users::kratos_identity_id.is_not_null())
        .select((users::name, users::email))
        .load(connection)
        .await?;
    for (name, email) in people {
        values.push(name);
        values.extend(email);
    }

    let logins: Vec<String> = github_credentials::table
        .select(github_credentials::login)
        .load(connection)
        .await?;
    values.extend(logins);

    let atlassian: Vec<(String, String, String, String)> = jira_credentials::table
        .select((
            jira_credentials::site_url,
            jira_credentials::account_email,
            jira_credentials::account_id,
            jira_credentials::display_name,
        ))
        .load(connection)
        .await?;
    for (site_url, email, account_id, display_name) in atlassian {
        values.extend([site_url, email, account_id, display_name]);
    }

    let tokens: Vec<(String, Option<String>)> = token_accounts::table
        .select((token_accounts::identity, token_accounts::site_url))
        .load(connection)
        .await?;
    for (identity, site_url) in tokens {
        values.push(identity);
        values.extend(site_url);
    }

    let mailboxes: Vec<(String, String)> = mail_accounts::table
        .select((mail_accounts::address, mail_accounts::display_name))
        .load(connection)
        .await?;
    for (address, display_name) in mailboxes {
        values.extend([address, display_name]);
    }

    Ok(values)
}
