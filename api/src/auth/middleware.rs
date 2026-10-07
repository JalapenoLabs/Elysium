// Copyright © 2026 Jalapeno Labs

//! The layers every `/api/v1` request passes through. See the module docs in `super`.

use anyhow::Context;
use axum::extract::{OriginalUri, Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use tracing::{Level, event};

use super::kratos::{AssuranceLevel, KratosError, SESSION_COOKIE};
use super::{AccessRefusal, Principal, sessions};
use crate::errors::ApiError;
use crate::models::user::{self, Profile, UserStatus};
use crate::state::AppState;

/// How stale `users.last_seen_at` may get. Coarse on purpose: it is shown as "last seen",
/// and a finer one would cost a write per request.
const LAST_SEEN_GRANULARITY: chrono::Duration = chrono::Duration::minutes(5);

/// The event stream, which an open tab holds without anyone using it.
const EVENT_STREAM_PATH: &str = "/api/v1/events";

/// Resolves the session cookie to a person, creating their account the first time, and
/// hands the handler a [`Principal`]. Refuses requests with no session, or one still
/// waiting on its second factor.
pub async fn authenticate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Nested routers see their path with the prefix stripped; the original has it whole.
    let path = request
        .extensions()
        .get::<OriginalUri>()
        .map_or_else(|| request.uri().path(), |original| original.path());
    let activity = activity_of(path);
    match principal(&state, request.headers(), activity).await {
        Ok(principal) => {
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}

/// Refuses people who may not use the workspace yet, or at all. Runs inside
/// [`authenticate`].
pub async fn require_access(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(principal) = request.extensions().get::<Principal>() else {
        return ApiError::Internal(anyhow::anyhow!(
            "require_access ran without authenticate in front of it"
        ))
        .into_response();
    };

    match check_access(&state, principal).await {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

/// Refuses an unsafe request (anything but `GET`, `HEAD`, and `OPTIONS`) whose `Origin` is
/// not Elysium's own.
///
/// The session cookie is `SameSite=Lax`, which keeps other sites' requests from carrying it,
/// but a sibling subdomain counts as the same site. The `Origin` check closes that gap.
/// Browsers send `Origin` on every such request, so a missing one is refused too.
pub async fn require_same_origin(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if is_safe(request.method()) {
        return next.run(request).await;
    }

    let expected = state.auth.public_url.origin().ascii_serialization();
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin == Some(expected.as_str()) {
        return next.run(request).await;
    }

    event!(
        name: "auth.origin.refused",
        Level::WARN,
        http.request.method = %request.method(),
        url.path = request.uri().path(),
        origin = origin.unwrap_or("<none>"),
        "refused an unsafe request from another origin",
    );
    ApiError::AccessRefused(AccessRefusal::CrossOrigin).into_response()
}

/// Whether a request is still allowed: the session is live and the person may use the
/// workspace. The event stream asks this as it runs, so a revoked session or a disabled
/// person stops receiving events.
pub async fn still_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    let Ok(principal) = principal(state, headers, Activity::Passive).await else {
        return false;
    };
    check_access(state, &principal).await.is_ok()
}

/// Whether a request counts as someone using Elysium, which keeps their session alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Activity {
    Use,
    Passive,
}

/// An open tab's event stream is not someone using Elysium, so it never extends a session;
/// every other request is.
fn activity_of(path: &str) -> Activity {
    if path == EVENT_STREAM_PATH {
        return Activity::Passive;
    }
    Activity::Use
}

async fn principal(
    state: &AppState,
    headers: &HeaderMap,
    activity: Activity,
) -> Result<Principal, ApiError> {
    let Some(cookie) = session_cookie(headers) else {
        return Err(ApiError::AccessRefused(AccessRefusal::Unauthenticated));
    };

    let session = match sessions::resolve(&state.redis, &state.auth.kratos, cookie).await {
        Ok(session) => session,
        Err(KratosError::NoSession) => {
            return Err(ApiError::AccessRefused(AccessRefusal::Unauthenticated));
        }
        Err(KratosError::SecondFactorRequired) => {
            return Err(ApiError::AccessRefused(AccessRefusal::SecondFactorRequired));
        }
        Err(error) => return Err(ApiError::BadGateway(error.to_string())),
    };

    let traits = &session.identity.traits;
    let profile = Profile::new(session.identity.id, &traits.email, &traits.name);
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let person = user::provision(&mut connection, &profile).await?;
    user::touch_last_seen(
        &mut connection,
        person.id,
        Utc::now(),
        LAST_SEEN_GRANULARITY,
    )
    .await?;
    if activity == Activity::Use {
        sessions::extend_if_due(&state.redis, &state.auth.kratos, cookie, &session).await;
    }

    Ok(Principal {
        user: person,
        assurance: session.authenticator_assurance_level,
    })
}

async fn check_access(state: &AppState, principal: &Principal) -> Result<(), ApiError> {
    match principal.user.status {
        Some(UserStatus::Active) => {}
        Some(UserStatus::Pending) => {
            return Err(ApiError::AccessRefused(AccessRefusal::AccountPending));
        }
        Some(UserStatus::Disabled) | None => {
            return Err(ApiError::AccessRefused(AccessRefusal::AccountDisabled));
        }
    }

    // Kratos already demands the second factor from anyone who has one
    // (`required_aal: highest_available`), so a one-factor session here means the person has
    // no authenticator app at all.
    if principal.assurance < AssuranceLevel::Aal2 {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        if user::workspace_settings(&mut connection).await?.require_mfa {
            return Err(ApiError::AccessRefused(
                AccessRefusal::MfaEnrollmentRequired,
            ));
        }
    }
    Ok(())
}

/// The value of the session cookie, if the request carries one.
fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn cookies(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(header::COOKIE, HeaderValue::from_str(value).expect("valid"));
        }
        headers
    }

    #[test]
    fn finds_the_session_cookie_among_others() {
        let headers = cookies(&["csrf_token_1=abc; elysium_session=ory_st_1; theme=dark"]);
        assert_eq!(session_cookie(&headers), Some("ory_st_1"));

        let split = cookies(&["csrf_token_1=abc", "elysium_session=ory_st_2"]);
        assert_eq!(session_cookie(&split), Some("ory_st_2"));
    }

    #[test]
    fn ignores_missing_empty_and_lookalike_cookies() {
        assert_eq!(session_cookie(&HeaderMap::new()), None);
        assert_eq!(session_cookie(&cookies(&["elysium_session="])), None);
        assert_eq!(session_cookie(&cookies(&["elysium_session_old=x"])), None);
    }

    #[test]
    fn only_the_event_stream_leaves_a_session_to_expire() {
        assert_eq!(activity_of("/api/v1/events"), Activity::Passive);
        assert_eq!(activity_of("/api/v1/projects"), Activity::Use);
        assert_eq!(activity_of("/api/v1/me"), Activity::Use);
    }

    #[test]
    fn only_reads_are_safe() {
        assert!(is_safe(&Method::GET));
        assert!(is_safe(&Method::HEAD));
        assert!(is_safe(&Method::OPTIONS));
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(!is_safe(&method), "{method} is unsafe");
        }
    }
}
