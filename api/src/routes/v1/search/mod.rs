// Copyright © 2026 Jalapeno Labs

//! `/api/v1/search`: the global search's records, found by their titles.
//!
//! The web app's topbar search and its search page find pages and actions in the browser, and
//! ask this route for the action items, projects, coding sessions, and initiatives whose titles
//! match what was typed. See `crate::search` for matching and ranking, and `docs/api.md`,
//! Search.

mod search_records;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(search_records::handle))
}
