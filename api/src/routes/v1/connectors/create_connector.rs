// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors`: add a connector of a kind, reading through an account the person
//! has (`{ "id": ... }`) or a new one (`{ "new": { ...its fields } }`), with its content.
//!
//! What each kind accepts is its own (`crate::connectors::kinds`): Jira and GitHub come with a
//! new account, made and checked the way their settings pages make one; a mailbox reads
//! through a browser the person already signed in to; Fireflies takes a key, new or saved,
//! and a storage location. A new account's secret is checked with its provider before
//! anything is stored.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::view;
use crate::auth::CurrentUser;
use crate::connectors::ConnectorKind;
use crate::connectors::kinds::{self, AccountChoice};
use crate::errors::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    kind: ConnectorKind,
    account: AccountChoice,
    #[serde(default = "empty_content")]
    content: Value,
}

fn empty_content() -> Value {
    Value::Object(Map::new())
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    let created = kinds::create(&state, &current, body.kind, body.account, body.content).await?;
    state.connectors.activity.changed(created.id);
    Ok((
        StatusCode::CREATED,
        Json(json!({ "connector": view(&state, created).await? })),
    ))
}
