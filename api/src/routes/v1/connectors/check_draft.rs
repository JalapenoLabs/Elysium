// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors/checks`: run a kind's checks against a connector not stored yet,
//! `{ kind, account, content }` as `POST /connectors` takes them, so the hub shows whether it
//! will work before it is saved. Nothing is stored; a new account's secret reaches its provider
//! and no further. Answers `202` with the run; progress arrives as `connectorCheckRun.updated`.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::auth::CurrentUser;
use crate::connectors::ConnectorKind;
use crate::connectors::kinds::{self, AccountChoice, Source};
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
    let source = Source::Draft {
        account: body.account,
        content: body.content,
    };
    let checks = kinds::prepare_checks(&state, &current, body.kind, source).await?;
    let run = kinds::start_checks(&state, &current, body.kind, None, checks);
    Ok((StatusCode::ACCEPTED, Json(json!({ "run": run }))))
}
