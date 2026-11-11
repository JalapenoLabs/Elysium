// Copyright © 2026 Jalapeno Labs

//! Tools for pages: reading the knowledge base's tree, editing it directly, every version kept, and
//! suggesting edits for a person to accept.
//!
//! Each calls the page routes' own function (`routes::v1::pages`), so a page changed here is
//! validated, versioned, indexed, and announced exactly as one changed in the browser. Reading is
//! for the person who connected the client; a change is by them with the client recorded as the
//! agent acting for them. Pages are part of the knowledge base, so a guest's client is refused, as
//! the routes refuse a guest.
//!
//! What these tools teach is shared with a turn's tools of the same names
//! (`crate::tools::knowledge`), so an agent reads pages the same way in a turn and over MCP.

use chrono::Utc;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use serde_json::json;

use super::bearer::McpCaller;
use super::learnings::acting_agent;
use super::workspace::{Workspace, answer, caller, refuse_without_write};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::{Agent, Author};
use crate::routes::v1::pages::{
    self, ListPagesArguments, MovePageArguments, PageArguments, PageLabelArguments,
    PageProjectArguments, SavePageRequest, SuggestPageRequest,
};
use crate::state::AppState;

/// What reading a page answers, shared with the agents' tool of the same name.
pub const READ_DESCRIPTION: &str = "Reads one page of the company's knowledge base whole: its \
    title, its text in Markdown, its parentId, its labels, the projectIds it is linked to, and \
    its currentVersion, which page_save and page_suggest need, with who published that version and \
    the agent acting for them, and the suggestions waiting for a person, each as the changes it \
    makes, so you need not suggest them again. Find an id with page_list, or as the pageId of a \
    citation that names one. proposalOnly means a company doc page, which takes suggestions alone, \
    and a category with evidence means a learning; page_save changes neither.";

/// What listing pages answers, shared with the agents' tool of the same name.
pub const LIST_DESCRIPTION: &str = "Lists the knowledge base's pages a branch at a time: the \
    pages at the top of the tree, or those directly under parentId, in order, each with its id, \
    title, currentVersion, labels, whether it is fixed in its place, and childCount, how many \
    pages sit directly under it. Each answer holds at most 200 pages and total says how many the \
    branch has: give offset for the rest. Use it to see how the tree is laid out and where a page \
    belongs. To find what a page says, search with knowledge_search and sources [\"page\"], and \
    to find the pages carrying a label, search with its label filter.";

/// How to write a page, shared with the agents' tool of the same name.
pub const SAVE_DESCRIPTION: &str = "Saves a page of the company's knowledge base: Markdown that \
    everyone in the company and their agents read, every version kept with who wrote it. To \
    change a page, read it with page_read, then give its id, its currentVersion as baseVersion, \
    and its title and body whole as they should now read: the body replaces what is there. If \
    the page changed since you read it the save is refused with page_changed: read it again, \
    make your change on what it says now, and save that, rather than your older text. To create \
    a page, leave id out and give parentId, the page it goes under from page_list, or null for \
    the top of the tree. Text the page already holds makes no version. A company doc page \
    refuses this with page_proposal_only: suggest the edit with page_suggest instead. A learning \
    refuses it with page_is_a_learning, and a learning is saved with learning_save. \
    To move a page use page_move, and to label one page_label_add. Needs write access.";

/// How to suggest an edit to a page, shared with the agents' tool of the same name.
pub const SUGGEST_DESCRIPTION: &str = "Suggests an edit to a page of the company's knowledge \
    base, for a person to accept or dismiss on the page; nothing changes until they accept it, \
    wholly or in part. Use it for a page agents may only suggest to (proposalOnly, such as the \
    company doc's), or when a person should decide. Read the page with page_read, then give its id, \
    its currentVersion as baseVersion, and edits against that version in the form Claude Code's \
    Edit tool takes: find is text the page holds exactly once, copied exactly, and replace takes \
    its place; an empty find adds replace at the end. A page published since is no reason to read \
    it again, since accepting merges the two. Give a reason, and the quote and source that show it \
    when you read it somewhere. An edit already waiting for review is answered rather than \
    suggested twice. A learning refuses it with page_is_a_learning. Needs write access.";

/// How to move a page, shared with the agents' tool of the same name.
pub const MOVE_DESCRIPTION: &str = "Moves a page of the knowledge base with everything under it, \
    or reorders it among its siblings. to.parentId is the page it goes under, or null for the \
    top of the tree, and to.after is the sibling it goes right after, or null to go first, both \
    from page_list and both required. A page already there changes nothing. Pages fixed in their \
    place refuse it with page_fixed. Needs write access.";

/// How to label a page, shared with the agents' tool of the same name.
pub const LABEL_ADD_DESCRIPTION: &str = "Puts a label on a page of the knowledge base. Labels cut \
    across the tree, so use one for what pages have in common wherever they sit, such as a team or \
    runbook, and reuse a label page_list shows rather than a new spelling of it. A label is no \
    part of what a page says: it makes no version, and any live page takes one, a company doc \
    page or a learning included. A label already there changes nothing, and a page carries at \
    most 50. Needs write access.";

/// How to link a page to a project, shared with the agents' tool of the same name.
pub const PROJECT_LINK_DESCRIPTION: &str = "Links a page of the knowledge base to a project, so \
    the project lists it among its docs and the project's Project Manager reads it. A page may be \
    linked to any number of projects. A link is no part of what a page says: it makes no version, \
    changes nobody's access, and any live page takes one, a company doc page or a learning \
    included. A link already there changes nothing. Needs write access.";

/// How to take a page off a project, shared with the agents' tool of the same name.
pub const PROJECT_UNLINK_DESCRIPTION: &str = "Takes a page of the knowledge base off a project's \
    docs. Like linking it makes no version, and the page and the project both stay. A link that \
    is not there changes nothing. Needs write access.";

/// How to take a label off a page, shared with the agents' tool of the same name.
pub const LABEL_REMOVE_DESCRIPTION: &str = "Takes a label off a page of the knowledge base. Like \
    adding one it makes no version, and any live page takes it. A label the page does not carry \
    changes nothing. Needs write access.";

#[tool_router(router = pages_router, vis = "pub(super)")]
impl Workspace {
    #[tool(name = "page_read", description = READ_DESCRIPTION)]
    async fn page_read(
        &self,
        Parameters(arguments): Parameters<PageArguments>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Err(refusal) = MemberUser::check(caller.user) {
            return answer(Err(refusal));
        }
        let read = pages::read_with_suggestions(&self.state.database, arguments.id).await;
        answer(read.map(|(page, suggestions)| json!({ "page": page, "suggestions": suggestions })))
    }

    #[tool(name = "page_list", description = LIST_DESCRIPTION)]
    async fn page_list(
        &self,
        Parameters(arguments): Parameters<ListPagesArguments>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Err(refusal) = MemberUser::check(caller.user) {
            return answer(Err(refusal));
        }
        let branch =
            pages::list_branch(&self.state.database, arguments.parent_id, arguments.offset).await;
        answer(branch.map(|branch| json!(branch)))
    }

    #[tool(name = "page_save", description = SAVE_DESCRIPTION)]
    async fn page_save(
        &self,
        Parameters(request): Parameters<SavePageRequest>,
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
        let agent = match recorded_agent(&self.state, &caller).await {
            Ok(agent) => agent,
            Err(refusal) => return answer(Err(refusal)),
        };
        let author = Author {
            user_id: member.id(),
            agent: Some(&agent),
            at: Utc::now(),
        };
        let saved = pages::save(&self.state.database, &self.state.events, author, request).await;
        answer(saved.map(|saved| json!(saved)))
    }

    #[tool(name = "page_suggest", description = SUGGEST_DESCRIPTION)]
    async fn page_suggest(
        &self,
        Parameters(request): Parameters<SuggestPageRequest>,
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
        // A suggestion changes nothing until a person accepts it, so a client that cannot be
        // recorded may still make one, as the person, as company_doc_propose always allowed.
        let agent = acting_agent(&self.state, &caller).await;
        let author = Author {
            user_id: member.id(),
            agent: agent.as_ref(),
            at: Utc::now(),
        };
        let suggested =
            pages::suggest(&self.state.database, &self.state.events, author, request).await;
        answer(suggested.map(|suggested| json!(suggested)))
    }

    #[tool(name = "page_move", description = MOVE_DESCRIPTION)]
    async fn page_move(
        &self,
        Parameters(arguments): Parameters<MovePageArguments>,
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
        let agent = match recorded_agent(&self.state, &caller).await {
            Ok(agent) => agent,
            Err(refusal) => return answer(Err(refusal)),
        };
        let author = Author {
            user_id: member.id(),
            agent: Some(&agent),
            at: Utc::now(),
        };
        let moved = pages::move_to(
            &self.state.database,
            &self.state.events,
            author,
            arguments.id,
            arguments.to,
        )
        .await;
        answer(moved.map(|page| json!({ "page": page })))
    }

    #[tool(name = "page_label_add", description = LABEL_ADD_DESCRIPTION)]
    async fn page_label_add(
        &self,
        Parameters(arguments): Parameters<PageLabelArguments>,
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
        let agent = match recorded_agent(&self.state, &caller).await {
            Ok(agent) => agent,
            Err(refusal) => return answer(Err(refusal)),
        };
        let author = Author {
            user_id: member.id(),
            agent: Some(&agent),
            at: Utc::now(),
        };
        let labeled = pages::add_label(
            &self.state.database,
            &self.state.events,
            author,
            arguments.id,
            &arguments.label,
        )
        .await;
        answer(labeled.map(|page| json!({ "page": page })))
    }

    #[tool(name = "page_label_remove", description = LABEL_REMOVE_DESCRIPTION)]
    async fn page_label_remove(
        &self,
        Parameters(arguments): Parameters<PageLabelArguments>,
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
        let agent = match recorded_agent(&self.state, &caller).await {
            Ok(agent) => agent,
            Err(refusal) => return answer(Err(refusal)),
        };
        let author = Author {
            user_id: member.id(),
            agent: Some(&agent),
            at: Utc::now(),
        };
        let unlabeled = pages::remove_label(
            &self.state.database,
            &self.state.events,
            author,
            arguments.id,
            &arguments.label,
        )
        .await;
        answer(unlabeled.map(|page| json!({ "page": page })))
    }

    #[tool(name = "page_project_link", description = PROJECT_LINK_DESCRIPTION)]
    async fn page_project_link(
        &self,
        Parameters(arguments): Parameters<PageProjectArguments>,
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
        let agent = match recorded_agent(&self.state, &caller).await {
            Ok(agent) => agent,
            Err(refusal) => return answer(Err(refusal)),
        };
        let author = Author {
            user_id: member.id(),
            agent: Some(&agent),
            at: Utc::now(),
        };
        let linked = pages::link_project(
            &self.state.database,
            &self.state.events,
            author,
            arguments.id,
            arguments.project_id,
        )
        .await;
        answer(linked.map(|page| json!({ "page": page })))
    }

    #[tool(name = "page_project_unlink", description = PROJECT_UNLINK_DESCRIPTION)]
    async fn page_project_unlink(
        &self,
        Parameters(arguments): Parameters<PageProjectArguments>,
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
        let agent = match recorded_agent(&self.state, &caller).await {
            Ok(agent) => agent,
            Err(refusal) => return answer(Err(refusal)),
        };
        let author = Author {
            user_id: member.id(),
            agent: Some(&agent),
            at: Utc::now(),
        };
        let unlinked = pages::unlink_project(
            &self.state.database,
            &self.state.events,
            author,
            arguments.id,
            arguments.project_id,
        )
        .await;
        answer(unlinked.map(|page| json!({ "page": page })))
    }
}

/// The agent a client's change to a page is recorded under.
///
/// A learning or a proposal can go without one, as it is only a label there. A page cannot: every
/// edit names its agent, and a change that named none would read as the person acting alone, who
/// may write the proposal-only pages agents may not.
///
/// # Errors
/// `403` for a client whose id the page's columns cannot hold, which only a client registered some
/// other way than through Hydra could have.
async fn recorded_agent(state: &AppState, caller: &McpCaller) -> Result<Agent, ApiError> {
    acting_agent(state, caller).await.ok_or_else(|| {
        ApiError::Forbidden(
            "this client's id cannot be recorded as the agent of a change, so it can only read \
             pages"
                .to_owned(),
        )
    })
}

#[cfg(test)]
mod tests;
