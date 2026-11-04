// Copyright © 2026 Jalapeno Labs

//! The company doc as the routes, the MCP server, agents' tools, and the review share it. See
//! `docs/knowledge.md`, The company doc.
//!
//! The doc's records and their rules are `crate::models::company_doc`. This module is what every
//! caller sees of them: the doc as it is answered, with each section's pending proposals beside
//! it ([`read`]), and proposing a change ([`propose`]), so a proposal made by an agent over MCP,
//! by a coding agent's tool, or by the scheduled review is checked, stored, and announced the
//! same way. The review itself, a workflow on the engine, is [`workflow`].

pub mod review;
pub mod workflow;

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::models::company_doc::{
    self, CompanyDocError, DocSection, Edit, HistoryEntry, NewProposal, Proposal, ProposalState,
    Proposer, Section,
};
use crate::realtime::{EventBus, ServerEvent};

/// The most edits one proposal holds: a section's worth of corrections, each small enough to
/// review.
pub const EDITS_MAX: usize = 20;

/// The longest reason, quote, and source, as `company_doc_proposals` allows.
pub const REASON_MAX_CHARACTERS: usize = 2_000;
pub const QUOTE_MAX_CHARACTERS: usize = 2_000;
pub const SOURCE_MAX_CHARACTERS: usize = 500;

/// The doc as clients and agents see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanyDocResponse {
    /// Every section, in the order the doc is read.
    pub sections: Vec<SectionResponse>,
}

/// One section, with the changes proposed to it that wait for a person.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionResponse {
    pub section: Section,
    /// What belongs there, for the agents that read and propose to it.
    pub purpose: &'static str,
    /// Markdown.
    pub content: String,
    pub updated_at: DateTime<Utc>,
    /// Pending proposals to this section, oldest first.
    pub proposals: Vec<ProposalResponse>,
}

/// The agent that acted for whoever proposed a change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    /// Its recorded form: `mcp:<client id>` or `session:<number>`.
    pub id: String,
    /// What an MCP client called itself, if anything.
    pub name: Option<String>,
}

/// A proposal as clients and agents see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalResponse {
    pub id: Uuid,
    pub section: String,
    /// The section as it read when the change was proposed, and as the change would leave it.
    pub base_content: String,
    pub content: String,
    pub reason: String,
    pub quote: Option<String>,
    pub source: Option<String>,
    /// The timeline record the review read it in, while that record exists.
    pub timeline_record_id: Option<Uuid>,
    /// The review's run that proposed it, while that run exists.
    pub workflow_run_id: Option<Uuid>,
    /// `pending`, `approved`, or `rejected`.
    pub state: String,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    /// Who proposed it: a person, or the machine user of Elysia or the coding agent.
    pub created_by: Uuid,
    pub agent: Option<AgentResponse>,
    pub created_at: DateTime<Utc>,
}

impl From<Proposal> for ProposalResponse {
    fn from(proposal: Proposal) -> Self {
        Self {
            id: proposal.id,
            section: proposal.section,
            base_content: proposal.base_content,
            content: proposal.content,
            reason: proposal.reason,
            quote: proposal.quote,
            source: proposal.source,
            timeline_record_id: proposal.timeline_record_id,
            workflow_run_id: proposal.workflow_run_id,
            state: proposal.state,
            decided_by: proposal.decided_by,
            decided_at: proposal.decided_at,
            created_by: proposal.created_by,
            agent: proposal.agent.map(|id| AgentResponse {
                id,
                name: proposal.agent_name,
            }),
            created_at: proposal.created_at,
        }
    }
}

/// A history entry as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryResponse {
    pub id: Uuid,
    /// `edited` or `approved`.
    pub kind: String,
    /// Who made the change.
    pub created_by: Uuid,
    /// The proposal an `approved` entry applied.
    pub proposal_id: Option<Uuid>,
    /// `changes`: each section changed, `{ from, to }`; `undid` when an undo wrote it.
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

impl From<HistoryEntry> for HistoryEntryResponse {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            id: entry.id,
            kind: entry.kind,
            created_by: entry.created_by,
            proposal_id: entry.proposal_id,
            data: entry.data,
            created_at: entry.created_at,
        }
    }
}

/// The doc as it stands, each section with its pending proposals.
///
/// # Errors
/// Propagates any database error.
pub async fn read(connection: &mut AsyncPgConnection) -> QueryResult<CompanyDocResponse> {
    let sections = company_doc::sections(connection).await?;
    let pending = company_doc::proposals(connection, ProposalState::Pending, None).await?;
    Ok(respond(sections, pending))
}

/// `sections` as answered, each with the proposals of `pending` made to it.
pub fn respond(sections: Vec<DocSection>, pending: Vec<Proposal>) -> CompanyDocResponse {
    let mut by_section: HashMap<String, Vec<ProposalResponse>> = HashMap::new();
    for proposal in pending {
        by_section
            .entry(proposal.section.clone())
            .or_default()
            .push(ProposalResponse::from(proposal));
    }
    let sections = sections
        .into_iter()
        .map(|section| SectionResponse {
            purpose: section.section.purpose(),
            proposals: by_section
                .remove(section.section.as_str())
                .unwrap_or_default(),
            section: section.section,
            content: section.content,
            updated_at: section.updated_at,
        })
        .collect();
    CompanyDocResponse { sections }
}

/// A change to propose to one section of the company doc, for a person to approve or reject.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposeRequest {
    /// The section to change, by its key.
    pub section: Section,
    /// The changes, applied in order to the section as it reads now, as Claude Code's Edit
    /// tool takes them: `find` is text the section holds exactly once, copied exactly, and
    /// `replace` takes its place. An empty `find` adds `replace` at the end of the section. At
    /// most 20.
    pub edits: Vec<Edit>,
    /// Why the doc should say this, in a sentence or two the person reviewing it reads first.
    pub reason: String,
    /// The words that show it, quoted exactly, such as how a person signs their own name.
    #[serde(default)]
    pub quote: Option<String>,
    /// Where the quote is from, such as a meeting's title or a channel and its date.
    #[serde(default)]
    pub source: Option<String>,
}

/// A proposal, and whether proposing it stored it or found the same change already pending.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedResponse {
    pub proposal: ProposalResponse,
    /// `false` when the same change was already waiting for review.
    pub created: bool,
    /// What happens next, for the agent that proposed it.
    pub message: &'static str,
}

/// Proposes `request` as `proposer`, announcing it when it is new.
///
/// # Errors
/// [`CompanyDocError::Invalid`] for a request out of bounds, and every refusal of
/// [`company_doc::propose`].
pub async fn propose(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    request: ProposeRequest,
    proposer: Proposer<'_>,
) -> Result<ProposedResponse, CompanyDocError> {
    let new = checked(request)?;
    let stored = company_doc::propose(connection, new, proposer, Utc::now()).await?;
    if stored.created {
        events.publish(&ServerEvent::CompanyDocChanged);
    }
    Ok(ProposedResponse {
        proposal: ProposalResponse::from(stored.proposal),
        created: stored.created,
        message: "Nothing in the company doc changes until a person approves this proposal.",
    })
}

/// `request` trimmed and bounded, ready to store.
fn checked(request: ProposeRequest) -> Result<NewProposal, CompanyDocError> {
    let invalid = |message: &str| Err(CompanyDocError::Invalid(message.to_owned()));
    if request.edits.is_empty() {
        return invalid("a proposal holds at least one edit");
    }
    if request.edits.len() > EDITS_MAX {
        return invalid("a proposal holds at most 20 edits; propose the rest separately");
    }
    let reason = request.reason.trim().to_owned();
    if reason.is_empty() || reason.chars().count() > REASON_MAX_CHARACTERS {
        return invalid("the reason is 1 to 2,000 characters");
    }
    let optional = |text: Option<String>, max: usize, field: &str| {
        let text = text.map(|text| text.trim().to_owned());
        match text {
            Some(text) if text.chars().count() > max => Err(CompanyDocError::Invalid(format!(
                "the {field} is at most {max} characters"
            ))),
            Some(text) if text.is_empty() => Ok(None),
            other => Ok(other),
        }
    };
    Ok(NewProposal {
        section: request.section,
        edits: request.edits,
        reason,
        quote: optional(request.quote, QUOTE_MAX_CHARACTERS, "quote")?,
        source: optional(request.source, SOURCE_MAX_CHARACTERS, "source")?,
        timeline_record_id: None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn request(value: Value) -> ProposeRequest {
        serde_json::from_value(value).expect("parses")
    }

    #[test]
    fn a_request_is_trimmed_and_blank_optional_fields_are_dropped() {
        let new = checked(request(json!({
            "section": "glossary",
            "edits": [{ "find": "", "replace": "- ACME: our first client" }],
            "reason": "  Said in the kickoff.  ",
            "quote": " ",
            "source": " Kickoff, November 3rd ",
        })))
        .expect("checks");

        assert_eq!(new.section, Section::Glossary);
        assert_eq!(new.reason, "Said in the kickoff.");
        assert_eq!(new.quote, None);
        assert_eq!(new.source.as_deref(), Some("Kickoff, November 3rd"));
    }

    #[test]
    fn a_request_out_of_bounds_is_refused_saying_why() {
        let no_edits = checked(request(json!({
            "section": "people", "edits": [], "reason": "Why",
        })))
        .expect_err("refused");
        assert!(
            no_edits.to_string().contains("at least one edit"),
            "{no_edits}"
        );

        let blank_reason = checked(request(json!({
            "section": "people",
            "edits": [{ "find": "", "replace": "- Priya" }],
            "reason": "   ",
        })))
        .expect_err("refused");
        assert!(
            blank_reason.to_string().contains("reason"),
            "{blank_reason}"
        );

        serde_json::from_value::<ProposeRequest>(json!({
            "section": "history",
            "edits": [{ "find": "", "replace": "x" }],
            "reason": "Why",
        }))
        .expect_err("only the doc's sections can be named");
    }
}
