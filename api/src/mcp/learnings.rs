// Copyright © 2026 Jalapeno Labs

//! Tools for learnings: what an agent learned, saved for the next one.
//!
//! They call the learning routes' own functions (`routes::v1::learnings`), with the person who
//! connected the client as the one who saved it and the client as the agent acting for them, so
//! a learning saved here is validated, recorded in its history, indexed, and announced exactly
//! as one saved in the browser. Learnings are part of the knowledge base, so a guest's client is
//! refused, as the routes refuse a guest.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use tracing::{Level, event};
use uuid::Uuid;

use super::bearer::McpCaller;
use super::workspace::{Workspace, answer, caller, refuse_without_write};
use crate::auth::MemberUser;
use crate::models::learning::Agent;
use crate::routes::v1::learnings::{self, SaveLearningRequest};
use crate::state::AppState;

/// Names one learning.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningArguments {
    /// The learning's id, as `learning_save` answered it.
    id: Uuid,
}

#[tool_router(router = learnings_router, vis = "pub(super)")]
impl Workspace {
    #[tool(
        name = "learning_save",
        description = "Saves a learning to the company's knowledge base, where every member and \
            their agents can find it at once. Save what the next agent would otherwise have to \
            rediscover: a decision and why, a pitfall and the way around it, a procedure that \
            worked, where something lives, who owns what. Write it for that agent: the title is \
            the takeaway itself, the body leads with the rule, then when it applies and why, \
            and the evidence shows it is true. Saving a title that already exists revises that \
            learning, and every revision is kept, so correct a learning by saving its title \
            again. Needs write access."
    )]
    async fn learning_save(
        &self,
        Parameters(request): Parameters<SaveLearningRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        let member = match MemberUser::check(caller.user.clone()) {
            Ok(member) => member,
            Err(refusal) => return answer(Err(refusal)),
        };
        if let Some(refusal) = refuse_without_write(&caller) {
            return Ok(refusal);
        }
        let agent = acting_agent(&self.state, &caller).await;
        let saved = learnings::save(&self.state, &member, agent.as_ref(), request).await;
        answer(saved.map(|saved| json!(saved)))
    }

    #[tool(
        name = "learning_delete",
        description = "Deletes a learning that is wrong or no longer true, taking it out of \
            the knowledge base. It stays in its history and anyone can restore it. Prefer \
            saving a correction when the learning is only partly wrong. Needs write access."
    )]
    async fn learning_delete(
        &self,
        Parameters(arguments): Parameters<LearningArguments>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        let member = match MemberUser::check(caller.user.clone()) {
            Ok(member) => member,
            Err(refusal) => return answer(Err(refusal)),
        };
        if let Some(refusal) = refuse_without_write(&caller) {
            return Ok(refusal);
        }
        let agent = acting_agent(&self.state, &caller).await;
        let deleted = learnings::delete(&self.state, &member, agent.as_ref(), arguments.id).await;
        answer(deleted.map(|()| json!({ "deleted": arguments.id })))
    }
}

/// The client acting for the caller, named as it registered with Hydra.
///
/// The name is only a label, so a client Hydra cannot describe right now is still recorded, by
/// its id alone, rather than refusing the person's change.
async fn acting_agent(state: &AppState, caller: &McpCaller) -> Option<Agent> {
    let name = match state.auth.hydra.client(&caller.client_id).await {
        Ok(client) => client.client_name,
        Err(error) => {
            event!(
                name: "mcp.agent.unnamed",
                Level::WARN,
                error.message = %error,
                oauth.client.id = %caller.client_id,
                "could not read an MCP client's name; recording it by id alone",
            );
            String::new()
        }
    };
    Agent::mcp_client(&caller.client_id, &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_learning_is_named_by_its_id_alone() {
        let id = Uuid::now_v7();
        let arguments: LearningArguments =
            serde_json::from_value(json!({ "id": id })).expect("parses");
        assert_eq!(arguments.id, id);

        serde_json::from_value::<LearningArguments>(json!({ "id": id, "title": "A" }))
            .expect_err("a stray field is refused");
    }

    #[test]
    fn the_save_tool_tells_the_agent_what_each_category_holds() {
        let schema = serde_json::to_value(schemars::schema_for!(SaveLearningRequest))
            .expect("the schema serializes");
        let text = schema.to_string();
        for category in ["decision", "pitfall", "procedure", "incident"] {
            assert!(text.contains(category), "{category} is offered");
        }
        assert!(
            text.contains("the way around it"),
            "each category carries its description: {text}"
        );
    }
}
