//! Cookie construction shared across the auth flows.
//!
//! Every auth cookie Elysium sets carries the same security attributes
//! (`HttpOnly`, `Secure`, `SameSite=Lax`, root path); defining them once here
//! keeps the passkey, password, TOTP, and bootstrap flows from each re-deriving
//! them (and drifting). Two shapes exist: an ephemeral cookie that carries
//! short-lived in-flight ceremony state between a begin/finish request pair, and
//! the session cookie the browser holds once signed in.

use axum_extra::extract::cookie::{Cookie, SameSite};

/// Builds an auth cookie with Elysium's standard security attributes.
///
/// `HttpOnly` keeps it out of JavaScript, `Secure` restricts it to HTTPS, and
/// `SameSite=Lax` is safe because the SPA and API share one origin.
pub fn build(name: &'static str, value: String) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .build()
}

/// Builds the cookie that clears `name` (same attributes, empty value).
///
/// Removing a cookie must echo the attributes it was set with, so this mirrors
/// [`build`] rather than constructing a bare name.
pub fn clearing(name: &'static str) -> Cookie<'static> {
    build(name, String::new())
}
