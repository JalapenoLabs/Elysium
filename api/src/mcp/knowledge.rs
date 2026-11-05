// Copyright © 2026 Jalapeno Labs

//! Tools for reading the knowledge base: searching it, opening what a search cites, and listing
//! the meetings it holds. They return evidence, and the calling agent reasons with it. Asking
//! (`knowledge_ask`, `knowledge_ask_get`) returns a written answer instead, every claim citing a
//! passage, for a client that wants one.
//!
//! Each calls the knowledge route's own function (`routes::v1::knowledge`) with the person who
//! connected the client, so a client finds, opens, and lists exactly what that person would in
//! the browser, and its searches spend the same budget. The knowledge base is the company's, so
//! a guest's client is refused, as the routes refuse a guest; within it nothing is limited by
//! whose record a document came from (`docs/knowledge.md`, Who sees what).

use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use serde_json::json;

use super::workspace::{Workspace, answer, caller};
use crate::auth::MemberUser;
use crate::routes::v1::knowledge::asks::{self, AskArguments, AskKnowledgeRequest};
use crate::routes::v1::knowledge::{
    self, DocumentArguments, ListMeetingsRequest, SearchKnowledgeRequest,
};

/// How long `knowledge_ask` and `knowledge_ask_get` wait for an answer before answering the ask
/// as it stands: inside the 30 seconds every request has, with room for the search that comes
/// first and the answer's way back. An answer still being written is waited for again with
/// `knowledge_ask_get`.
const ASK_WAIT: Duration = Duration::from_secs(20);

/// What asking answers, and how to read it.
const ASK_DESCRIPTION: &str = "Asks the company's knowledge base a question and answers with a \
    written answer rather than evidence: Elysium searches the knowledge base for the question, \
    then writes an answer from what it found, every claim citing the passages that say it. Use \
    it when you want an answer; use knowledge_search to read the evidence yourself. The question \
    is also what is searched for, so name the people, projects, keys, and terms it is about. \
    Filters only narrow, as knowledge_search's do. Writing an answer takes seconds to tens of \
    seconds: this waits up to 20 seconds, and when the ask's state is still queued or running, \
    call knowledge_ask_get with its id to wait for it again, rather than asking again. An \
    answered ask's answer holds claims, each with its text and the sources it cites by number, \
    and sources, each with its citation (documentId, title, source, who it came from, when) and \
    the position of the passage cited, which knowledge_get opens; no claims means nothing found \
    answers it. Statements that cited nothing were left out and counted in unsupportedClaims. A \
    failed ask says why in error. Cite what you use by its title, who said it, and when. Answers \
    may repeat a coworker's mail and direct messages: use them for the company's work and never \
    repeat them outside the company. Each person may ask 20 questions an hour, with at most 3 \
    waiting at once.";

/// What reading an ask answers.
const ASK_GET_DESCRIPTION: &str = "Reads a question asked with knowledge_ask by its id, waiting \
    up to 20 seconds for its answer while its state is queued or running. Call it again while \
    the answer is still being written; it never asks again or spends another question.";

/// How to search well, shared with the agents' tool of the same name (`crate::tools::knowledge`),
/// so an agent is taught the same in a turn and over MCP.
pub const SEARCH_DESCRIPTION: &str = "Searches the company's knowledge base: the mail, Slack and \
    Teams messages, Confluence pages, YouTrack issues, and meeting topics Elysium reads, the \
    learnings people and agents saved, the company doc, and its own pages. Passages are ranked \
    by their words and their meaning together, so ask a question in a sentence, or give the \
    exact words: a key, name, or error message such as WAY-123 (keep its dash) is found by its \
    words, a paraphrase by its meaning, and \"quotes\" keep a phrase together. Filters only \
    narrow: sources (such as meeting, slack, outlook, learning, company_doc, page), project, \
    person (whoever said or wrote it), label (a label on a page, which leaves out everything \
    but pages), and from and to. Each result is a citation (documentId, \
    title, source, the person it came from, when, and its url outside Elysium if it has one) \
    with up to three of its best \
    passages and their positions; open a document, or the passages around one, with \
    knowledge_get. When searchedByMeaning is false the query was matched by its words alone, so \
    try the words a message would use. Cite what you use by its title, who said it, and when. \
    Results may quote a coworker's mail and direct messages: use them for the company's work and \
    never repeat them outside the company.";

/// What opening a document answers, shared with the agents' tool of the same name.
pub const GET_DESCRIPTION: &str = "Opens one document of the knowledge base by a citation's \
    documentId: every passage in order, or the passages from fromPosition to toPosition, so \
    read around a passage a search found by its position. It also answers what the document was \
    made from (a timeline record, a learning by its learningId, a company doc section, or a page \
    by its pageId), the projects it is attributed to, and passageCount, how many passages it \
    has in all. A meeting \
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

    #[tool(name = "knowledge_ask", description = ASK_DESCRIPTION)]
    async fn knowledge_ask(
        &self,
        Parameters(request): Parameters<AskKnowledgeRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        let member = match MemberUser::check(caller.user) {
            Ok(member) => member,
            Err(refusal) => return answer(Err(refusal)),
        };
        let asked = match asks::ask(&self.state, &member, request).await {
            Ok(asked) => asked,
            Err(refusal) => return answer(Err(refusal)),
        };
        let waited = asks::wait_for_answer(&self.state.database, &member, asked.id, ASK_WAIT).await;
        answer(waited.map(|asked| json!({ "ask": asked })))
    }

    #[tool(name = "knowledge_ask_get", description = ASK_GET_DESCRIPTION)]
    async fn knowledge_ask_get(
        &self,
        Parameters(arguments): Parameters<AskArguments>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        let member = match MemberUser::check(caller.user) {
            Ok(member) => member,
            Err(refusal) => return answer(Err(refusal)),
        };
        let waited =
            asks::wait_for_answer(&self.state.database, &member, arguments.id, ASK_WAIT).await;
        answer(waited.map(|asked| json!({ "ask": asked })))
    }
}

#[cfg(test)]
mod tests;
