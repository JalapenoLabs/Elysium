// Copyright © 2026 Jalapeno Labs

//! `GET /api/ok`: liveness probe.

/// Answers `ok` whenever the process can serve requests at all.
pub async fn handle() -> &'static str {
    "ok"
}
