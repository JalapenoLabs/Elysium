// Copyright © 2026 Jalapeno Labs

//! Tools for the Arsox satellites coding sessions run on.
//!
//! They call the satellite routes' own functions (`routes::v1::satellites`), so a satellite
//! registered here is sealed, watched, and announced exactly as one added in Settings. A
//! satellite's bearer secret is accepted when creating or updating one and never answered
//! back; rmcp's argument logging is capped in `main` so it never reaches the logs either.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::workspace::{Workspace, answer, caller, refuse_without_write};
use crate::routes::v1::satellites::{self, CreateSatelliteRequest, UpdateSatelliteRequest};

/// Names one satellite.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SatelliteArguments {
    /// The satellite's id, from `satellites_list`.
    id: Uuid,
}

/// Changes to one satellite.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSatelliteArguments {
    /// The satellite's id, from `satellites_list`.
    id: Uuid,
    /// The fields to change; those left out stay as they are.
    changes: UpdateSatelliteRequest,
}

#[tool_router(router = satellites_router, vis = "pub(super)")]
impl Workspace {
    #[tool(
        name = "satellites_list",
        description = "Lists the satellites coding sessions run on, with each one's latest \
            status: whether Elysium reaches it, its version, and how busy it is. Secrets are \
            never shown."
    )]
    async fn satellites_list(&self) -> Result<CallToolResult, ErrorData> {
        let satellites = satellites::list(&self.state).await;
        answer(satellites.map(|satellites| json!({ "satellites": satellites })))
    }

    #[tool(
        name = "satellites_get",
        description = "Shows one satellite and its latest status."
    )]
    async fn satellites_get(
        &self,
        Parameters(arguments): Parameters<SatelliteArguments>,
    ) -> Result<CallToolResult, ErrorData> {
        let satellite = satellites::find(&self.state, arguments.id).await;
        answer(satellite.map(|satellite| json!({ "satellite": satellite })))
    }

    #[tool(
        name = "satellites_create",
        description = "Registers an Arsox satellite by its URL and bearer secret, and starts \
            watching it. The secret is sealed and can never be read back. Needs write access."
    )]
    async fn satellites_create(
        &self,
        Parameters(request): Parameters<CreateSatelliteRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Some(refusal) = refuse_without_write(&caller) {
            return Ok(refusal);
        }
        let satellite = satellites::create(&self.state, caller.user.id, request).await;
        answer(satellite.map(|satellite| json!({ "satellite": satellite })))
    }

    #[tool(
        name = "satellites_update",
        description = "Changes a satellite's name, description, URL, secret, or whether it is \
            active; Elysium reconnects with the change. Deactivating one stops new sessions \
            on it. Needs write access."
    )]
    async fn satellites_update(
        &self,
        Parameters(arguments): Parameters<UpdateSatelliteArguments>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Some(refusal) = refuse_without_write(&caller) {
            return Ok(refusal);
        }
        let satellite = satellites::update(&self.state, arguments.id, arguments.changes).await;
        answer(satellite.map(|satellite| json!({ "satellite": satellite })))
    }
}
