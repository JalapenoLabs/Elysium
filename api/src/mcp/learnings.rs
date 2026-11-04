// Copyright © 2026 Jalapeno Labs

//! Tools for learnings: what an agent learned, saved for the next one.
//!
//! They call the learning routes' own functions (`routes::v1::learnings`), with the person who
//! connected the client as the one who saved it and the client as the agent acting for them, so
//! a learning saved here is validated, recorded in its history, indexed, and announced exactly
//! as one saved in the browser. Learnings are part of the knowledge base, so a guest's client is
//! refused, as the routes refuse a guest.
//!
//! The categories a learning is filed under are the workspace's to add to, so `learning_save`'s
//! schema cannot name them when it is compiled. [`describe_categories`] writes them into it each
//! time the tools are listed: every key, with the description that says what belongs there.

use std::sync::Arc;

use anyhow::Context;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::bearer::McpCaller;
use super::workspace::{Workspace, answer, caller, refuse_without_write};
use crate::auth::MemberUser;
use crate::models::learning::Agent;
use crate::models::learning_category::{self, LearningCategory};
use crate::routes::v1::learnings::{self, SaveLearningRequest};
use crate::state::AppState;

/// The tool whose schema [`describe_categories`] completes. The `#[tool]` attribute below takes
/// a literal, so the name is written there too.
const SAVE_TOOL: &str = "learning_save";

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

/// Writes every category into `learning_save`'s schema among `tools`, so an agent picks from
/// the workspace's list as it stands, knowing what each holds.
///
/// A list that cannot be read leaves the schema as compiled, which still names the field, and is
/// logged: listing the tools must not fail because of it, and a save naming no category there is
/// refused with the keys there are.
pub(super) async fn describe_categories(state: &AppState, tools: &mut [Tool]) {
    let Some(tool) = tools.iter_mut().find(|tool| tool.name == SAVE_TOOL) else {
        unreachable!("the learnings router lists {SAVE_TOOL}");
    };
    let categories = async {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        learning_category::list(&mut connection)
            .await
            .context("the learning categories could not be read")
    };
    let categories = match categories.await {
        Ok(categories) => categories,
        Err(error) => {
            event!(
                name: "mcp.learning_categories.unread",
                Level::WARN,
                error.message = %error,
                "could not read the learning categories; {SAVE_TOOL} lists them as compiled",
            );
            return;
        }
    };

    let mut schema = (*tool.input_schema).clone();
    let Some(Value::Object(category)) = schema
        .get_mut("properties")
        .and_then(|properties| properties.get_mut("category"))
    else {
        unreachable!("{SAVE_TOOL} takes a category");
    };
    category.insert("oneOf".to_owned(), category_choices(&categories));
    tool.input_schema = Arc::new(schema);
}

/// Each category as a choice in a JSON Schema `oneOf`: its key, what people call it, and what
/// belongs there, in the order they are offered.
fn category_choices(categories: &[LearningCategory]) -> Value {
    categories
        .iter()
        .map(|category| {
            json!({
                "const": category.key,
                "title": category.label,
                "description": category.description,
            })
        })
        .collect()
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

    fn category(key: &str, label: &str, description: &str, built_in: bool) -> LearningCategory {
        let now = chrono::Utc::now();
        LearningCategory {
            key: key.to_owned(),
            label: label.to_owned(),
            description: description.to_owned(),
            position: 0,
            built_in,
            created_by: Uuid::nil(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn the_save_tool_offers_every_category_with_what_it_holds() {
        let categories = [
            category(
                "pitfall",
                "Pitfall",
                "Something that behaves other than expected, and the way around it.",
                true,
            ),
            category(
                "eda_tools",
                "EDA tools",
                "Simulators, synthesis, and their licenses.",
                false,
            ),
        ];

        assert_eq!(
            category_choices(&categories),
            json!([
                {
                    "const": "pitfall",
                    "title": "Pitfall",
                    "description":
                        "Something that behaves other than expected, and the way around it.",
                },
                {
                    "const": "eda_tools",
                    "title": "EDA tools",
                    "description": "Simulators, synthesis, and their licenses.",
                },
            ])
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
    async fn listing_the_tools_offers_the_workspace_categories_as_they_stand() {
        use crate::models::learning_category::NewCategory;
        use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database};

        let (url, mut connection) = migrated_database().await;
        let state = app_state(&url).await;
        learning_category::create(
            &mut connection,
            NewCategory {
                key: "eda_tools".to_owned(),
                label: "EDA tools".to_owned(),
                description: "Simulators, synthesis, and their licenses.".to_owned(),
            },
            TEST_PERSON_ID,
        )
        .await
        .expect("added");

        let mut tools = Workspace::learnings_router().list_all();
        describe_categories(&state, &mut tools).await;

        let save = tools
            .iter()
            .find(|tool| tool.name == SAVE_TOOL)
            .expect("listed");
        let choices = save.input_schema["properties"]["category"]["oneOf"]
            .as_array()
            .expect("the categories are offered");
        assert_eq!(
            choices.len(),
            10,
            "the nine built-in ones and the workspace's own"
        );
        assert_eq!(choices[0]["const"], "decision");
        assert_eq!(
            choices[9],
            json!({
                "const": "eda_tools",
                "title": "EDA tools",
                "description": "Simulators, synthesis, and their licenses.",
            })
        );
    }

    #[test]
    fn the_compiled_schema_has_a_category_for_the_choices_to_fill() {
        let schema = serde_json::to_value(schemars::schema_for!(SaveLearningRequest))
            .expect("the schema serializes");
        assert!(
            schema["properties"]["category"].is_object(),
            "describe_categories fills properties.category: {schema}"
        );
    }
}
