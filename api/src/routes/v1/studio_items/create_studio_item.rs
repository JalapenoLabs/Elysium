// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/studio-items`: start an item and its first thread, with the prompt queued as
//! the thread's first turn.
//!
//! The item row is written first, because its session names it. A create either yields the
//! item with its first turn queued or nothing: a thread the satellite refuses, or a first turn
//! it refuses, takes the item back out.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;
use validator::Validate;

use super::StudioItemResponse;
use super::thread::studio_thread_settings;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::studio_item::{self, NewStudioItem};
use crate::models::{project, satellite, storage_location};
use crate::realtime::ServerEvent;
use crate::routes::v1::coding_sessions::open::{self, SessionOpening};
use crate::routes::v1::coding_sessions::{CodingSessionResponse, validate_not_blank};
use crate::state::AppState;

/// Longest title taken from a prompt when none is given; a tile shows about this much.
const DERIVED_TITLE_MAX_CHARACTERS: usize = 80;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// The first prompt, queued as the first turn. Large enough for a pasted brief.
    #[validate(
        length(min = 1, max = 100_000),
        custom(function = "validate_not_blank")
    )]
    prompt: String,
    /// Taken from the prompt's first line when absent.
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    title: Option<String>,
    project_id: Option<Uuid>,
    storage_location_id: Uuid,
    satellite_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // Everything the item names is looked up before a thread exists, so an unknown id
    // answers here instead of after a thread was opened.
    if let Some(project_id) = body.project_id {
        project::find(&mut connection, project_id).await?;
    }
    let satellite = satellite::find(&mut connection, body.satellite_id).await?;
    storage_location::find_for_project(&mut connection, body.storage_location_id, body.project_id)
        .await
        .map_err(|error| match error {
            diesel::result::Error::NotFound => ApiError::BadRequest(
                "the storage location does not exist, or is not one this item's project may use"
                    .to_owned(),
            ),
            other => other.into(),
        })?;
    drop(connection);

    let settings = studio_thread_settings(&state, body.project_id).await?;
    let title = body
        .title
        .map_or_else(|| title_from_prompt(&body.prompt), |title| title.trim().to_owned());
    let item = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        studio_item::create(
            &mut connection,
            &NewStudioItem {
                created_by: current.id(),
                id: Uuid::now_v7(),
                title: title.clone(),
                prompt: body.prompt.clone(),
                project_id: body.project_id,
                storage_location_id: body.storage_location_id,
            },
        )
        .await?
    };

    let opened = match open::open(
        &state,
        SessionOpening {
            created_by: current.id(),
            satellite_id: satellite.id,
            settings,
            title,
            project_id: item.project_id,
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: Some(item.id),
        },
    )
    .await
    {
        Ok(opened) => opened,
        Err(open_error) => {
            forget_item(&state, item.id).await;
            return Err(open_error);
        }
    };
    if let Err(turn_error) = opened.handle.start_turn(body.prompt).await {
        open::discard(&state, &opened).await;
        forget_item(&state, item.id).await;
        return Err(turn_error.into());
    }
    open::announce(&state, &opened);

    let response = StudioItemResponse::new(item, &[]);
    state
        .events
        .publish(&ServerEvent::StudioItemUpserted(response.clone()));
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "item": response,
            "session": CodingSessionResponse::new(opened.session, Some(opened.thread)),
        })),
    ))
}

/// Removes an item whose first thread could not be opened. A row that will not go is logged;
/// it holds no files and shows in the grid as an item without a session.
async fn forget_item(state: &AppState, id: Uuid) {
    let removed = match state.database.get().await {
        Ok(mut connection) => studio_item::delete(&mut connection, id)
            .await
            .map_err(anyhow::Error::from),
        Err(pool_error) => Err(anyhow::Error::from(pool_error)),
    };
    if let Err(error) = removed {
        event!(
            name: "studio.create.orphaned_item",
            Level::ERROR,
            studio_item.id = %id,
            error.message = %error,
            "could not remove an item whose thread was not opened; delete it by hand",
        );
    }
}

/// A title from the prompt's first non-blank line, cut to a tile's width at a character
/// boundary with an ellipsis when it is longer.
fn title_from_prompt(prompt: &str) -> String {
    let first_line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if first_line.chars().count() <= DERIVED_TITLE_MAX_CHARACTERS {
        return first_line.to_owned();
    }
    let cut: String = first_line
        .chars()
        .take(DERIVED_TITLE_MAX_CHARACTERS - 1)
        .collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_come_from_the_first_line_and_fit_a_tile() {
        assert_eq!(
            title_from_prompt("\n  Model me a banana with Blender\nMake it ripe."),
            "Model me a banana with Blender"
        );

        let long = "a".repeat(200);
        let title = title_from_prompt(&long);
        assert_eq!(title.chars().count(), DERIVED_TITLE_MAX_CHARACTERS);
        assert!(title.ends_with('…'));

        let multibyte = "é".repeat(100);
        assert_eq!(
            title_from_prompt(&multibyte).chars().count(),
            DERIVED_TITLE_MAX_CHARACTERS,
            "cut on characters, never inside one"
        );
    }

    #[test]
    fn bodies_require_a_prompt_a_location_and_a_satellite() {
        let valid: RequestBody = serde_json::from_value(json!({
            "prompt": "Model me a banana",
            "storageLocationId": Uuid::nil(),
            "satelliteId": Uuid::nil(),
        }))
        .expect("parses");
        valid.validate().expect("a project and title are optional");

        let blank: RequestBody = serde_json::from_value(json!({
            "prompt": "   ",
            "storageLocationId": Uuid::nil(),
            "satelliteId": Uuid::nil(),
        }))
        .expect("parses");
        assert!(
            blank
                .validate()
                .expect_err("a blank prompt")
                .field_errors()
                .contains_key("prompt")
        );

        serde_json::from_value::<RequestBody>(json!({
            "prompt": "Model me a banana",
            "satelliteId": Uuid::nil(),
        }))
        .expect_err("a storage location is required");
    }
}
