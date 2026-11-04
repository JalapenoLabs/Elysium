// Copyright © 2026 Jalapeno Labs

//! Tools for reading the knowledge base: searching it, opening what a search cites, and listing
//! the meetings it holds. They return evidence, and the calling agent reasons with it.
//!
//! Each calls the knowledge route's own function (`routes::v1::knowledge`) with the person who
//! connected the client, so a client finds, opens, and lists exactly what that person would in
//! the browser, and its searches spend the same budget. The knowledge base is the company's, so
//! a guest's client is refused, as the routes refuse a guest; within it nothing is limited by
//! whose record a document came from (`docs/knowledge.md`, Who sees what).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use serde_json::json;

use super::workspace::{Workspace, answer, caller};
use crate::auth::MemberUser;
use crate::routes::v1::knowledge::{
    self, DocumentArguments, ListMeetingsRequest, SearchKnowledgeRequest,
};

/// How to search well, shared with the agents' tool of the same name (`crate::tools::knowledge`),
/// so an agent is taught the same in a turn and over MCP.
pub const SEARCH_DESCRIPTION: &str = "Searches the company's knowledge base: the mail, Slack and \
    Teams messages, Confluence pages, YouTrack issues, and meeting topics Elysium reads, the \
    learnings people and agents saved, and the company doc. Passages are ranked by their words \
    and their meaning together, so ask a question in a sentence, or give the exact words: a key, \
    name, or error message such as WAY-123 (keep its dash) is found by its words, a paraphrase by \
    its meaning, and \"quotes\" keep a phrase together. Filters only narrow: sources (such as \
    meeting, slack, outlook, learning, company_doc), project, person (whoever said or wrote it), \
    and from and to. Each result is a citation (documentId, title, source, the person it came \
    from, when, and its url outside Elysium if it has one) with up to three of its best \
    passages and their positions; open a document, or the passages around one, with \
    knowledge_get. When searchedByMeaning is false the query was matched by its words alone, so \
    try the words a message would use. Cite what you use by its title, who said it, and when. \
    Results may quote a coworker's mail and direct messages: use them for the company's work and \
    never repeat them outside the company.";

/// What opening a document answers, shared with the agents' tool of the same name.
pub const GET_DESCRIPTION: &str = "Opens one document of the knowledge base by a citation's \
    documentId: every passage in order, or the passages from fromPosition to toPosition, so \
    read around a passage a search found by its position. It also answers what the document was \
    made from (a timeline record, a learning by its learningId, or a company doc section), the \
    projects it is attributed to, and passageCount, how many passages it has in all. A meeting \
    is in the knowledge base as its topics, each a document: list them with meetings_list.";

/// What listing meetings answers, shared with the agents' tool of the same name.
pub const MEETINGS_DESCRIPTION: &str = "Lists the meetings the knowledge base holds, newest \
    first: each one's title, the source it was read from, when it started, whose meeting source \
    recorded it, and its topics in the order they were said, each a citation to open with \
    knowledge_get. Narrow by from and to (when it started), person, or project. To find what \
    was said about something, search with knowledge_search and sources [\"meeting\"] instead.";

#[tool_router(router = knowledge_router, vis = "pub(super)")]
impl Workspace {
    #[tool(name = "knowledge_search", description = SEARCH_DESCRIPTION)]
    async fn knowledge_search(
        &self,
        Parameters(request): Parameters<SearchKnowledgeRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        let member = match MemberUser::check(caller.user) {
            Ok(member) => member,
            Err(refusal) => return answer(Err(refusal)),
        };
        let found = knowledge::search(
            &self.state.knowledge,
            &self.state.database,
            member.id(),
            request,
        )
        .await;
        answer(found.map(|found| json!(found)))
    }

    #[tool(name = "knowledge_get", description = GET_DESCRIPTION)]
    async fn knowledge_get(
        &self,
        Parameters(arguments): Parameters<DocumentArguments>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Err(refusal) = MemberUser::check(caller.user) {
            return answer(Err(refusal));
        }
        let (id, span) = arguments.into_parts();
        let opened = knowledge::open(&self.state.database, id, &span).await;
        answer(opened.map(|document| json!({ "document": document })))
    }

    #[tool(name = "meetings_list", description = MEETINGS_DESCRIPTION)]
    async fn meetings_list(
        &self,
        Parameters(request): Parameters<ListMeetingsRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Err(refusal) = MemberUser::check(caller.user) {
            return answer(Err(refusal));
        }
        let listed = knowledge::list_meetings(&self.state.database, request).await;
        answer(listed.map(|meetings| json!({ "meetings": meetings })))
    }
}

#[cfg(test)]
mod tests;
