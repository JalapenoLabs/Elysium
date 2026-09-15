// Copyright © 2026 Jalapeno Labs

//! `/api/v1/mail`: connected mailboxes, and the mail server and domains self-hosted
//! mailboxes live on. Credentials are write-only and never leave the API; OAuth accounts
//! never even pass one through a request body.

mod check_domain_dns;
mod create_domain;
mod create_mailbox;
mod create_server;
mod delete_account;
mod delete_domain;
mod finish_oauth;
mod get_capabilities;
mod get_server;
mod list_accounts;
mod list_domains;
mod send_test_message;
mod start_oauth;
mod test_account;
mod update_account;

use anyhow::Context;
use axum::Router;
use axum::routing::{delete, get, patch, post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::errors::ApiError;
use crate::mail;
use crate::mail::stalwart::Administrator;
use crate::mail::transport::{Auth, Mailbox};
use crate::models::mail_account::{self, MailAccount, MailAccountKind};
use crate::models::mail_domain::MailDomain;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/capabilities", get(get_capabilities::handle))
        .route(
            "/server",
            get(get_server::handle).post(create_server::handle),
        )
        .route(
            "/domains",
            get(list_domains::handle).post(create_domain::handle),
        )
        .route("/domains/{id}", delete(delete_domain::handle))
        .route("/domains/{id}/dns", get(check_domain_dns::handle))
        .route(
            "/accounts",
            get(list_accounts::handle).post(create_mailbox::handle),
        )
        .route(
            "/accounts/{id}",
            patch(update_account::handle).delete(delete_account::handle),
        )
        .route("/accounts/{id}/test", post(test_account::handle))
        .route(
            "/accounts/{id}/test-message",
            post(send_test_message::handle),
        )
        .route("/oauth/{kind}/start", get(start_oauth::handle))
        .route("/oauth/callback", get(finish_oauth::handle))
}

/// Browser cookie tying an OAuth callback to the browser that started the flow, so a
/// callback link crafted by someone else cannot connect their account here.
const OAUTH_FLOW_COOKIE: &str = "elysium_mail_oauth";

/// Where a pending OAuth flow waits in Redis, keyed by its `state`.
const OAUTH_FLOW_KEY_PREFIX: &str = "mail:oauth-flow:";

/// How long a started flow stays redeemable. Matches the broker's consent window,
/// which already covers signing in with a second factor.
const OAUTH_FLOW_TTL_SECONDS: u64 = 15 * 60;

/// Where the broker returns the browser, relative to the origin the browser used.
const OAUTH_CALLBACK_PATH: &str = "/api/v1/mail/oauth/callback";

/// Where the browser lands when a flow ends, successfully or not.
const MAIL_SETTINGS_PATH: &str = "/settings/email";

/// What the API remembers between starting an OAuth flow and its callback.
#[derive(Serialize, Deserialize)]
struct PendingOAuthFlow {
    kind: MailAccountKind,
    code_verifier: String,
}

/// A mailbox as clients see it. The credential is never included, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAccountResponse {
    id: Uuid,
    kind: MailAccountKind,
    address: String,
    display_name: String,
    is_active: bool,
    last_checked_at: Option<DateTime<Utc>>,
    /// Why the latest check failed; `None` when it succeeded or none has run.
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    /// The mail domain of a self-hosted mailbox; `None` for OAuth accounts.
    mail_domain_id: Option<Uuid>,
}

impl From<MailAccount> for MailAccountResponse {
    fn from(account: MailAccount) -> Self {
        Self {
            id: account.id,
            kind: account.kind,
            address: account.address,
            display_name: account.display_name,
            is_active: account.is_active,
            last_checked_at: account.last_checked_at,
            last_error: account.last_error,
            created_at: account.created_at,
            updated_at: account.updated_at,
            mail_domain_id: account.mail_domain_id,
        }
    }
}

/// A mail domain as clients see it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailDomainResponse {
    id: Uuid,
    name: String,
    /// The domain the server was created with, which cannot be removed.
    is_default: bool,
    created_at: DateTime<Utc>,
}

impl From<MailDomain> for MailDomainResponse {
    fn from(domain: MailDomain) -> Self {
        Self {
            id: domain.id,
            name: domain.name,
            is_default: domain.is_default,
            created_at: domain.created_at,
        }
    }
}

/// The mail server's administrator, for a request that changes the server.
///
/// # Errors
/// [`ApiError::Unavailable`] with `no_server` when no mail server exists, and
/// [`ApiError::Internal`] when the database fails or the stored secret does not decrypt.
async fn require_administrator(
    state: &AppState,
    no_server: &'static str,
) -> Result<Administrator, ApiError> {
    state
        .mail
        .hosting
        .administrator()
        .await?
        .ok_or(ApiError::Unavailable(no_server))
}

/// Dot-separated labels of letters, digits, and hyphens, at least two of them. Used for
/// both mail domains and the server's hostname.
fn validate_domain(value: &str) -> Result<(), ValidationError> {
    let labels: Vec<&str> = value.split('.').collect();
    let well_formed = labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        });
    if !well_formed {
        return Err(ValidationError::new("domain")
            .with_message("must be a domain such as example.com".into()));
    }
    Ok(())
}

/// Resolves an account into a mailbox with a credential that works right now.
///
/// OAuth accounts get a fresh access token from the broker. When the provider rotates
/// the refresh token in the same exchange the new one is sealed immediately: the old
/// one may already be dead.
///
/// # Errors
/// [`ApiError::Unavailable`] when no broker is configured for an OAuth account,
/// [`ApiError::BadGateway`] when the broker refuses, and [`ApiError::Internal`] when
/// the stored credential does not decrypt.
async fn open_mailbox(state: &AppState, account: &MailAccount) -> Result<Mailbox, ApiError> {
    let (imap, smtp) = mail::endpoints(account.kind);
    let credential = account
        .credential(&state.cipher)
        .context("stored mail credential does not decrypt")?;

    let auth = if account.kind == MailAccountKind::SelfHosted {
        Auth::Password(credential)
    } else {
        let broker = state.mail.broker.as_ref().ok_or(ApiError::Unavailable(
            "no OAuth broker is configured on this deployment",
        ))?;
        let grant = broker.refresh(account.kind, &credential).await?;

        if let Some(rotated) = grant.refresh_token {
            let mut connection = state
                .database
                .get()
                .await
                .context("no database connection available")?;
            let updated = mail_account::replace_credential(
                &mut connection,
                &state.cipher,
                account.id,
                &rotated,
            )
            .await?;
            state
                .events
                .publish(&ServerEvent::MailAccountUpserted(updated.into()));
        }
        Auth::AccessToken(grant.access_token)
    };

    Ok(Mailbox {
        address: account.address.clone(),
        display_name: account.display_name.clone(),
        imap,
        smtp,
        auth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_names_need_two_well_formed_labels() {
        for valid in [
            "example.com",
            "mail.example.com",
            "elysium.local",
            "a-b.test",
        ] {
            assert!(validate_domain(valid).is_ok(), "{valid} should be accepted");
        }
        for invalid in [
            "localhost",
            "-bad.example",
            "exa mple.com",
            "example..com",
            "bad-.com",
        ] {
            assert!(
                validate_domain(invalid).is_err(),
                "{invalid} should be refused"
            );
        }
    }
}
