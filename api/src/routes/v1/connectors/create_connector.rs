// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors`: add a connector of a kind with the new account it signs in with
//! (`{ "new": { ...its fields } }`) and its content. Every connector owns its account; none is
//! ever shared, so naming an existing one is refused.
//!
//! What each kind accepts is its own (`crate::connectors::kinds`): Jira and GitHub's accounts
//! are made and checked the way their settings pages make one, Confluence's as Jira's is, and a
//! key's with its provider, all before anything is stored. A kind read through a browser takes
//! no fields: its browser starts with it, `starting`, and its person signs in on its stream.

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
