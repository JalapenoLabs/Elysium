//! Realtime event publishing: fan-out helpers over the SSE bus.
//!
//! [`crate::sse`] owns the per-user channels and transport; this module resolves
//! recipients and publishes typed [`Payload`] events to them. Org changes reach
//! every member; presence and profile changes reach a user's teammates (everyone
//! who shares an org, which includes the user's own other windows). See
//! `docs/realtime.md`.

use uuid::Uuid;

use crate::proto::common::v1::Uuid as ProtoUuid;
use crate::proto::realtime::v1::event_envelope::Payload;
use crate::proto::realtime::v1::{PresenceChanged, SessionChanged};
use crate::proto::user::v1::Presence;
use crate::state::AppState;
use crate::users;

/// Publishes an event to every member of an org (best-effort).
pub async fn publish_org(state: &AppState, org_id: Uuid, payload: Payload) {
    match member_subjects(state, org_id).await {
        Ok(subjects) => {
            for subject in subjects {
                state.sse.send(&subject, payload.clone());
            }
        }
        Err(error) => tracing::error!(error = %error, "failed to resolve org members for publish"),
    }
}

/// Publishes an event to a user's teammates: everyone sharing an org with them,
/// which includes the user's own other windows (best-effort).
pub async fn publish_teammates(state: &AppState, subject: &str, payload: Payload) {
    match teammate_subjects(state, subject).await {
        Ok(subjects) => {
            for teammate in subjects {
                state.sse.send(&teammate, payload.clone());
            }
        }
        Err(error) => tracing::error!(error = %error, "failed to resolve teammates for publish"),
    }
}

/// Publishes an event to a single user's connections.
pub fn publish_user(state: &AppState, subject: &str, payload: Payload) {
    state.sse.send(subject, payload);
}

/// Announces a user's presence change to their teammates, fire-and-forget.
///
/// Spawns because it runs from the SSE connection guard's `Drop` (offline) and the
/// connect path (online), neither of which should block; a failure only means a
/// missed presence blip.
pub fn announce_presence(state: &AppState, subject: String, online: bool) {
    let state = state.clone();
    tokio::spawn(async move {
        let Ok(Some(user_id)) = users::id_for_subject(&state.pool, &subject).await else {
            return;
        };
        let presence = if online {
            Presence::Online
        } else {
            Presence::Offline
        };
        let payload = Payload::PresenceChanged(PresenceChanged {
            user_id: Some(ProtoUuid {
                value: user_id.to_string(),
            }),
            presence: presence as i32,
        });
        publish_teammates(&state, &subject, payload).await;
    });
}

/// Notifies a user's connections that their session accounts changed (e.g. signed
/// out elsewhere), so their clients re-read `/session/accounts` and `/me`.
pub fn announce_session_change(state: &AppState, subject: &str, reason: &str) {
    publish_user(
        state,
        subject,
        Payload::SessionChanged(SessionChanged {
            reason: reason.to_string(),
        }),
    );
}

/// The subjects of every member of an org.
async fn member_subjects(state: &AppState, org_id: Uuid) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT u.subject FROM users u JOIN org_members m ON m.user_id = u.id WHERE m.org_id = $1",
    )
    .bind(org_id)
    .fetch_all(&state.pool)
    .await
}

/// The subjects of everyone who shares at least one org with `subject` (including
/// the user themselves).
async fn teammate_subjects(state: &AppState, subject: &str) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT u2.subject
           FROM users u1
           JOIN org_members m1 ON m1.user_id = u1.id
           JOIN org_members m2 ON m2.org_id = m1.org_id
           JOIN users u2 ON u2.id = m2.user_id
          WHERE u1.subject = $1",
    )
    .bind(subject)
    .fetch_all(&state.pool)
    .await
}
