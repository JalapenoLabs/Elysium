// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors/options`: what each of a kind's selection fields can pick from,
//! through an account the person has or a new one, `{ kind, account }`. Answers
//! `{ fields: { <field>: { options: [{ id, label, detail }], truncated } } }`, read from the
//! provider now. Nothing is stored.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};

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
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let fields = kinds::options(&state, &current, body.kind, body.account).await?;
    Ok(Json(json!({ "fields": fields })))
}
