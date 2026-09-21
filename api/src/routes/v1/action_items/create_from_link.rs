// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/from-link`: add an item for a Jira issue, a GitHub issue, or a
//! GitHub pull request that a picker listed.
//!
//! The thing is read through its credential, never typed: the item starts from its title,
//! priority, and due date, is owned as its assignee says, and links to it as its primary.
//! After that the priority and due date are the item's own and are never synced. The user
//! created it, so it starts `open`. A thing already linked to an item answers `409`.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{
    LinkTarget, link_responses, publish_item_links, publish_item_write, refuse_unknown_projects,
    unique_ids, validate_not_blank,
};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item::{ActionItemState, NewActionItem};
use crate::models::action_item_link::{self, LinkKind, LinkProvider, NewLink};
use crate::state::AppState;

/// The longest title an item holds, matching the items table.
const TITLE_MAX_CHARACTERS: usize = 500;

/// A [`LinkTarget`] and the memberships the new item starts with. The fields are spelled
/// out rather than flattened, since serde cannot refuse unknown fields through a flatten.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    provider: LinkProvider,
    credential_id: Uuid,
    kind: LinkKind,
    #[validate(length(min = 1, max = 300), custom(function = "validate_not_blank"))]
    reference: String,
    #[serde(default)]
    project_ids: Vec<Uuid>,
    #[serde(default)]
    initiative_ids: Vec<Uuid>,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;
    let target = LinkTarget {
        provider: body.provider,
        credential_id: body.credential_id,
        kind: body.kind,
        reference: body.reference,
    };

    let remote = state
        .links
        .provider(target.provider)
        .find(target.credential_id, target.kind, &target.reference)
        .await?;
    let title = if remote.title.trim().is_empty() {
        remote.key.clone()
    } else {
        remote
            .title
            .trim()
            .chars()
            .take(TITLE_MAX_CHARACTERS)
            .collect()
    };
    let new_item = NewActionItem {
        title,
        notes: String::new(),
        state: ActionItemState::Open,
        priority: remote.starting_priority(),
        due_at: remote.starting_due_at(),
        owner: remote.owner.clone(),
        project_ids: unique_ids(body.project_ids),
        initiative_ids: unique_ids(body.initiative_ids),
    };
    let new_link = NewLink {
        credential: target.credential(),
        kind: target.kind,
        external_id: remote.external_id.clone(),
        observation: remote.observation(),
    };

    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let linked =
        action_item_link::create_linked_item(&mut connection, new_item, new_link, Actor::User, now)
            .await
            .map_err(refuse_unknown_projects)?;
    let link = linked
        .links
        .into_iter()
        .next()
        .context("an item created from a link has that link")?;
    let item = publish_item_write(&state.events, &mut connection, linked.item, &[], now).await?;
    publish_item_links(&state.events, &mut connection, link.action_item_id).await?;
    let link = link_responses(&mut connection, vec![link])
        .await?
        .pop()
        .context("one link in, one response out")?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "item": item, "link": link })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_from_a_link_names_the_thing_and_may_name_its_memberships() {
        let body: RequestBody = serde_json::from_value(json!({
            "provider": "jira",
            "credentialId": Uuid::nil(),
            "kind": "issue",
            "reference": "ELY-12",
            "projectIds": [Uuid::nil()],
        }))
        .expect("parses");
        body.validate().expect("valid");
        assert_eq!(body.reference, "ELY-12");
        assert_eq!(body.project_ids, [Uuid::nil()]);

        let blank: RequestBody = serde_json::from_value(json!({
            "provider": "github",
            "credentialId": Uuid::nil(),
            "kind": "pull-request",
            "reference": "  ",
        }))
        .expect("parses");
        blank
            .validate()
            .expect_err("a blank reference names nothing");

        serde_json::from_value::<RequestBody>(json!({
            "provider": "gitlab",
            "credentialId": Uuid::nil(),
            "kind": "issue",
            "reference": "a/b#1",
        }))
        .expect_err("an unknown provider");
        serde_json::from_value::<RequestBody>(json!({
            "provider": "jira",
            "credentialId": Uuid::nil(),
            "kind": "issue",
            "reference": "ELY-12",
            "title": "typed",
        }))
        .expect_err("the title comes from the provider, never from the request");
    }
}
