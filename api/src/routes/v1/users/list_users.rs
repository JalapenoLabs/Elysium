// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/users`: every user, machines included. Admins also see how each person signs
//! in, read from Kratos.

use std::collections::HashMap;

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use futures_util::{StreamExt as _, stream};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::UserResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::user;
use crate::state::AppState;

/// How many identities are asked of Kratos at once. Each is a single-row read; this keeps a
/// large workspace from opening a connection per person.
const KRATOS_LOOKUPS_AT_ONCE: usize = 8;

pub async fn handle(
    State(state): State<AppState>,
    CurrentUser(caller): CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let users = user::list(&mut connection).await?;
    drop(connection);

    if !caller.is_admin() {
        let users: Vec<UserResponse> = users.into_iter().map(UserResponse::from).collect();
        return Ok(Json(json!({ "users": users })));
    }

    // Asked per identity: only a single identity's answer names its credential kinds
    // without their secrets. The list still loads when Kratos cannot
    // answer; it just says nothing about sign-in.
    let identities: Vec<Uuid> = users
        .iter()
        .filter_map(|user| user.kratos_identity_id)
        .collect();
    let mut answers = stream::iter(identities)
        .map(|identity| state.auth.kratos.identity(identity))
        .buffer_unordered(KRATOS_LOOKUPS_AT_ONCE);
    let mut methods: HashMap<Uuid, Vec<String>> = HashMap::new();
    while let Some(answer) = answers.next().await {
        match answer {
            Ok(identity) => {
                methods.insert(identity.id, identity.methods());
            }
            Err(error) => event!(
                name: "auth.identity.unavailable",
                Level::WARN,
                error.message = %error,
                "listing a user without sign-in methods; Kratos did not answer",
            ),
        }
    }

    let users: Vec<UserResponse> = users
        .into_iter()
        .map(|user| {
            let sign_in = user
                .kratos_identity_id
                .and_then(|identity| methods.get(&identity).cloned());
            UserResponse::new(user, sign_in)
        })
        .collect();
    Ok(Json(json!({ "users": users })))
}
