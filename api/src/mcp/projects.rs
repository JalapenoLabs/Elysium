// Copyright © 2026 Jalapeno Labs

//! Tools for projects. Reading them is what lets a client start a coding session, which names
//! its project.

use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use serde_json::json;

use super::workspace::{Workspace, answer};
use crate::routes::v1::projects;

#[tool_router(router = projects_router, vis = "pub(super)")]
impl Workspace {
    #[tool(
        name = "projects_list",
        description = "Lists the workspace's projects, alphabetically, with each one's id, \
            name, description, and how it picks the GitHub token its sessions start with."
    )]
    async fn projects_list(&self) -> Result<CallToolResult, ErrorData> {
        let projects = projects::list(&self.state).await;
        answer(projects.map(|projects| json!({ "projects": projects })))
    }
}
