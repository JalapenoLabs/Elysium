// Copyright © 2026 Jalapeno Labs

//! `/api/v1/mail`: connected mailboxes. Credentials are write-only and never leave
//! the API; OAuth accounts never even pass one through a request body.

mod create_mailbox;
mod delete_account;
mod finish_oauth;
mod get_capabilities;
mod list_accounts;
mod send_test_message;
mod start_oauth;
mod test_account;
mod update_account;

use anyhow::Context;
use axum::Router;
use axum::routing::{get, patch, post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::mail::transport::{Auth, Mailbox};
use crate::models::mail_account::{self, MailAccount, MailAccountKind};
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/capabilities", get(get_capabilities::handle))
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

/// Resolves an account into a mailbox with a credential that works right now.
///
/// OAuth accounts get a fresh access token from the broker. When the provider rotates
/// the refresh token in the same exchange the new one is sealed immediately: the old
/// one may already be dead.
///
/// # Errors
/// [`ApiError::Unavailable`] when the service the account needs is not configured,
/// [`ApiError::BadGateway`] when the broker refuses, and [`ApiError::Internal`] when
/// the stored credential does not decrypt.
async fn open_mailbox(state: &AppState, account: &MailAccount) -> Result<Mailbox, ApiError> {
    let Some((imap, smtp)) = state.mail.endpoints(account.kind) else {
        return Err(ApiError::Unavailable(
            "self-hosted mail is not configured on this deployment",
        ));
    };
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
