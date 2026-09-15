// Copyright © 2026 Jalapeno Labs

//! `/api/v1/mail`: connected mailboxes. Credentials are write-only and never leave
//! the API; OAuth accounts never even pass one through a request body.

mod create_mailbox;
mod delete_account;
mod finish_oauth;
mod get_capabilities;
mod list_accounts;
mod send_test_message;
mod set_up_server;
mod start_oauth;
mod test_account;
mod update_account;

use anyhow::Context;
use axum::Router;
use axum::routing::{get, patch, post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::errors::ApiError;
use crate::mail::stalwart::{Administrator, StalwartError};
use crate::mail::transport::{Auth, Mailbox};
use crate::models::mail_account::{self, MailAccount, MailAccountKind};
use crate::models::mail_server;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/capabilities", get(get_capabilities::handle))
        .route("/server", post(set_up_server::handle))
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
        }
    }
}

/// Whether the bundled mail server can host mailboxes yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MailServerStatus {
    /// The server waits for its setup: none has run, or the server was reset since and
    /// no longer knows the stored administrator.
    SetupRequired,
    Ready,
    /// The server did not answer.
    Unreachable,
}

/// The bundled mail server as clients see it. The administrator is never included.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MailServerResponse {
    status: MailServerStatus,
    /// The domain chosen at setup; `None` before setup.
    domain: Option<String>,
    /// Why the server is unreachable, or why a set-up server needs setup again.
    error: Option<String>,
}

/// Asks the bundled server, live, whether it accepts the stored administrator.
///
/// # Errors
/// [`ApiError::Internal`] when the database fails or the stored secret does not decrypt.
async fn mail_server_status(state: &AppState) -> Result<MailServerResponse, ApiError> {
    let Some((domain, administrator)) = stored_administrator(state).await? else {
        return Ok(MailServerResponse {
            status: MailServerStatus::SetupRequired,
            domain: None,
            error: None,
        });
    };

    let (status, error) = match state.mail.stalwart.verify(&administrator).await {
        Ok(()) => (MailServerStatus::Ready, None),
        // A reset server starts over in bootstrap mode, which knows no administrator.
        Err(StalwartError::Unauthorized) => (
            MailServerStatus::SetupRequired,
            Some(
                "the mail server no longer accepts Elysium's administrator, so it was probably reset"
                    .to_owned(),
            ),
        ),
        Err(error) => (MailServerStatus::Unreachable, Some(error.to_string())),
    };
    Ok(MailServerResponse {
        status,
        domain: Some(domain),
        error,
    })
}

/// The stored administrator and the domain it was set up with, or `None` before setup.
///
/// # Errors
/// [`ApiError::Internal`] when the database fails or the stored secret does not decrypt.
async fn stored_administrator(
    state: &AppState,
) -> Result<Option<(String, Administrator)>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Some(server) = mail_server::find(&mut connection).await? else {
        return Ok(None);
    };

    let password = server
        .admin_secret(&state.cipher)
        .context("stored mail server administrator does not decrypt")?;
    let administrator = Administrator {
        username: server.admin_username,
        password,
    };
    Ok(Some((server.domain, administrator)))
}

/// The administrator for a request that changes mailboxes on the bundled server.
///
/// # Errors
/// [`ApiError::Unavailable`] with `not_set_up` before the server is set up, and [`ApiError::Internal`]
/// when the database fails or the stored secret does not decrypt.
async fn require_administrator(
    state: &AppState,
    not_set_up: &'static str,
) -> Result<Administrator, ApiError> {
    stored_administrator(state)
        .await?
        .map(|(_domain, administrator)| administrator)
        .ok_or(ApiError::Unavailable(not_set_up))
}

/// Dot-separated labels of letters, digits, and hyphens, at least two of them.
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
    let (imap, smtp) = state.mail.endpoints(account.kind);
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
