// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/connectors/kinds`: every connector kind and account kind, defined once.
//!
//! Structure only: whose a kind is, the account it reads through, what its form asks for, and
//! its checks in order. The web app names every id from its translations.

use axum::Json;
use serde_json::{Value, json};

use crate::auth::CurrentUser;
use crate::connectors::kinds::{ACCOUNT_DEFINITIONS, DEFINITIONS};

pub async fn handle(_current: CurrentUser) -> Json<Value> {
    Json(json!({ "kinds": DEFINITIONS, "accounts": ACCOUNT_DEFINITIONS }))
}
