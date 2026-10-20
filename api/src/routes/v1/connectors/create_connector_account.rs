// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors`: read a service through one of the signed-in person's browsers.
//!
//! The browser must be theirs, and has at most one connector. The first read records what the
//! mailbox already holds without raising anything; see `crate::connectors::ingest`.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::ConnectorAccountResponse;
use crate::auth::CurrentUser;
use crate::connectors::ConnectorKind;
use crate::errors::ApiError;
use crate::models::connector_account::{self, NewConnectorAccount};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    kind: ConnectorKind,
    browser_session_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let new_account = NewConnectorAccount {
        id: Uuid::now_v7(),
        kind: body.kind.as_str().to_owned(),
        browser_session_id: body.browser_session_id,
        created_by: current.id(),
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

    #[test]
    fn a_body_names_a_known_kind_and_a_browser() {
        let body: RequestBody = serde_json::from_value(json!({
            "kind": "outlook",
            "browserSessionId": Uuid::nil(),
        }))
        .expect("parses");
        assert_eq!(body.kind, ConnectorKind::Outlook);

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
}
