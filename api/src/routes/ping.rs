// Copyright © 2026 Jalapeno Labs

//! `GET /api/ping`: round-trip check.

/// Answers `pong`.
pub async fn handle() -> &'static str {
    "pong"
}
