// Copyright © 2026 Jalapeno Labs

//! Tools for the company doc: the one document that knows the company, which agents read and
//! propose changes to.
//!
//! `company_doc_read` answers what `GET /api/v1/company-doc` answers: every section, each with
//! the proposals still waiting for review. `company_doc_propose` calls `crate::company_doc::propose`,
//! as the review and agents' own tool do, with the person who connected the client as the one
//! proposing and the client as the agent acting for them. Nothing it proposes changes the doc
//! until a person approves it. The doc is part of the knowledge base, so a guest's client is
//! refused, as the routes refuse a guest.

use anyhow::Context;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use serde_json::json;

use super::learnings::acting_agent;
use super::workspace::{Workspace, answer, caller, refuse_without_write};
use crate::auth::MemberUser;
use crate::company_doc::{self, ProposeRequest};
use crate::errors::ApiError;
use crate::models::company_doc::Proposer;

#[tool_router(router = company_doc_router, vis = "pub(super)")]
impl Workspace {
    #[tool(
        name = "company_doc_read",
        description = "Reads the company doc: who the company is, its leadership, its people and \
            how their names are spelled, stakeholders, products, projects, and a glossary of \
            names and jargon. Each section lists the proposed changes still waiting for a \
            person's review, so you know what may change and need not propose it again. Read it \
            before writing anything that names the company's people, clients, or terms."
    )]
    async fn company_doc_read(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Err(refusal) = MemberUser::check(caller.user) {
            return answer(Err(refusal));
        }
        let read = async {
            let mut connection = self
                .state
                .database
                .get()
                .await
                .context("no database connection available")?;
            Ok::<_, ApiError>(json!(company_doc::read(&mut connection).await?))
        };
        answer(read.await)
    }

    #[tool(
        name = "company_doc_propose",
        description = "Proposes a change to one section of the company doc, for a person to \
            approve or reject; nothing changes until they approve it. Propose what you learned \
            for certain: a person's name as they spell it themselves, a new client or product, \
            a term and what it means, a changed role. Read the doc first and give edits against \
            a section as it reads now, in the form Claude Code's Edit tool takes. A change \
            already waiting for review is answered rather than proposed twice. Needs write \
            access."
    )]
    async fn company_doc_propose(
        &self,
        Parameters(request): Parameters<ProposeRequest>,
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
        let proposer = Proposer::Person {
            user_id: member.id(),
            agent: agent.as_ref(),
        };
        let proposed = async {
            let mut connection = self
                .state
                .database
                .get()
                .await
                .context("no database connection available")?;
            let proposed =
                company_doc::propose(&mut connection, &self.state.events, request, proposer)
                    .await?;
            Ok::<_, ApiError>(json!(proposed))
        };
        answer(proposed.await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_tools_are_listed_under_the_names_the_knowledge_base_gives_them() {
        let names: Vec<String> = Workspace::company_doc_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"company_doc_read".to_owned()));
        assert!(names.contains(&"company_doc_propose".to_owned()));
    }

    #[test]
    fn the_propose_tool_takes_sections_by_their_keys_and_edits_as_find_and_replace() {
        let schema = serde_json::to_value(schemars::schema_for!(ProposeRequest))
            .expect("the schema serializes");
        let schema = schema.to_string();
        for key in ["glossary", "people", "find", "replace", "reason"] {
            assert!(schema.contains(key), "{key} is described: {schema}");
        }
    }
}
