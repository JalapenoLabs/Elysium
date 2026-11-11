// Copyright © 2026 Jalapeno Labs

//! What every kind read through a browser shares: the browser is the connector's account. See
//! `docs/browser.md` and `docs/connectors.md`.
//!
//! A new connector starts its own browser at its kind's start page ([`create`]), and its person
//! signs in on the stream its page shows. Removing the connector removes the browser and the
//! profile signed in on it ([`remove`]). A connector can be without one, waiting: one that
//! shared a browser with another kind before each connector owned its own; [`sign_in`] starts
//! it one.

use serde_json::Value;
use url::Url;
use uuid::Uuid;

use super::definition;
use crate::auth::CurrentUser;
use crate::browser::{CreateError, DeleteError};
use crate::connectors::ConnectorKind;
use crate::errors::ApiError;
use crate::models::browser_session::{self, BrowserSession};
use crate::models::connector::{self, AccountRef, Connector, NewConnector};
use crate::state::AppState;

/// The browser `id`, when `owner` created it.
///
/// # Errors
/// [`ApiError::NotFound`] when it does not exist or is someone else's.
pub(super) async fn owned_browser(
    state: &AppState,
    id: Uuid,
    owner: Uuid,
) -> Result<BrowserSession, ApiError> {
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let browser = browser_session::find(&mut connection, id)
        .await?
        .filter(|browser| browser.created_by == owner)
        .ok_or(ApiError::NotFound)?;
    Ok(browser)
}

/// The browser a stored connector reads through.
///
/// # Errors
/// [`ApiError::CodedConflict`] while it waits for one, and as [`owned_browser`].
pub(super) async fn browser_of(
    state: &AppState,
    connector: &Connector,
) -> Result<BrowserSession, ApiError> {
    let id = connector
        .browser_session_id
        .ok_or(ApiError::CodedConflict {
            code: "browser_not_started",
            message: "this connector has no browser yet; start one from its page and sign in",
        })?;
    owned_browser(state, id, connector.created_by).await
}

/// What a request for a draft of a browser kind answers: there is nothing to check or list
/// until the connector exists, since its browser starts with it.
pub(super) fn starts_with_its_browser() -> ApiError {
    ApiError::BadRequest(
        "this is read through a browser, which starts when the connector is added; sign in on \
         its stream there"
            .to_owned(),
    )
}

/// Starts a browser for `owner` at `kind`'s start page.
///
/// # Errors
/// [`ApiError::CodedConflict`] when every port the host streams on is taken.
async fn start(
    state: &AppState,
    owner: Uuid,
    kind: ConnectorKind,
) -> Result<BrowserSession, ApiError> {
    let start_url = definition(kind)
        .start_url
        .and_then(|start_url| Url::parse(start_url).ok())
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "{} reads through a browser but names no start page",
                kind.as_str()
            ))
        })?;
    // The browser is named for what it is for; its person only ever meets it as its connector.
    state
        .browsers
        .create(owner, kind.as_str().to_owned(), start_url)
        .await
        .map_err(|error| match error {
            CreateError::PortsExhausted => ApiError::CodedConflict {
                code: "browser_ports_exhausted",
                message: "every browser this host can stream is in use; remove a connector read \
                          through a browser first",
            },
            CreateError::Internal(error) => ApiError::Internal(error),
        })
}

/// Removes a browser that never came to belong to a connector, logging rather than failing:
/// the request has already failed for its own reason, which is what its caller is told.
async fn discard(state: &AppState, browser: &BrowserSession) {
    if let Err(error) = state.browsers.delete(browser).await {
        tracing::event!(
            name: "connectors.browser.discard_failed",
            tracing::Level::WARN,
            browser.id = %browser.id,
            error.message = %error,
            "a browser started for a connector that was not saved could not be removed",
        );
    }
}

/// Creates a connector of `kind` for `current`, reading `content`, with a browser of its own,
/// keeping what it saves in `storage_location_id` for a kind that saves files.
///
/// # Errors
/// As [`start`], and any database error, after which the browser is removed again.
pub(super) async fn create(
    state: &AppState,
    current: &CurrentUser,
    kind: ConnectorKind,
    content: Value,
    storage_location_id: Option<Uuid>,
) -> Result<Connector, ApiError> {
    let browser = start(state, current.id(), kind).await?;
    let mut new_connector =
        NewConnector::new(kind.as_str(), AccountRef::Browser(browser.id), current.id());
    new_connector.content = content;
    new_connector.storage_location_id = storage_location_id;
    let stored = async {
        let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
        Ok::<_, ApiError>(connector::create(&mut connection, new_connector).await?)
    };
    match stored.await {
        Ok(created) => Ok(created),
        Err(error) => {
            discard(state, &browser).await;
            Err(error)
        }
    }
}

/// Starts a browser for `stored`, a connector of `kind` waiting for one.
///
/// # Errors
/// [`ApiError::Conflict`] when it already has one, and as [`create`].
pub(super) async fn sign_in(
    state: &AppState,
    kind: ConnectorKind,
    stored: &Connector,
) -> Result<Connector, ApiError> {
    if stored.browser_session_id.is_some() {
        return Err(ApiError::Conflict("this connector already has its browser"));
    }
    let browser = start(state, stored.created_by, kind).await?;
    let attached = async {
        let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
        connector::attach_browser(&mut connection, stored.id, browser.id)
            .await
            .map_err(|error| match error {
                // Another request started one first.
                diesel::result::Error::NotFound => {
                    ApiError::Conflict("this connector already has its browser")
                }
                other => other.into(),
            })
    };
    match attached.await {
        Ok(updated) => {
            state.connectors.activity.changed(updated.id);
            Ok(updated)
        }
        Err(error) => {
            discard(state, &browser).await;
            Err(error)
        }
    }
}

/// Removes `stored` with its browser: the container, its network, and the profile its person
/// signed in on. The connector's row goes with the browser's, by its foreign key.
///
/// # Errors
/// [`ApiError::BadGateway`] when Docker refuses, which leaves both to be removed again.
pub(super) async fn remove(state: &AppState, stored: &Connector) -> Result<(), ApiError> {
    let Some(browser_id) = stored.browser_session_id else {
        let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
        connector::delete(&mut connection, stored.id).await?;
        return Ok(());
    };
    let browser = owned_browser(state, browser_id, stored.created_by).await?;
    state
        .browsers
        .delete(&browser)
        .await
        .map_err(|error| match error {
            DeleteError::Docker(error) => ApiError::BadGateway(error.to_string()),
            DeleteError::Internal(error) => ApiError::Internal(error),
        })
}
