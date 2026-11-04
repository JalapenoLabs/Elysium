// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/bulk`: apply one change to many items at once, as a person's
//! multi-select asks: a state, a priority, a snooze, joining or leaving an initiative,
//! deleting, or restoring.
//!
//! Each item goes through the write its own route makes, in one transaction, and records its
//! own history with the person as the actor (`crate::models::action_item_bulk`). An item the
//! records refuse is answered with the reason while the rest apply, so the answer is `200`
//! listing both. The person acts directly, so no changeset is involved. Resolving owes every
//! resolved item's linked issues their closes, which the watcher lands.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{publish_item_links, publish_item_writes, unique_ids};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::action_item::ActionItemState;
use crate::models::action_item_bulk::{self, BulkChange, MAX_ITEMS};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// Each item to change, once or more; repeats count once.
    item_ids: Vec<Uuid>,
    change: BulkChange,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let now = Utc::now();

    let item_ids = unique_ids(body.item_ids);
    if item_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "itemIds must name at least one item".to_owned(),
        ));
    }
    if item_ids.len() > MAX_ITEMS {
        return Err(ApiError::BadRequest(format!(
            "itemIds may name at most {MAX_ITEMS} items at once"
        )));
    }
    if let BulkChange::Snooze { until: Some(until) } = body.change
        && until <= now
    {
        return Err(ApiError::BadRequest(
            "until must be in the future".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let outcome = action_item_bulk::change_many(
        &mut connection,
        &item_ids,
        body.change,
        current.actor(),
        now,
    )
    .await?;

    let resolved_ids: Vec<Uuid> = match body.change {
        BulkChange::State {
            state: ActionItemState::Resolved,
        } => outcome
            .written
            .iter()
            .filter(|written| !written.history.is_empty())
            .map(|written| written.record.id)
            .collect(),
        _ => Vec::new(),
    };
    let left_initiative_ids = match body.change {
        BulkChange::LeaveInitiative { initiative_id } => vec![initiative_id],
        _ => Vec::new(),
    };
    let items = publish_item_writes(
        &state.events,
        &mut connection,
        outcome.written,
        &left_initiative_ids,
        now,
    )
    .await?;
    // Resolving owed a close to every linked issue still open; the watcher lands them.
    for item_id in &resolved_ids {
        publish_item_links(&state.events, &mut connection, *item_id).await?;
    }
    if !resolved_ids.is_empty() {
        state.links.wake_watcher();
    }

    Ok(Json(json!({ "items": items, "refused": outcome.refused })))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use tower::ServiceExt as _;

    use super::*;
    use crate::models::action_item::{self, ActionItemPriority, NewActionItem, Owner};
    use crate::models::action_item_event::list_for_item;
    use crate::routes;
    use crate::test_support::{
        TEST_PERSON, TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie,
    };

    /// The router's answer to a bulk change, as the test person, from `origin`.
    async fn post_change(state: &AppState, origin: &str, body: &Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/action-items/bulk")
            .header(header::COOKIE, signed_in_cookie(state).await)
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request builds");
        let response = routes::router(state)
            .with_state(state.clone())
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body reads");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
    async fn a_person_resolves_a_selection_and_hears_what_refused() {
        let (url, mut connection) = migrated_database().await;
        let state = app_state(&url).await;
        let mut ids = Vec::new();
        for title in ["first", "second"] {
            let item = NewActionItem {
                title: title.to_owned(),
                notes: String::new(),
                state: ActionItemState::Open,
                priority: ActionItemPriority::Normal,
                due_at: None,
                owner: Owner::User,
                project_ids: Vec::new(),
                initiative_ids: Vec::new(),
            };
            let created = action_item::create(&mut connection, item, TEST_PERSON, Utc::now())
                .await
                .expect("create");
            ids.push(created.record.id);
        }
        let missing = Uuid::now_v7();
        let body = json!({
            "itemIds": [ids[0], ids[1], ids[0], missing],
            "change": { "kind": "state", "state": "resolved" },
        });

        let (status, refused_origin) =
            post_change(&state, "https://elsewhere.example", &body).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(refused_origin["code"], "cross_origin");

        let (status, answer) = post_change(&state, "http://localhost:4000", &body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let item_states: Vec<&Value> = answer["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|item| &item["state"])
            .collect();
        assert_eq!(item_states, [&json!("resolved"), &json!("resolved")]);
        assert_eq!(
            answer["refused"],
            json!([{ "itemId": missing, "message": "no action item has this id" }])
        );

        for id in ids {
            let history = list_for_item(&mut connection, id).await.expect("history");
            let resolved = history.last().expect("an entry");
            assert_eq!(resolved.kind, "state_changed");
            assert_eq!(resolved.actor, format!("user:{TEST_PERSON_ID}"));
        }

        let (status, empty) = post_change(
            &state,
            "http://localhost:4000",
            &json!({ "itemIds": [], "change": { "kind": "delete" } }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{empty}");
    }

    #[test]
    fn a_body_names_items_and_one_change() {
        let item_id = Uuid::now_v7();
        let body: RequestBody = serde_json::from_value(json!({
            "itemIds": [item_id],
            "change": { "kind": "priority", "priority": "urgent" },
        }))
        .expect("parses");
        assert_eq!(body.item_ids, [item_id]);

        serde_json::from_value::<RequestBody>(json!({ "itemIds": [item_id] }))
            .expect_err("a change is required");
        serde_json::from_value::<RequestBody>(json!({
            "itemIds": [item_id],
            "change": { "kind": "delete" },
            "everything": true,
        }))
        .expect_err("unknown fields are refused");
    }
}
