// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors`: read a service as the signed-in person.
//!
//! How the account signs in follows its kind (`crate::connectors::SignIn`):
//!
//! - **Through a browser** (Outlook, Gmail): `browserSessionId` names one of their browsers,
//!   which has at most one connector. The first read records what the mailbox already holds
//!   without raising anything; see `crate::connectors::ingest`.
//! - **With a credential** (Fireflies): `apiKey` is the person's own key, checked with the
//!   service before anything is stored and then sealed, and `storageLocationId` is where
//!   the account's recordings are kept.
//!
//! A field the kind does not take is refused rather than ignored, so a client never believes
//! it set something that was dropped.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use secrecy::SecretString;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use uuid::Uuid;

use super::ConnectorAccountResponse;
use crate::auth::CurrentUser;
use crate::connectors::{ConnectorKind, SignIn};
use crate::errors::ApiError;
use crate::meetings::source::SourceError;
use crate::models::connector_account::{self, NewConnectorAccount};
use crate::models::storage_location;
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// The longest API key accepted. Fireflies' are well under a hundred characters.
const API_KEY_MAX_BYTES: usize = 512;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    kind: ConnectorKind,
    browser_session_id: Option<Uuid>,
    api_key: Option<ApiKey>,
    storage_location_id: Option<Uuid>,
}

/// An API key from a request body, checked as it is parsed so a validation report never
/// carries it. Surrounding whitespace, which copying often brings, is dropped.
#[derive(Debug)]
pub struct ApiKey(SecretString);

impl<'de> Deserialize<'de> for ApiKey {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let key = String::deserialize(deserializer)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(serde::de::Error::custom("apiKey must not be blank"));
        }
        if key.len() > API_KEY_MAX_BYTES {
            return Err(serde::de::Error::custom("apiKey exceeds 512 characters"));
        }
        Ok(Self(SecretString::from(key)))
    }
}

/// How the body says the account signs in, once it has been checked against its kind.
#[derive(Debug)]
enum Plan {
    Browser(Uuid),
    Credential {
        api_key: SecretString,
        storage_location_id: Uuid,
    },
}

/// The body's sign-in, or what is wrong with it for its kind.
fn plan(body: RequestBody) -> Result<Plan, ApiError> {
    match body.kind.sign_in() {
        SignIn::Browser => {
            if body.api_key.is_some() || body.storage_location_id.is_some() {
                return Err(ApiError::BadRequest(format!(
                    "{} reads through a browser and takes no apiKey or storageLocationId",
                    body.kind.as_str()
                )));
            }
            let browser_session_id = body.browser_session_id.ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "{} reads through a browser: browserSessionId is required",
                    body.kind.as_str()
                ))
            })?;
            Ok(Plan::Browser(browser_session_id))
        }
        SignIn::Credential => {
            if body.browser_session_id.is_some() {
                return Err(ApiError::BadRequest(format!(
                    "{} signs in with an API key and takes no browserSessionId",
                    body.kind.as_str()
                )));
            }
            match (body.api_key, body.storage_location_id) {
                (Some(ApiKey(api_key)), Some(storage_location_id)) => Ok(Plan::Credential {
                    api_key,
                    storage_location_id,
                }),
                _ => Err(ApiError::BadRequest(format!(
                    "{} needs an apiKey, and a storageLocationId to keep its recordings in",
                    body.kind.as_str()
                ))),
            }
        }
    }
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    let kind = body.kind;
    let plan = plan(body)?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let new_account = match plan {
        Plan::Browser(browser_session_id) => {
            NewConnectorAccount::through_browser(kind.as_str(), browser_session_id, current.id())
        }
        Plan::Credential {
            api_key,
            storage_location_id,
        } => {
            match storage_location::find(&mut connection, storage_location_id).await {
                Ok(_location) => {}
                Err(DieselError::NotFound) => {
                    return Err(ApiError::BadRequest(
                        "storageLocationId names no storage location".to_owned(),
                    ));
                }
                Err(error) => return Err(error.into()),
            }
            // Only Fireflies signs in with a key today; a key it refuses is never stored.
            match state.meetings.fireflies().whoami(&api_key).await {
                Ok(_user) => {}
                Err(SourceError::Unauthorized(message)) => {
                    return Err(ApiError::BadRequest(message));
                }
                Err(error) => return Err(ApiError::BadGateway(error.to_string())),
            }
            NewConnectorAccount::with_credential(
                &state.cipher,
                kind.as_str(),
                &api_key,
                Some(storage_location_id),
                current.id(),
            )
        }
    };

    let account = match connector_account::create(&mut connection, new_account).await {
        Ok(account) => account,
        Err(DieselError::NotFound) => {
            return Err(ApiError::BadRequest(
                "browserSessionId names no browser of yours".to_owned(),
            ));
        }
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            return Err(ApiError::Conflict("that browser already has a connector"));
        }
        Err(error) => return Err(error.into()),
    };

    let response = ConnectorAccountResponse::new(account);
    state
        .events
        .publish(&ServerEvent::ConnectorAccountUpserted(response.clone()));
    Ok((StatusCode::CREATED, Json(json!({ "connector": response }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(value: Value) -> RequestBody {
        serde_json::from_value(value).expect("parses")
    }

    #[test]
    fn a_body_names_a_known_kind_and_a_browser() {
        let parsed = body(json!({
            "kind": "outlook",
            "browserSessionId": Uuid::nil(),
        }));
        assert_eq!(parsed.kind, ConnectorKind::Outlook);
        assert!(matches!(plan(parsed), Ok(Plan::Browser(id)) if id == Uuid::nil()));

        let unknown = serde_json::from_value::<RequestBody>(json!({
            "kind": "carrier-pigeon",
            "browserSessionId": Uuid::nil(),
        }));
        assert!(
            unknown
                .expect_err("an unknown kind is refused")
                .to_string()
                .contains("unknown variant")
        );
    }

    #[test]
    fn fireflies_takes_a_key_and_a_storage_location() {
        let parsed = body(json!({
            "kind": "fireflies",
            "apiKey": "  key-from-fireflies  ",
            "storageLocationId": Uuid::nil(),
        }));
        assert!(matches!(
            plan(parsed),
            Ok(Plan::Credential { storage_location_id, .. }) if storage_location_id == Uuid::nil()
        ));
    }

    #[test]
    fn a_field_the_kind_does_not_take_is_refused() {
        let browser_for_fireflies = body(json!({
            "kind": "fireflies",
            "browserSessionId": Uuid::nil(),
            "apiKey": "key",
            "storageLocationId": Uuid::nil(),
        }));
        assert!(matches!(
            plan(browser_for_fireflies),
            Err(ApiError::BadRequest(_))
        ));

        let key_for_outlook = body(json!({
            "kind": "outlook",
            "browserSessionId": Uuid::nil(),
            "apiKey": "key",
        }));
        assert!(matches!(
            plan(key_for_outlook),
            Err(ApiError::BadRequest(_))
        ));

        let fireflies_without_storage = body(json!({ "kind": "fireflies", "apiKey": "key" }));
        assert!(matches!(
            plan(fireflies_without_storage),
            Err(ApiError::BadRequest(message)) if message.contains("storageLocationId")
        ));
    }

    #[test]
    fn a_blank_key_never_parses() {
        let blank = serde_json::from_value::<RequestBody>(json!({
            "kind": "fireflies",
            "apiKey": "   ",
            "storageLocationId": Uuid::nil(),
        }));
        assert!(
            blank
                .expect_err("a blank key is refused")
                .to_string()
                .contains("must not be blank")
        );
    }
}
