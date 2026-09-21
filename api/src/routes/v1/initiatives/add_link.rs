// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/initiatives/{id}/links`: link an initiative to a container a picker listed:
//! a Jira epic or saved filter, or a GitHub milestone or label.
//!
//! The container is read through its credential first. Its children join the initiative on
//! the watcher's next pass, which this wakes at once: each becomes an item, `open` and owned
//! as the provider reports, unless an item already links to it. Linking the same container
//! again answers it unchanged.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::InitiativeLinkResponse;
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_link::{LinkCredential, LinkProvider};
use crate::models::initiative_link::{self, ContainerKind, NewContainer};
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::{publish_history, validate_not_blank};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    provider: LinkProvider,
    credential_id: Uuid,
    kind: ContainerKind,
    /// An epic's key, a saved filter's id, `owner/name#3` for a milestone, or
    /// `owner/name:label`.
    #[validate(length(min = 1, max = 300), custom(function = "validate_not_blank"))]
    reference: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let container = state
        .links
        .provider(body.provider)
        .find_container(body.credential_id, body.kind, &body.reference)
        .await?;
    let new_container = NewContainer {
        credential: LinkCredential {
            provider: body.provider,
            id: body.credential_id,
        },
        kind: body.kind,
        external_id: container.external_id,
        external_key: container.key,
        url: container.url,
        title: container.title,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (link, history) =
        initiative_link::add(&mut connection, id, new_container, Actor::User, Utc::now()).await?;
    drop(connection);

    let link = InitiativeLinkResponse::from(link);
    publish_history(&state.events, history);
    state
        .events
        .publish(&ServerEvent::InitiativeLinkUpserted(link.clone()));
    state.links.wake_watcher();

    Ok((StatusCode::CREATED, Json(json!({ "link": link }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_container_is_a_provider_a_credential_a_kind_and_a_reference() {
        let body: RequestBody = serde_json::from_value(json!({
            "provider": "github",
            "credentialId": Uuid::nil(),
            "kind": "milestone",
            "reference": "JalapenoLabs/Elysium#3",
        }))
        .expect("parses");
        body.validate().expect("valid");

        serde_json::from_value::<RequestBody>(json!({
            "provider": "jira",
            "credentialId": Uuid::nil(),
            "kind": "board",
            "reference": "12",
        }))
        .expect_err("a board is not a container");
    }
}
