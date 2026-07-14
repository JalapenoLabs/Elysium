//! The user profile surface: names, bio, derived presence, and manual status.
//!
//! Presence (`online` / `away` / `offline`) is derived, never stored: a user is
//! online when they hold a live SSE connection (`SseRegistry`), otherwise offline
//! (`away` awaits a client idle signal). The manual status is the one thing the
//! user sets, and it wins over derived presence only until it expires. See
//! `docs/identity.md`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};

use crate::proto::common::v1::{Timestamp, Uuid as ProtoUuid};
use crate::proto::realtime::v1::event_envelope::Payload;
use crate::proto::realtime::v1::ProfileUpdated;
use crate::proto::user::v1::{
    ManualStatusKind, Presence, SetStatusRequest, UpdateProfileRequest, User,
};
use crate::protobuf::Protobuf;
use crate::session::SessionUser;
use crate::state::AppState;

/// Default lifetime of a manual status when the request does not specify one.
const DEFAULT_STATUS_DURATION: Duration = Duration::hours(24);

/// `GET /api/v1/users/me`: the authenticated user's full profile.
pub async fn get_me(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    load(&state, &principal.sub).await
}

/// `PATCH /api/v1/users/me`: update the editable profile fields.
///
/// `display_name` falls back to `given_name` when blank, so the render field is
/// never empty for a user who only entered their given name.
pub async fn update_me(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Protobuf(request): Protobuf<UpdateProfileRequest>,
) -> Response {
    let given_name = request.given_name.trim();
    let family_name = request.family_name.trim();
    let display_name = match request.display_name.trim() {
        "" => given_name,
        chosen => chosen,
    };

    // `avatar_url` is an optional partial update: COALESCE leaves it unchanged when
    // the field is omitted, and sets it (including to empty, which clears) when sent.
    let result = sqlx::query(
        "UPDATE users
            SET given_name = $1, family_name = $2, display_name = $3, bio = $4,
                avatar_url = COALESCE($5, avatar_url)
          WHERE subject = $6",
    )
    .bind(given_name)
    .bind(family_name)
    .bind(display_name)
    .bind(request.bio.trim())
    .bind(request.avatar_url.as_deref().map(str::trim))
    .bind(&principal.sub)
    .execute(&state.pool)
    .await;

    if let Err(error) = result {
        tracing::error!(error = %error, "profile update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    load_and_publish(&state, &principal.sub).await
}

/// `PUT /api/v1/users/me/status`: set or clear the manual status override.
///
/// A `kind` of `UNSPECIFIED` clears the override; otherwise it is stored with an
/// expiry (`duration_seconds`, or 24h by default) after which presence reverts to
/// automatic.
pub async fn set_status(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Protobuf(request): Protobuf<SetStatusRequest>,
) -> Response {
    let kind = request.kind();
    let result = if matches!(kind, ManualStatusKind::Unspecified) {
        sqlx::query(
            "UPDATE users
                SET manual_status = NULL, manual_status_text = '', manual_status_expires_at = NULL
              WHERE subject = $1",
        )
        .bind(&principal.sub)
        .execute(&state.pool)
        .await
    } else {
        let duration = if request.duration_seconds > 0 {
            Duration::seconds(request.duration_seconds)
        } else {
            DEFAULT_STATUS_DURATION
        };
        // Only a custom status carries a label; the fixed kinds render from `kind`.
        let text = if matches!(kind, ManualStatusKind::Custom) {
            request.text.trim()
        } else {
            ""
        };
        sqlx::query(
            "UPDATE users
                SET manual_status = $1, manual_status_text = $2,
                    manual_status_expires_at = now() + make_interval(secs => $3)
              WHERE subject = $4",
        )
        .bind(kind_to_db(kind))
        .bind(text)
        .bind(duration.num_seconds() as f64)
        .bind(&principal.sub)
        .execute(&state.pool)
        .await
    };

    if let Err(error) = result {
        tracing::error!(error = %error, "status update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    load_and_publish(&state, &principal.sub).await
}

/// Row shape for the profile read.
#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: uuid::Uuid,
    subject: String,
    email: String,
    given_name: String,
    family_name: String,
    display_name: String,
    avatar_url: String,
    bio: String,
    manual_status: Option<String>,
    manual_status_text: String,
    manual_status_expires_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Builds the current `User` for a subject, deriving presence.
///
/// Shared with the session bootstrap (see `crate::context`), which returns the
/// same profile shape as `GET /api/v1/users/me`.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn fetch(state: &AppState, subject: &str) -> sqlx::Result<Option<User>> {
    let row = sqlx::query_as::<_, ProfileRow>(
        "SELECT id, subject, email, given_name, family_name, display_name, avatar_url, bio,
                manual_status, manual_status_text, manual_status_expires_at,
                last_seen_at, created_at, updated_at
           FROM users WHERE subject = $1",
    )
    .bind(subject)
    .fetch_optional(&state.pool)
    .await?;

    Ok(row.map(|row| to_user(row, state.sse.is_online(subject))))
}

/// Loads the user by subject and returns the `User` protobuf.
async fn load(state: &AppState, subject: &str) -> Response {
    match fetch(state, subject).await {
        Ok(Some(user)) => Protobuf(user).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "profile load failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Loads the profile, publishes `profile.updated` to teammates, and responds.
///
/// Used after a mutation so a user's other windows and their teammates see the
/// change live (see `docs/realtime.md`).
async fn load_and_publish(state: &AppState, subject: &str) -> Response {
    match fetch(state, subject).await {
        Ok(Some(user)) => {
            crate::events::publish_teammates(
                state,
                subject,
                Payload::ProfileUpdated(ProfileUpdated {
                    user: Some(user.clone()),
                }),
            )
            .await;
            Protobuf(user).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "profile load failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Builds the `User` protobuf from a row, deriving presence and the live override.
fn to_user(row: ProfileRow, online: bool) -> User {
    let presence = if online {
        Presence::Online
    } else {
        Presence::Offline
    };

    // The override applies only while unexpired; a lapsed one reads as none.
    let active_override = row
        .manual_status_expires_at
        .filter(|expires_at| *expires_at > Utc::now())
        .and(row.manual_status.as_deref());
    let manual_status = active_override.map_or(ManualStatusKind::Unspecified, kind_from_db);
    let manual_status_text = if active_override.is_some() {
        row.manual_status_text
    } else {
        String::new()
    };
    let manual_status_expires_at = if active_override.is_some() {
        row.manual_status_expires_at.map(to_timestamp)
    } else {
        None
    };

    User {
        id: Some(ProtoUuid {
            value: row.id.to_string(),
        }),
        subject: row.subject,
        email: row.email,
        given_name: row.given_name,
        family_name: row.family_name,
        display_name: row.display_name,
        avatar_url: row.avatar_url,
        bio: row.bio,
        presence: presence as i32,
        manual_status: manual_status as i32,
        manual_status_text,
        manual_status_expires_at,
        last_seen_at: row.last_seen_at.map(to_timestamp),
        created_at: Some(to_timestamp(row.created_at)),
        updated_at: Some(to_timestamp(row.updated_at)),
    }
}

/// Maps a manual-status kind to its stored text value.
fn kind_to_db(kind: ManualStatusKind) -> &'static str {
    match kind {
        ManualStatusKind::OutOfOffice => "out_of_office",
        ManualStatusKind::DoNotDisturb => "do_not_disturb",
        ManualStatusKind::Custom => "custom",
        ManualStatusKind::Unspecified => "",
    }
}

/// Maps a stored manual-status value back to its kind; unknown reads as none.
fn kind_from_db(value: &str) -> ManualStatusKind {
    match value {
        "out_of_office" => ManualStatusKind::OutOfOffice,
        "do_not_disturb" => ManualStatusKind::DoNotDisturb,
        "custom" => ManualStatusKind::Custom,
        _ => ManualStatusKind::Unspecified,
    }
}

/// Converts a UTC instant into the canonical proto `Timestamp`.
fn to_timestamp(instant: DateTime<Utc>) -> Timestamp {
    Timestamp {
        epoch_seconds: instant.timestamp(),
        nanos: instant.timestamp_subsec_nanos(),
        timezone: "Etc/UTC".to_string(),
    }
}
