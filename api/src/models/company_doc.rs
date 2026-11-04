// Copyright © 2026 Jalapeno Labs

//! The company doc: its fixed sections, the changes proposed to them, its history, and its place
//! in the knowledge index. See `docs/knowledge.md`, The company doc.
//!
//! One document per instance knows the company, in seven fixed [`Section`]s of Markdown. Two
//! kinds of change reach it:
//!
//! - **A person edits it** ([`edit`]), any number of sections at once, as one entry in its
//!   history.
//! - **Anyone else proposes** ([`propose`]): the scheduled review, Elysia, a coding agent, or a
//!   person's MCP client. A proposal is one section's new text with the reason for it, and
//!   nothing changes until a person approves it ([`approve`]) or rejects it ([`reject`]). A
//!   proposal records the section as it read when it was made, so approving one after the
//!   section changed merges the two changes three ways, as git merges a branch, and refuses one
//!   whose changes overlap rather than undoing someone's edit.
//!
//! Every change that writes the doc records a history entry in the same transaction, shaped as
//! learnings' history is: `data.changes` holds each section it changed, `{ from, to }`, so
//! [`undo`] can put back what any one change wrote, section by section, keeping a section
//! changed again since. The same transaction indexes each changed section as its own document
//! (`crate::models::knowledge_document`), so search finds what the doc says the moment it
//! commits.
//!
//! The review's position on the timeline is kept here too ([`review_cursor`],
//! [`advance_review_cursor`]); what it reads and how it proposes is `crate::company_doc::review`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::database::schema::{
    company_doc_events, company_doc_proposals, company_doc_review_cursor, company_doc_sections,
};
use crate::knowledge::passages;
use crate::models::agent::Agent;
use crate::models::knowledge_document::{self, IndexedDocument, KnowledgeSource};
use crate::models::user::{CODING_AGENT_USER_ID, ELYSIA_USER_ID};

/// The longest a section may be, in characters, as `company_doc_sections.content` allows. Seven
/// of them fit the review's one prompt beside what it reads (`crate::company_doc::review`).
pub const CONTENT_MAX_CHARACTERS: usize = 12_000;

/// One of the doc's fixed sections, in the order the doc is read.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    /// Who the company is: what it does, for whom, and where.
    Company,
    /// Who leads it, and what each of them owns.
    Leadership,
    /// Everyone who comes up, with how their name is spelled and what they do.
    People,
    /// Clients, partners, investors, and vendors, and who speaks for each.
    Stakeholders,
    /// What the company makes and sells.
    Products,
    /// The work under way, and who is on it.
    Projects,
    /// Names, acronyms, and jargon, spelled as the company spells them.
    Glossary,
}

impl Section {
    /// Every section, in the order the doc is read.
    pub const ALL: [Self; 7] = [
        Self::Company,
        Self::Leadership,
        Self::People,
        Self::Stakeholders,
        Self::Products,
        Self::Projects,
        Self::Glossary,
    ];

    /// The key stored in `section` and sent to clients.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Company => "company",
            Self::Leadership => "leadership",
            Self::People => "people",
            Self::Stakeholders => "stakeholders",
            Self::Products => "products",
            Self::Projects => "projects",
            Self::Glossary => "glossary",
        }
    }

    /// The section stored as `key`, if it is one.
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|section| section.as_str() == key)
    }

    /// Its heading, as the index titles its document and prompts name it. English, since only
    /// people's own words are translated in the web app, and it names the section by its key.
    pub const fn heading(self) -> &'static str {
        match self {
            Self::Company => "Company",
            Self::Leadership => "Leadership",
            Self::People => "People",
            Self::Stakeholders => "Stakeholders",
            Self::Products => "Products",
            Self::Projects => "Projects",
            Self::Glossary => "Glossary",
        }
    }

    /// What belongs there, for the agents that propose to it.
    pub const fn purpose(self) -> &'static str {
        match self {
            Self::Company => "Who the company is: what it does, for whom, and where.",
            Self::Leadership => "Who leads the company, and what each of them owns.",
            Self::People => {
                "Everyone who comes up, one per line: their name exactly as it is spelled, how \
                 it is often misheard or misspelled, and what they do."
            }
            Self::Stakeholders => {
                "Clients, partners, investors, and vendors, one per line, with who speaks for \
                 each."
            }
            Self::Products => "What the company makes and sells, one per line.",
            Self::Projects => "The work under way, one per line, with who is on it.",
            Self::Glossary => {
                "Names, acronyms, and jargon, one per line in alphabetical order: the term as \
                 the company spells it, what it means, and how it is often misheard."
            }
        }
    }
}

/// A section as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocSection {
    pub section: Section,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = company_doc_sections, check_for_backend(diesel::pg::Pg))]
struct SectionRow {
    section: String,
    content: String,
    updated_at: DateTime<Utc>,
}

impl SectionRow {
    fn into_section(self) -> DocSection {
        let section = Section::parse(&self.section)
            .expect("the column's check holds only the sections this build knows");
        DocSection {
            section,
            content: self.content,
            updated_at: self.updated_at,
        }
    }
}

/// Where a proposal stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalState {
    Pending,
    Approved,
    Rejected,
}

impl ProposalState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// A stored proposal.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = company_doc_proposals, check_for_backend(diesel::pg::Pg))]
pub struct Proposal {
    pub id: Uuid,
    /// A [`Section`] key.
    pub section: String,
    /// The section as it read when the change was proposed.
    pub base_content: String,
    /// The section as the change would leave it.
    pub content: String,
    pub reason: String,
    pub quote: Option<String>,
    pub source: Option<String>,
    pub timeline_record_id: Option<Uuid>,
    pub workflow_run_id: Option<Uuid>,
    /// A [`ProposalState`] name.
    pub state: String,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
    /// `mcp:<client id>` or `session:<number>`; `None` for Elysia and the review.
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = company_doc_proposals)]
struct ProposalRow<'a> {
    id: Uuid,
    section: &'static str,
    base_content: &'a str,
    content: &'a str,
    reason: &'a str,
    quote: Option<&'a str>,
    source: Option<&'a str>,
    timeline_record_id: Option<Uuid>,
    workflow_run_id: Option<Uuid>,
    created_by: Uuid,
    agent: Option<String>,
    agent_name: Option<&'a str>,
    created_at: DateTime<Utc>,
}

/// Who proposes a change.
#[derive(Debug, Clone, Copy)]
pub enum Proposer<'a> {
    /// A person, through the MCP client acting for them when one is named.
    Person {
        user_id: Uuid,
        agent: Option<&'a Agent>,
    },
    /// The agent of the coding or Studio session with this number.
    Session(i64),
    /// Elysia: one of her turns, or a run of the scheduled review, which she carries out.
    Elysia { workflow_run_id: Option<Uuid> },
}

impl Proposer<'_> {
    /// Who the proposal is created by: the person, or the agent's machine user.
    const fn user_id(self) -> Uuid {
        match self {
            Self::Person { user_id, .. } => user_id,
            Self::Session(_) => CODING_AGENT_USER_ID,
            Self::Elysia { .. } => ELYSIA_USER_ID,
        }
    }

    /// The agent acting for them, in its recorded form.
    fn agent(self) -> Option<String> {
        match self {
            Self::Person { agent, .. } => agent.map(|agent| agent.id().to_owned()),
            Self::Session(number) => Some(format!("session:{number}")),
            Self::Elysia { .. } => None,
        }
    }

    fn agent_name(&self) -> Option<&str> {
        match self {
            Self::Person { agent, .. } => agent.and_then(Agent::name),
            Self::Session(_) | Self::Elysia { .. } => None,
        }
    }

    const fn workflow_run_id(self) -> Option<Uuid> {
        match self {
            Self::Elysia { workflow_run_id } => workflow_run_id,
            Self::Person { .. } | Self::Session(_) => None,
        }
    }
}

/// One change to a section, in the form Claude Code's own Edit tool takes: `find` is text the
/// section holds exactly once, and `replace` takes its place. An empty `find` adds `replace` at
/// the end of the section, on a line of its own.
///
/// Agents propose edits rather than a section's whole text, so an agent that misreads a long
/// section cannot drop what it did not mean to change.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Edit {
    /// Text the section holds exactly once, to be replaced; empty to add to the end.
    pub find: String,
    /// What takes its place; empty to remove it.
    pub replace: String,
}

/// A change to propose.
#[derive(Debug, Clone)]
pub struct NewProposal {
    pub section: Section,
    /// Applied in order to the section as it stands.
    pub edits: Vec<Edit>,
    pub reason: String,
    pub quote: Option<String>,
    pub source: Option<String>,
    /// The timeline record it was read in, for the review's proposals.
    pub timeline_record_id: Option<Uuid>,
}

/// A proposal, and whether proposing it stored it or found the same change already pending.
#[derive(Debug, Clone)]
pub struct Proposed {
    pub proposal: Proposal,
    pub created: bool,
}

/// What a history entry records, and so how its `data` is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// A person's own change, or an undo. `data.changes` maps each section to `{ from, to }`,
    /// and `data.undid` names the entry an undo reversed.
    Edited,
    /// A proposal applied, named by the entry's `proposal_id`. `data.changes` as for an edit.
    Approved,
}

impl HistoryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Edited => "edited",
            Self::Approved => "approved",
        }
    }
}

/// A stored history entry.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = company_doc_events, check_for_backend(diesel::pg::Pg))]
pub struct HistoryEntry {
    pub id: Uuid,
    /// A [`HistoryKind`] name, kept as text so entries a later version writes still load.
    pub kind: String,
    /// Who made the change.
    pub created_by: Uuid,
    pub proposal_id: Option<Uuid>,
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = company_doc_events)]
struct HistoryRow {
    id: Uuid,
    kind: &'static str,
    created_by: Uuid,
    proposal_id: Option<Uuid>,
    data: Value,
    created_at: DateTime<Utc>,
}

/// Who makes a change to the doc itself, and when: always a person, since everyone else
/// proposes.
#[derive(Debug, Clone, Copy)]
pub struct Author {
    pub user_id: Uuid,
    pub at: DateTime<Utc>,
}

/// Why a change to the doc was refused.
#[derive(Debug, thiserror::Error)]
pub enum CompanyDocError {
    /// Includes [`DieselError::NotFound`] for an unknown proposal or history entry.
    #[error(transparent)]
    Database(#[from] DieselError),
    /// A proposal out of bounds, such as one with no edits; the message says which bound.
    #[error("{0}")]
    Invalid(String),
    #[error("the {} section would be longer than {CONTENT_MAX_CHARACTERS} characters", .0.as_str())]
    TooLong(Section),
    /// An edit's `find` is not in the section, or is there more than once; the message says
    /// which, for the agent to read the section again.
    #[error("{0}")]
    EditDoesNotApply(String),
    #[error("this change leaves the {} section as it is", .0.as_str())]
    ChangesNothing(Section),
    #[error("this proposal was already {0}")]
    AlreadyDecided(&'static str),
    /// The section changed since the proposal was made, in the same lines it changes.
    #[error(
        "the {} section changed since this was proposed, in the same lines; reject it, or make \
         the change by hand",
        .0.as_str()
    )]
    Overlaps(Section),
    /// The change an undo names no longer shows in the doc; the message says why.
    #[error("{0}")]
    NothingToUndo(&'static str),
}

/// Every section, in order.
///
/// # Errors
/// Propagates any database error.
pub async fn sections(connection: &mut AsyncPgConnection) -> QueryResult<Vec<DocSection>> {
    let rows: Vec<SectionRow> = company_doc_sections::table
        .select(SectionRow::as_select())
        .load(connection)
        .await?;
    let mut sections: Vec<DocSection> = rows.into_iter().map(SectionRow::into_section).collect();
    sections.sort_by_key(|section| section.section);
    Ok(sections)
}

/// Proposals in `state`, oldest first: every one, or only the newest `newest` of them.
///
/// # Errors
/// Propagates any database error.
pub async fn proposals(
    connection: &mut AsyncPgConnection,
    state: ProposalState,
    newest: Option<i64>,
) -> QueryResult<Vec<Proposal>> {
    let query = company_doc_proposals::table
        .filter(company_doc_proposals::state.eq(state.as_str()))
        .select(Proposal::as_select());
    let Some(limit) = newest else {
        return query
            .order(company_doc_proposals::id.asc())
            .load(connection)
            .await;
    };
    let mut newest: Vec<Proposal> = query
        .order(company_doc_proposals::id.desc())
        .limit(limit)
        .load(connection)
        .await?;
    newest.reverse();
    Ok(newest)
}

/// The doc's history, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn history(connection: &mut AsyncPgConnection) -> QueryResult<Vec<HistoryEntry>> {
    company_doc_events::table
        .order(company_doc_events::id.asc())
        .select(HistoryEntry::as_select())
        .load(connection)
        .await
}

/// Writes each of `changes` over its section, as one change by `author`. Sections given their
/// own text again are left alone, and a change that leaves every section as it was records
/// nothing.
///
/// # Errors
/// [`CompanyDocError::TooLong`] for a section past [`CONTENT_MAX_CHARACTERS`]; any database
/// error.
pub async fn edit(
    connection: &mut AsyncPgConnection,
    changes: BTreeMap<Section, String>,
    author: Author,
) -> Result<Option<HistoryEntry>, CompanyDocError> {
    for (section, content) in &changes {
        if content.chars().count() > CONTENT_MAX_CHARACTERS {
            return Err(CompanyDocError::TooLong(*section));
        }
    }
    connection
        .transaction(async move |connection| {
            Ok(write_sections(connection, changes, author, Written::Edit { undid: None }).await?)
        })
        .await
}

/// Proposes `new` for a person to review: its edits applied to the section as it stands. The
/// same change already pending is answered rather than stored again.
///
/// # Errors
/// [`CompanyDocError::EditDoesNotApply`] for an edit whose text the section does not hold once,
/// [`CompanyDocError::ChangesNothing`] for edits that leave it as it is, and
/// [`CompanyDocError::TooLong`] for edits that make it too long; any database error.
pub async fn propose(
    connection: &mut AsyncPgConnection,
    new: NewProposal,
    proposer: Proposer<'_>,
    at: DateTime<Utc>,
) -> Result<Proposed, CompanyDocError> {
    connection
        .transaction(async move |connection| {
            let current = lock_section(connection, new.section).await?;
            let content = apply_edits(&current.content, &new.edits)?;
            if content == current.content {
                return Err(CompanyDocError::ChangesNothing(new.section));
            }
            if content.chars().count() > CONTENT_MAX_CHARACTERS {
                return Err(CompanyDocError::TooLong(new.section));
            }

            let inserted = diesel::insert_into(company_doc_proposals::table)
                .values(ProposalRow {
                    id: Uuid::now_v7(),
                    section: new.section.as_str(),
                    base_content: &current.content,
                    content: &content,
                    reason: &new.reason,
                    quote: new.quote.as_deref(),
                    source: new.source.as_deref(),
                    timeline_record_id: new.timeline_record_id,
                    workflow_run_id: proposer.workflow_run_id(),
                    created_by: proposer.user_id(),
                    agent: proposer.agent(),
                    agent_name: proposer.agent_name(),
                    created_at: at,
                })
                .on_conflict_do_nothing()
                .returning(Proposal::as_returning())
                .get_result(connection)
                .await
                .optional()?;
            if let Some(proposal) = inserted {
                return Ok(Proposed {
                    proposal,
                    created: true,
                });
            }
            // `company_doc_proposals_pending_once` refused it: the same text is pending.
            let pending = company_doc_proposals::table
                .filter(company_doc_proposals::section.eq(new.section.as_str()))
                .filter(company_doc_proposals::state.eq(ProposalState::Pending.as_str()))
                .filter(company_doc_proposals::content.eq(&content))
                .select(Proposal::as_select())
                .first(connection)
                .await?;
            Ok(Proposed {
                proposal: pending,
                created: false,
            })
        })
        .await
}

/// Approves proposal `id` and writes it into its section, as a change by the person approving.
///
/// A section that changed since the proposal was made gets both changes, merged three ways from
/// the text the proposal was made against; one whose changes overlap the proposal's is refused
/// and the proposal stays pending. A proposal whose change the section already holds is
/// approved without writing anything.
///
/// # Errors
/// `NotFound` for an unknown proposal, [`CompanyDocError::AlreadyDecided`],
/// [`CompanyDocError::Overlaps`], and [`CompanyDocError::TooLong`] for a merge past the limit;
/// any database error.
pub async fn approve(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author,
) -> Result<Proposal, CompanyDocError> {
    connection
        .transaction(async move |connection| {
            let proposal = lock_pending(connection, id).await?;
            let section = Section::parse(&proposal.section)
                .expect("the column references a section this build knows");
            let current = lock_section(connection, section).await?;
            let content = merged(&proposal.base_content, &current.content, &proposal.content)
                .ok_or(CompanyDocError::Overlaps(section))?;
            if content.chars().count() > CONTENT_MAX_CHARACTERS {
                return Err(CompanyDocError::TooLong(section));
            }

            let proposal = decide(connection, id, ProposalState::Approved, author).await?;
            write_sections(
                connection,
                BTreeMap::from([(section, content)]),
                author,
                Written::Approval { proposal_id: id },
            )
            .await?;
            Ok(proposal)
        })
        .await
}

/// Rejects proposal `id`. The doc is left as it is, so nothing is recorded in its history; the
/// proposal keeps who rejected it and when, and the review reads it so it does not propose the
/// same change again.
///
/// # Errors
/// `NotFound` for an unknown proposal, [`CompanyDocError::AlreadyDecided`]; any database error.
pub async fn reject(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author,
) -> Result<Proposal, CompanyDocError> {
    connection
        .transaction(async move |connection| {
            lock_pending(connection, id).await?;
            Ok(decide(connection, id, ProposalState::Rejected, author).await?)
        })
        .await
}

/// Reverses history entry `entry_id`, recording the reversal as an edit of its own that names
/// the entry, so it can be undone in turn.
///
/// Each section the entry changed that still holds what it wrote gets back what it replaced; a
/// section changed again since keeps its newer text, as undoing a learning's revision does
/// (`crate::models::learning::undo`).
///
/// # Errors
/// `NotFound` for an unknown entry, and [`CompanyDocError::NothingToUndo`] when every section
/// it wrote has changed since or its record cannot be read; any database error.
pub async fn undo(
    connection: &mut AsyncPgConnection,
    entry_id: Uuid,
    author: Author,
) -> Result<Option<HistoryEntry>, CompanyDocError> {
    connection
        .transaction(async move |connection| {
            let entry: HistoryEntry = company_doc_events::table
                .find(entry_id)
                .select(HistoryEntry::as_select())
                .first(connection)
                .await?;
            let written = written_changes(&entry.data).ok_or(CompanyDocError::NothingToUndo(
                "this change's record cannot be read",
            ))?;
            let mut current = BTreeMap::new();
            for section in written.keys() {
                let locked = lock_section(connection, *section).await?;
                current.insert(*section, locked.content);
            }
            let reverted = reverted_sections(&current, &written);
            if reverted.is_empty() {
                return Err(CompanyDocError::NothingToUndo(
                    "every section this change wrote has changed again since",
                ));
            }
            Ok(write_sections(
                connection,
                reverted,
                author,
                Written::Edit {
                    undid: Some(entry.id),
                },
            )
            .await?)
        })
        .await
}

/// How a call to [`write_sections`] is recorded.
#[derive(Debug, Clone, Copy)]
enum Written {
    Edit { undid: Option<Uuid> },
    Approval { proposal_id: Uuid },
}

/// Writes `changes` and records them as one entry, reindexing each section changed; answers
/// `None`, recording nothing, when every section already held its text.
async fn write_sections(
    connection: &mut AsyncPgConnection,
    changes: BTreeMap<Section, String>,
    author: Author,
    written: Written,
) -> QueryResult<Option<HistoryEntry>> {
    let mut recorded = Map::new();
    for (section, content) in changes {
        let current = lock_section(connection, section).await?;
        if current.content == content {
            continue;
        }
        let updated: SectionRow =
            diesel::update(company_doc_sections::table.find(section.as_str()))
                .set(company_doc_sections::content.eq(&content))
                .returning(SectionRow::as_returning())
                .get_result(connection)
                .await?;
        reindex(connection, &updated.into_section(), author).await?;
        recorded.insert(
            section.as_str().to_owned(),
            json!({ "from": current.content, "to": content }),
        );
    }
    if recorded.is_empty() {
        return Ok(None);
    }

    let mut data = json!({ "changes": recorded });
    let (kind, proposal_id) = match written {
        Written::Edit { undid } => {
            if let Some(undid) = undid {
                data["undid"] = json!(undid);
            }
            (HistoryKind::Edited, None)
        }
        Written::Approval { proposal_id } => (HistoryKind::Approved, Some(proposal_id)),
    };
    let entry = diesel::insert_into(company_doc_events::table)
        .values(HistoryRow {
            id: Uuid::now_v7(),
            kind: kind.as_str(),
            created_by: author.user_id,
            proposal_id,
            data,
            created_at: author.at,
        })
        .returning(HistoryEntry::as_returning())
        .get_result(connection)
        .await?;
    Ok(Some(entry))
}

/// Puts `section` in the index as it now reads, or takes it out while it is empty. Attributed to
/// the person whose change it now reads as.
async fn reindex(
    connection: &mut AsyncPgConnection,
    section: &DocSection,
    author: Author,
) -> QueryResult<()> {
    let source = KnowledgeSource::CompanyDocSection(section.section);
    let passages = passages::split(&section.content);
    if passages.is_empty() {
        return knowledge_document::remove(connection, source).await;
    }
    let document = IndexedDocument {
        source,
        title: format!("Company doc: {}", section.section.heading()),
        // The doc lives in Elysium, so a citation opens it there.
        url: None,
        occurred_at: section.updated_at,
        created_by: author.user_id,
        passages,
    };
    knowledge_document::upsert(connection, &document)
        .await
        .map(drop)
}

async fn lock_section(
    connection: &mut AsyncPgConnection,
    section: Section,
) -> QueryResult<DocSection> {
    company_doc_sections::table
        .find(section.as_str())
        .for_update()
        .select(SectionRow::as_select())
        .first(connection)
        .await
        .map(SectionRow::into_section)
}

/// Locks proposal `id`, refusing one already decided.
async fn lock_pending(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> Result<Proposal, CompanyDocError> {
    let proposal: Proposal = company_doc_proposals::table
        .find(id)
        .for_update()
        .select(Proposal::as_select())
        .first(connection)
        .await?;
    match proposal.state.as_str() {
        "pending" => Ok(proposal),
        "approved" => Err(CompanyDocError::AlreadyDecided("approved")),
        _ => Err(CompanyDocError::AlreadyDecided("rejected")),
    }
}

async fn decide(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    state: ProposalState,
    author: Author,
) -> QueryResult<Proposal> {
    diesel::update(company_doc_proposals::table.find(id))
        .set((
            company_doc_proposals::state.eq(state.as_str()),
            company_doc_proposals::decided_by.eq(author.user_id),
            company_doc_proposals::decided_at.eq(author.at),
        ))
        .returning(Proposal::as_returning())
        .get_result(connection)
        .await
}

/// `current` with `edits` applied in order. Each edit's `find` must be in the text exactly once
/// as it stands after the edits before it, so an edit changes only what its author saw; an empty
/// `find` adds `replace` as a line of its own at the end. The result is trimmed, as every
/// section is stored.
///
/// # Errors
/// [`CompanyDocError::EditDoesNotApply`] naming the edit, by its place from 1, whose text is
/// missing or found more than once.
pub fn apply_edits(current: &str, edits: &[Edit]) -> Result<String, CompanyDocError> {
    let mut text = current.to_owned();
    for (index, edit) in edits.iter().enumerate() {
        let number = index + 1;
        if edit.find.is_empty() {
            let addition = edit.replace.trim();
            if !addition.is_empty() {
                if !text.trim().is_empty() {
                    text.push('\n');
                }
                text.push_str(addition);
            }
            continue;
        }
        match text.match_indices(&edit.find).count() {
            1 => text = text.replacen(&edit.find, &edit.replace, 1),
            0 => {
                return Err(CompanyDocError::EditDoesNotApply(format!(
                    "edit {number}'s find text is not in the section; read the doc again and \
                     copy the text exactly"
                )));
            }
            _ => {
                return Err(CompanyDocError::EditDoesNotApply(format!(
                    "edit {number}'s find text is in the section more than once; include more of \
                     the text around it"
                )));
            }
        }
    }
    Ok(text.trim().to_owned())
}

/// The section with both changes: `current` (what it holds now) and `proposed`, each made from
/// `base`, merged line by line. `None` when the two change the same lines differently.
fn merged(base: &str, current: &str, proposed: &str) -> Option<String> {
    if current == base {
        return Some(proposed.to_owned());
    }
    // diffy merges whole lines, so each text ends with its last line's newline; the result is
    // trimmed again, as every section is stored.
    let with_newline = |text: &str| format!("{text}\n");
    diffy::merge(
        &with_newline(base),
        &with_newline(current),
        &with_newline(proposed),
    )
    .ok()
    .map(|merged| merged.trim().to_owned())
}

/// Each section an entry's `data.changes` wrote, as `(from, to)`; `None` when it cannot be read.
fn written_changes(data: &Value) -> Option<BTreeMap<Section, (String, String)>> {
    let changes = data.get("changes")?.as_object()?;
    let mut written = BTreeMap::new();
    for (key, change) in changes {
        let section = Section::parse(key)?;
        let from = change.get("from")?.as_str()?.to_owned();
        let to = change.get("to")?.as_str()?.to_owned();
        written.insert(section, (from, to));
    }
    Some(written)
}

/// The sections of `current` that still hold what `written` wrote, each with what it replaced.
fn reverted_sections(
    current: &BTreeMap<Section, String>,
    written: &BTreeMap<Section, (String, String)>,
) -> BTreeMap<Section, String> {
    written
        .iter()
        .filter(|(section, (_, to))| current.get(*section) == Some(to))
        .map(|(section, (from, _))| (*section, from.clone()))
        .collect()
}

/// How far the review has read the timeline: the ingestion instant and id of the last record it
/// read, or `None` before its first review.
///
/// # Errors
/// Propagates any database error.
pub async fn review_cursor(
    connection: &mut AsyncPgConnection,
) -> QueryResult<Option<(DateTime<Utc>, Uuid)>> {
    let (at, id): (Option<DateTime<Utc>>, Option<Uuid>) = company_doc_review_cursor::table
        .select((
            company_doc_review_cursor::reviewed_through_at,
            company_doc_review_cursor::reviewed_through_id,
        ))
        .first(connection)
        .await?;
    Ok(at.zip(id))
}

/// Moves the review's position from `from` to `to`, unless another review moved it first, which
/// answers `false` and moves nothing. Part of the caller's transaction, which stores what the
/// review proposed from the records up to `to`, so both happen or neither does.
///
/// # Errors
/// Propagates any database error.
pub async fn advance_review_cursor(
    connection: &mut AsyncPgConnection,
    from: Option<(DateTime<Utc>, Uuid)>,
    to: (DateTime<Utc>, Uuid),
) -> QueryResult<bool> {
    let current = company_doc_review_cursor::table
        .select((
            company_doc_review_cursor::reviewed_through_at,
            company_doc_review_cursor::reviewed_through_id,
        ))
        .for_update()
        .first::<(Option<DateTime<Utc>>, Option<Uuid>)>(connection)
        .await?;
    if current.0.zip(current.1) != from {
        return Ok(false);
    }
    diesel::update(company_doc_review_cursor::table)
        .set((
            company_doc_review_cursor::reviewed_through_at.eq(to.0),
            company_doc_review_cursor::reviewed_through_id.eq(to.1),
        ))
        .execute(connection)
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(find: &str, replace: &str) -> Edit {
        Edit {
            find: find.to_owned(),
            replace: replace.to_owned(),
        }
    }

    #[test]
    fn every_section_round_trips_through_its_key() {
        for section in Section::ALL {
            assert_eq!(Section::parse(section.as_str()), Some(section));
            assert_eq!(
                serde_json::to_value(section).expect("serializes"),
                json!(section.as_str())
            );
        }
        assert_eq!(Section::parse("Glossary"), None, "keys are lowercase");
    }

    #[test]
    fn edits_replace_text_found_once_and_add_to_the_end_when_find_is_empty() {
        let people = "- Priya Raman, head of operations\n- Tom Okafor, sales";

        let renamed = apply_edits(people, &[edit("Tom Okafor, sales", "Tomás Okafor, sales")])
            .expect("applies");
        assert_eq!(
            renamed,
            "- Priya Raman, head of operations\n- Tomás Okafor, sales"
        );

        let added = apply_edits(people, &[edit("", "- Lena Brandt, finance\n")]).expect("adds");
        assert_eq!(added, format!("{people}\n- Lena Brandt, finance"));

        let first_entry = apply_edits("", &[edit("", "- Lena Brandt")]).expect("adds");
        assert_eq!(
            first_entry, "- Lena Brandt",
            "an empty section starts with no blank line"
        );
    }

    #[test]
    fn an_edit_whose_text_is_missing_or_repeated_is_refused_naming_it() {
        let glossary = "- ACME: a client\n- ACME: a client";
        let repeated = apply_edits(glossary, &[edit("ACME: a client", "ACME Corp: a client")])
            .expect_err("ambiguous");
        assert!(
            repeated.to_string().contains("more than once"),
            "{repeated}"
        );

        let missing = apply_edits("- ACME", &[edit("", "- Beta"), edit("Gamma", "Delta")])
            .expect_err("missing");
        assert!(missing.to_string().starts_with("edit 2's"), "{missing}");
    }

    #[test]
    fn approving_after_the_section_changed_merges_both_changes() {
        let base = "- Alpha\n- Delta\n- Kilo\n- Tango";
        // Someone added Bravo since; the proposal adds Sierra further down.
        let current = "- Alpha\n- Bravo\n- Delta\n- Kilo\n- Tango";
        let proposed = "- Alpha\n- Delta\n- Kilo\n- Sierra\n- Tango";

        assert_eq!(
            merged(base, current, proposed).as_deref(),
            Some("- Alpha\n- Bravo\n- Delta\n- Kilo\n- Sierra\n- Tango")
        );
        assert_eq!(
            merged(base, base, proposed).as_deref(),
            Some(proposed),
            "an unchanged section takes the proposal as it is"
        );
    }

    #[test]
    fn approving_a_change_to_lines_changed_since_is_refused() {
        let base = "- Priya Ramen, operations";
        let current = "- Priya Raman, operations";
        let proposed = "- Priya Rahman, operations";

        assert_eq!(merged(base, current, proposed), None);
    }

    #[test]
    fn undoing_puts_back_only_the_sections_that_still_hold_what_the_change_wrote() {
        let data = json!({
            "changes": {
                "people": { "from": "- Priya", "to": "- Priya Raman" },
                "glossary": { "from": "", "to": "- ACME" },
            },
        });
        let written = written_changes(&data).expect("reads");
        // Since the change, someone edited the glossary again.
        let current = BTreeMap::from([
            (Section::People, "- Priya Raman".to_owned()),
            (Section::Glossary, "- ACME\n- Beta".to_owned()),
        ]);

        let reverted = reverted_sections(&current, &written);

        assert_eq!(
            reverted,
            BTreeMap::from([(Section::People, "- Priya".to_owned())])
        );
    }

    #[test]
    fn an_unreadable_change_is_refused_rather_than_guessed_at() {
        for data in [
            json!({}),
            json!({ "changes": { "people": { "from": "a" } } }),
            json!({ "changes": { "history": { "from": "a", "to": "b" } } }),
        ] {
            assert!(written_changes(&data).is_none(), "{data} is refused");
        }
    }
}
