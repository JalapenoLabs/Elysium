// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/workspace-settings/agent-configuration/fetch`: fetch the configuration
//! repository again now, for an admin who changed what it holds or what its token may read.
//! The engine fetches it (`crate::agent_configuration::keeper`) and announces how it went as
//! `workspaceSettings.updated`, so this answers `202 Accepted` at once.

use axum::extract::State;
use axum::http::StatusCode;

use crate::auth::AdminUser;
use crate::realtime::Signal;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>, _admin: AdminUser) -> StatusCode {
    state.events.signal(&Signal::AgentConfiguration);
    StatusCode::ACCEPTED
}
