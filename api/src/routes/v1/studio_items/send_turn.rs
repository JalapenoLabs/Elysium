// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/studio-items/{id}/turns`: send a prompt, with or without a drawing.
//!
//! The body is always `multipart/form-data`, so a plain prompt and a drawn one travel the same
//! way. Fields: `prompt` (required), `satelliteId` (where a continued item runs), and for a
//! drawing `annotated` (the drawing flattened over the view, PNG), `capture` (the clean view,
//! PNG), `sourceAssetId` (the file drawn over), and `cameraOrbit` (the 3D viewer's orbit).
//!
//! The turn runs in the item's live thread, or in a new session that continues the item when
//! the latest thread has ended (`crate::studio::continuation`), whose brief, when it needs
//! one, opens the prompt. A drawing is uploaded into the workspace at
//! `feedback/<feedbackId>/annotated.png` and attached to the turn, so the harness reads it as
//! an image; both images are kept in the item's storage location and recorded as a
//! `studio_feedback` row before the turn starts, which names its turn once the satellite
//! accepts it. See `docs/studio.md`, Feedback.

use anyhow::Context;
use arsox_sdk::client::{ThreadHandle, TurnOptions};
use arsox_sdk::proto::turn::v1::TurnAttachment;
use axum::Json;
use axum::extract::multipart::{Field, Multipart, MultipartRejection};
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use bytes::Bytes;
use chrono::Utc;
use futures_util::stream;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::{StudioFeedbackResponse, StudioItemResponse};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::fleet::views::TurnView;
use crate::models::storage_location::{self, StorageLocation};
use crate::models::studio_asset;
use crate::models::studio_feedback::{self, StudioFeedback};
use crate::models::studio_item::{self, StudioItem};
use crate::realtime::ServerEvent;
use crate::routes::v1::coding_sessions::CodingSessionResponse;
use crate::state::AppState;
use crate::studio::continuation::{self, Continued};

/// The largest drawing. It is handed to the harness as an image, and Arsox refuses a turn
/// attachment past 3.75 MiB because the model APIs do; a viewer capture downscaled to fit
/// keeps every stroke legible.
pub const MAX_ANNOTATED_BYTES: usize = 3_932_160;

/// The largest clean capture. It is only kept, never sent to the agent, so it may be a full
/// 4K PNG of the viewer.
pub const MAX_CAPTURE_BYTES: usize = 12 * 1024 * 1024;

/// The largest request body: both images, plus the text fields.
pub const MAX_BODY_BYTES: usize = MAX_ANNOTATED_BYTES + MAX_CAPTURE_BYTES + 1024 * 1024;

/// The longest prompt, as for a Coding turn: a pasted spec fits.
const MAX_PROMPT_CHARS: usize = 100_000;

/// The longest camera orbit kept, matching the column's check. `model-viewer` writes three
/// short numbers with units.
const MAX_CAMERA_ORBIT_CHARS: usize = 200;

/// Every PNG starts with these bytes.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A prompt as the form sent it.
#[derive(Debug, Default)]
struct TurnForm {
    prompt: String,
    satellite_id: Option<Uuid>,
    source_asset_id: Option<Uuid>,
    camera_orbit: Option<String>,
    annotated: Option<Bytes>,
    capture: Option<Bytes>,
}

/// A drawing, once it is stored and recorded.
#[derive(Debug)]
struct Drawing {
    feedback: StudioFeedback,
    attachment: TurnAttachment,
    location: StorageLocation,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let form =
        read_form(multipart.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?)
            .await?;

    let item = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        let item = studio_item::find(&mut connection, id).await?;
        if let Some(source_asset_id) = form.source_asset_id {
            studio_asset::find_for_item(&mut connection, id, source_asset_id)
                .await
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => ApiError::BadRequest(
                        "sourceAssetId must be one of the item's files".to_owned(),
                    ),
                    other => other.into(),
                })?;
        }
        item
    };
    if item.deleted_at.is_some() {
        return Err(ApiError::Conflict(
            "restore this item before sending it a prompt",
        ));
    }

    let item_thread =
        continuation::thread_for_turn(&state, &item, current.id(), form.satellite_id).await?;

    let mut attachments = Vec::new();
    let mut prompt = String::new();
    if let Some(Continued::Brief(brief)) = &item_thread.continued {
        prompt.push_str(&brief.text);
        attachments.extend(brief.thumbnail.clone());
    }
    prompt.push_str(&form.prompt);

    let drawing = match form.annotated.clone() {
        Some(annotated) => Some(
            keep_drawing(
                &state,
                &item,
                current.id(),
                &item_thread.handle,
                item_thread.session.id,
                &form,
                annotated,
            )
            .await?,
        ),
        None => None,
    };
    if let Some(drawing) = &drawing {
        attachments.push(drawing.attachment.clone());
    }

    let options = TurnOptions {
        attachments,
        ..TurnOptions::default()
    };
    let turn = match item_thread.handle.start_turn_with(prompt, options).await {
        Ok(turn) => turn,
        Err(error) => {
            if let Some(drawing) = &drawing {
                forget_drawing(&state, drawing).await;
            }
            return Err(error.into());
        }
    };
    let turn = TurnView::from(turn.queued());

    let feedback = match drawing {
        Some(drawing) => Some(record_turn(&state, drawing.feedback, &turn.turn_id).await),
        None => None,
    };
    let continued = item_thread.continued.is_some();
    if continued {
        announce_item(&state, &item).await;
    }

    let session = CodingSessionResponse::new(item_thread.session, Some(item_thread.thread));
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "session": session,
            "turn": turn,
            "feedback": feedback,
            "continued": continued,
        })),
    ))
}

/// Reads the form, checking each part against its own limit as it arrives.
///
/// # Errors
/// `400` for a missing or blank prompt, an unknown or repeated field, a value that does not
/// parse, a part over its limit, an image that is not a PNG, or a capture without a drawing.
async fn read_form(mut multipart: Multipart) -> Result<TurnForm, ApiError> {
    let mut form = TurnForm::default();
    let mut has_prompt = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::BadRequest(error.body_text()))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "prompt" => {
                form.prompt = text(field, &name, MAX_PROMPT_CHARS * 4).await?;
                has_prompt = true;
            }
            "satelliteId" => form.satellite_id = Some(uuid(field, &name).await?),
            "sourceAssetId" => form.source_asset_id = Some(uuid(field, &name).await?),
            "cameraOrbit" => {
                form.camera_orbit = Some(text(field, &name, MAX_CAMERA_ORBIT_CHARS).await?);
            }
            "annotated" => form.annotated = Some(png(field, &name, MAX_ANNOTATED_BYTES).await?),
            "capture" => form.capture = Some(png(field, &name, MAX_CAPTURE_BYTES).await?),
            other => {
                return Err(ApiError::BadRequest(format!("unknown field `{other}`")));
            }
        }
    }

    if !has_prompt || form.prompt.trim().is_empty() {
        return Err(ApiError::BadRequest("prompt must not be blank".to_owned()));
    }
    if form.prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(ApiError::BadRequest(format!(
            "prompt must be at most {MAX_PROMPT_CHARS} characters"
        )));
    }
    if form.annotated.is_none()
        && (form.capture.is_some() || form.source_asset_id.is_some() || form.camera_orbit.is_some())
    {
        return Err(ApiError::BadRequest(
            "capture, sourceAssetId, and cameraOrbit describe a drawing; send annotated with them"
                .to_owned(),
        ));
    }
    Ok(form)
}

/// A part's bytes, refused as soon as they pass `limit`.
async fn bytes_of(mut field: Field<'_>, name: &str, limit: usize) -> Result<Bytes, ApiError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| ApiError::BadRequest(error.body_text()))?
    {
        if bytes.len() + chunk.len() > limit {
            return Err(ApiError::BadRequest(format!(
                "`{name}` must be at most {limit} bytes"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(bytes))
}

async fn text(field: Field<'_>, name: &str, limit: usize) -> Result<String, ApiError> {
    let bytes = bytes_of(field, name, limit).await?;
    String::from_utf8(bytes.to_vec())
        .map_err(|_not_utf8| ApiError::BadRequest(format!("`{name}` must be UTF-8 text")))
}

async fn uuid(field: Field<'_>, name: &str) -> Result<Uuid, ApiError> {
    let value = text(field, name, 64).await?;
    value
        .trim()
        .parse()
        .map_err(|_not_uuid| ApiError::BadRequest(format!("`{name}` must be a UUID")))
}

async fn png(field: Field<'_>, name: &str, limit: usize) -> Result<Bytes, ApiError> {
    let bytes = bytes_of(field, name, limit).await?;
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err(ApiError::BadRequest(format!(
            "`{name}` must be a PNG image"
        )));
    }
    Ok(bytes)
}

/// Keeps both images in the item's storage location, records the drawn prompt, and uploads
/// the drawing into the workspace to be attached to the turn. Anything kept is removed again
/// when a later step fails.
///
/// # Errors
/// `409` when the images would pass the location's storage limit, and the failures of the
/// provider, the database, and the satellite.
async fn keep_drawing(
    state: &AppState,
    item: &StudioItem,
    actor: Uuid,
    workspace: &ThreadHandle,
    session_id: i64,
    form: &TurnForm,
    annotated: Bytes,
) -> Result<Drawing, ApiError> {
    let feedback_id = Uuid::now_v7();
    let directory = format!(
        "{}/feedback/{feedback_id}",
        studio_asset::item_directory(item.id)
    );
    let annotated_path = format!("{directory}/annotated.png");
    let capture_path = form
        .capture
        .as_ref()
        .map(|_capture| format!("{directory}/capture.png"));
    let capture_size = form.capture.as_ref().map(Bytes::len);
    let location = store_images(
        state,
        item,
        (&annotated_path, &annotated),
        capture_path.as_deref().zip(form.capture.as_ref()),
    )
    .await?;

    let workspace_path = format!("feedback/{feedback_id}/annotated.png");
    let annotated_size = annotated.len() as u64;
    let written = workspace
        .write_file(
            &workspace_path,
            annotated_size,
            stream::once(async move { Ok::<_, std::io::Error>(annotated) }),
        )
        .await;
    let kept_paths: Vec<&str> = std::iter::once(annotated_path.as_str())
        .chain(capture_path.as_deref())
        .collect();
    if let Err(error) = written {
        remove_files(state, &location, &kept_paths).await;
        return Err(error.into());
    }

    let feedback = StudioFeedback {
        id: feedback_id,
        studio_item_id: item.id,
        session_id: Some(session_id),
        turn_id: None,
        prompt: form.prompt.clone(),
        source_asset_id: form.source_asset_id,
        camera_orbit: form.camera_orbit.clone(),
        annotated_storage_path: annotated_path.clone(),
        annotated_size_bytes: i64::try_from(annotated_size).unwrap_or(i64::MAX),
        capture_storage_path: capture_path.clone(),
        capture_size_bytes: capture_size.map(|size| i64::try_from(size).unwrap_or(i64::MAX)),
        created_by: actor,
        created_at: Utc::now(),
    };
    let recorded = match state.database.get().await {
        Ok(mut connection) => studio_feedback::create(&mut connection, &feedback)
            .await
            .map_err(ApiError::from),
        Err(error) => Err(anyhow::Error::from(error)
            .context("no database connection available")
            .into()),
    };
    let feedback = match recorded {
        Ok(feedback) => feedback,
        Err(error) => {
            remove_files(state, &location, &kept_paths).await;
            return Err(error);
        }
    };

    Ok(Drawing {
        feedback,
        attachment: TurnAttachment {
            path: workspace_path,
            content_type: Some("image/png".to_owned()),
            size_bytes: annotated_size,
        },
        location,
    })
}

/// Keeps a drawing, and its clean capture when there is one, in the item's storage location,
/// within the location's limit. A capture that fails takes the drawing with it.
///
/// # Errors
/// `409` when the images would pass the location's storage limit, and the failures of the
/// provider and the database.
async fn store_images(
    state: &AppState,
    item: &StudioItem,
    annotated: (&str, &Bytes),
    capture: Option<(&str, &Bytes)>,
) -> Result<StorageLocation, ApiError> {
    let (annotated_path, annotated_bytes) = annotated;
    let new_bytes = annotated_bytes.len() + capture.map_or(0, |(_path, bytes)| bytes.len());

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let location = storage_location::find(&mut connection, item.storage_location_id).await?;
    if let Some(limit) = location.storage_limit_bytes {
        let used = studio_item::bytes_in_location(&mut connection, location.id).await?;
        let remaining = u64::try_from(limit.saturating_sub(used)).unwrap_or_default();
        if new_bytes as u64 > remaining {
            return Err(ApiError::Conflict(
                "the item's storage location is full; free space or raise its limit",
            ));
        }
    }
    drop(connection);

    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;
    upload(
        state,
        &location,
        &access_key,
        annotated_path,
        annotated_bytes.clone(),
    )
    .await?;
    let Some((capture_path, capture_bytes)) = capture else {
        return Ok(location);
    };
    if let Err(error) = upload(
        state,
        &location,
        &access_key,
        capture_path,
        capture_bytes.clone(),
    )
    .await
    {
        remove_files(state, &location, &[annotated_path]).await;
        return Err(error);
    }
    Ok(location)
}

async fn upload(
    state: &AppState,
    location: &StorageLocation,
    access_key: &secrecy::SecretString,
    path: &str,
    bytes: Bytes,
) -> Result<(), ApiError> {
    let size = bytes.len() as u64;
    state
        .storage
        .upload(
            location,
            access_key,
            path,
            stream::once(async move { Ok::<_, std::io::Error>(bytes) }),
            size,
            Some("image/png"),
        )
        .await?;
    Ok(())
}

/// Removes files a failed request kept. A file that will not go is logged and stays until a
/// permanent delete empties the item's directory.
async fn remove_files(state: &AppState, location: &StorageLocation, paths: &[&str]) {
    let access_key = match location.access_key(&state.cipher) {
        Ok(access_key) => access_key,
        Err(error) => {
            event!(
                name: "studio.feedback.orphaned_file",
                Level::WARN,
                error.message = %error,
                "could not remove a drawing whose prompt failed: the access key cannot be decrypted",
            );
            return;
        }
    };
    for path in paths {
        if let Err(error) = state.storage.delete(location, &access_key, path).await {
            event!(
                name: "studio.feedback.orphaned_file",
                Level::WARN,
                file.path = %path,
                error.message = %error,
                "could not remove a drawing whose prompt failed",
            );
        }
    }
}

/// Forgets a drawing whose turn the satellite refused: its row, then its files.
async fn forget_drawing(state: &AppState, drawing: &Drawing) {
    match state.database.get().await {
        Ok(mut connection) => {
            if let Err(error) = studio_feedback::delete(&mut connection, drawing.feedback.id).await
            {
                event!(
                    name: "studio.feedback.orphaned_row",
                    Level::ERROR,
                    studio_feedback.id = %drawing.feedback.id,
                    error.message = %error,
                    "could not forget a drawn prompt whose turn was refused",
                );
            }
        }
        Err(error) => event!(
            name: "studio.feedback.orphaned_row",
            Level::ERROR,
            studio_feedback.id = %drawing.feedback.id,
            error.message = %error,
            "could not forget a drawn prompt whose turn was refused",
        ),
    }
    let paths: Vec<&str> = std::iter::once(drawing.feedback.annotated_storage_path.as_str())
        .chain(drawing.feedback.capture_storage_path.as_deref())
        .collect();
    remove_files(state, &drawing.location, &paths).await;
}

/// Names the turn a drawn prompt started, and tells clients about it. A failure to record the
/// turn is logged: the turn runs, and the drawing stays without its turn's name.
async fn record_turn(
    state: &AppState,
    feedback: StudioFeedback,
    turn_id: &str,
) -> StudioFeedbackResponse {
    let updated = match state.database.get().await {
        Ok(mut connection) => studio_feedback::set_turn_id(&mut connection, feedback.id, turn_id)
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    let feedback = match updated {
        Ok(updated) => updated,
        Err(message) => {
            event!(
                name: "studio.feedback.turn_unrecorded",
                Level::WARN,
                studio_feedback.id = %feedback.id,
                error.message = %message,
                "could not record the turn a drawn prompt started",
            );
            feedback
        }
    };
    let response = StudioFeedbackResponse::from(feedback);
    state
        .events
        .publish(&ServerEvent::StudioFeedbackCreated(response.clone()));
    response
}

/// Tells clients the item changed: it continued on a new session.
async fn announce_item(state: &AppState, item: &StudioItem) {
    let assets = match state.database.get().await {
        Ok(mut connection) => studio_asset::list_for_items(&mut connection, &[item.id])
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    match assets {
        Ok(assets) => {
            let references: Vec<_> = assets.iter().collect();
            state
                .events
                .publish(&ServerEvent::StudioItemUpserted(StudioItemResponse::new(
                    item.clone(),
                    &references,
                )));
        }
        Err(message) => event!(
            name: "studio.item.announce_failure",
            Level::WARN,
            studio_item.id = %item.id,
            error.message = %message,
            "could not announce an item that continued on a new session",
        ),
    }
}

#[cfg(test)]
mod flow_tests;

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::extract::FromRequest as _;
    use axum::http::Request;

    use super::*;
    use crate::test_support::multipart_form;

    async fn form(parts: &[(&str, &[u8])]) -> Result<TurnForm, ApiError> {
        let (content_type, body) = multipart_form(parts);
        let request = Request::builder()
            .method("POST")
            .header("content-type", content_type)
            .body(Body::from(body))
            .expect("request");
        let multipart = Multipart::from_request(request, &())
            .await
            .expect("multipart");
        read_form(multipart).await
    }

    fn png_bytes() -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(b"pixels");
        bytes
    }

    #[tokio::test]
    async fn a_plain_prompt_reads_alone() {
        let form = form(&[("prompt", b"Make it yellower")])
            .await
            .expect("form");
        assert_eq!(form.prompt, "Make it yellower");
        assert!(form.annotated.is_none());
    }

    #[tokio::test]
    async fn a_drawn_prompt_reads_every_part() {
        let source = Uuid::now_v7().to_string();
        let png = png_bytes();
        let form = form(&[
            ("prompt", b"Shorten the stem"),
            ("sourceAssetId", source.as_bytes()),
            ("cameraOrbit", b"30deg 75deg 2m"),
            ("annotated", &png),
            ("capture", &png),
        ])
        .await
        .expect("form");
        assert_eq!(form.source_asset_id.map(|id| id.to_string()), Some(source));
        assert_eq!(form.camera_orbit.as_deref(), Some("30deg 75deg 2m"));
        assert!(form.annotated.is_some() && form.capture.is_some());
    }

    #[tokio::test]
    async fn a_blank_or_missing_prompt_is_refused() {
        form(&[("prompt", b"   ")]).await.expect_err("refused");
        form(&[("annotated", &png_bytes())])
            .await
            .expect_err("refused");
    }

    #[tokio::test]
    async fn an_image_that_is_not_a_png_is_refused() {
        form(&[("prompt", b"Fix it"), ("annotated", b"GIF89a...")])
            .await
            .expect_err("a GIF is refused");
    }

    #[tokio::test]
    async fn a_drawing_past_its_limit_is_refused_while_it_is_read() {
        let mut huge = png_bytes();
        huge.resize(MAX_ANNOTATED_BYTES + 1, 0);
        form(&[("prompt", b"Fix it"), ("annotated", &huge)])
            .await
            .expect_err("an oversized drawing is refused");
    }

    #[tokio::test]
    async fn drawing_details_without_a_drawing_are_refused() {
        form(&[("prompt", b"Fix it"), ("capture", &png_bytes())])
            .await
            .expect_err("a capture needs a drawing");
        form(&[("prompt", b"Fix it"), ("cameraOrbit", b"0deg 0deg 1m")])
            .await
            .expect_err("an orbit needs a drawing");
    }

    #[tokio::test]
    async fn an_unknown_field_is_refused() {
        form(&[("prompt", b"Fix it"), ("satelite", b"typo")])
            .await
            .expect_err("a misspelled field is refused");
    }
}
