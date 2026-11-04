// Copyright © 2026 Jalapeno Labs

//! The guidance every agent connected to Elysium reads as its session starts. See
//! `docs/knowledge.md`, Agent guidance.
//!
//! Two kinds of agent read it ([`Audience`]): MCP clients such as Claude Code and Codex, told when
//! they connect to `/api/mcp`, and every turn of Elysium's own agents, told when the turn connects
//! to its `elysium_knowledge` server. Each reads three parts, in this order ([`instructions`]):
//!
//! 1. **Its server's introduction**, compiled here: what the server is, and what its tools are
//!    for where that differs between the two. What a tool does belongs to the code, so it lives
//!    in the tool's description or here, never in the guidance.
//! 2. **The guidance**, the newest version admins wrote under Settings, Agent guidance
//!    (`crate::models::agent_guidance`): when to search, how to cite, what is worth saving as a
//!    learning, and whatever else the company asks of its agents.
//! 3. **The rules** ([`rules`]), compiled here and part of no version: that what the knowledge base
//!    holds stays in the company, and where a person opens a citation. Fixed text rather than a
//!    check on what admins write, since no check can tell that a reworded rule still says the
//!    same, and the link names this deployment's origin, which is not an admin's to type.
//!
//! The whole of it stays within [`INSTRUCTIONS_MAX_CHARACTERS`], so no client cuts the rules off
//! its end: a save past [`content_max_characters`] is refused.
//!
//! # Reading it on every session
//!
//! Both servers are stateless, so a client's session starts with an `initialize` (or, on newer
//! protocol versions, a `discover`) that is answered on its own, and each reads the current
//! version then: a change reaches every connected agent on its next session. [`AgentGuidance`]
//! keeps the newest version in memory, read from Postgres once, and every save and restore
//! replaces it as it commits. The API runs one replica (`docs/engine.md`, One replica of the API),
//! so the writes this process makes are every write there is.

use std::sync::{Arc, PoisonError, RwLock};

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use url::Url;
use uuid::Uuid;

use crate::database::Pool;
use crate::models::agent_guidance::{self, GuidanceVersion, NewVersion};
use crate::oauth::mcp_resource;

/// The most of a server's instructions Claude Code reads, in characters: it drops the rest
/// (`CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH`, 4,096 unless a person sets it). Every agent Elysium
/// serves runs Claude Code or a client like it, so the whole of what one reads fits here, and the
/// rules at its end are never cut. Raising it lets the guidance grow past what Claude Code reads.
pub const INSTRUCTIONS_MAX_CHARACTERS: usize = 4_096;

/// Between the parts of what an agent reads, so each reads as its own paragraph.
const PARAGRAPH_BREAK: &str = "\n\n";

/// A kind of agent that reads the guidance, each told first what its own server is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// An MCP client a person connected to `/api/mcp`, such as Claude Code or Codex.
    McpClients,
    /// A turn of one of Elysium's own agents, through its `elysium_knowledge` server: coding and
    /// Studio sessions, and Elysia.
    Turns,
}

impl Audience {
    /// Every kind of agent that reads the guidance.
    pub const ALL: [Self; 2] = [Self::McpClients, Self::Turns];

    /// What this kind of agent is told before the guidance: what its server is.
    const fn introduction(self) -> &'static str {
        match self {
            Self::McpClients => {
                "Elysium is a shared workspace of projects, action items, initiatives, and coding \
                 sessions, and the company's knowledge base. Every call acts as the person who \
                 connected this client, with what they granted it. A coding session is an agent \
                 working on a project's repositories in Elysium's workspace: start one with \
                 sessions_create, steer it with sessions_send_prompt, and follow its work with \
                 sessions_events."
            }
            // A turn has no learning_save yet (docs/knowledge.md, MCP), while the guidance is
            // written for every agent and may ask for one.
            Self::Turns => {
                "The company's knowledge base: what was said in mail, chat, meetings, and pages, \
                 what people and agents learned, and the company doc. This server has no \
                 learning_save: where the guidance below asks you to save a learning, say what \
                 you learned in your answer instead."
            }
        }
    }
}

/// What Elysium tells every agent after the guidance, which no edit removes: what the knowledge
/// base holds stays in the company, and where a person opens what an agent cites.
pub fn rules(public_url: &Url) -> String {
    let origin = public_url.origin().ascii_serialization();
    format!(
        "Results may quote a coworker's mail and direct messages: they are the company's to use \
         for its own work, and never for repeating outside the company or in work for another \
         client. A person opens any citation in Elysium at \
         {origin}/knowledge/documents/<documentId>."
    )
}

/// Everything `audience` reads as its session starts: its server's introduction, `guidance`, and
/// the [`rules`], each a paragraph. Guidance left empty adds nothing between the two.
pub fn instructions(audience: Audience, guidance: &str, public_url: &Url) -> String {
    let rules = rules(public_url);
    let parts = [audience.introduction(), guidance, &rules];
    let written: Vec<&str> = parts.into_iter().filter(|part| !part.is_empty()).collect();
    written.join(PARAGRAPH_BREAK)
}

/// The longest guidance this deployment takes, in characters: what is left of
/// [`INSTRUCTIONS_MAX_CHARACTERS`] once the longest introduction and the rules, which name this
/// deployment's origin, are counted.
pub fn content_max_characters(public_url: &Url) -> usize {
    Audience::ALL
        .into_iter()
        .fold(INSTRUCTIONS_MAX_CHARACTERS, |max, audience| {
            let fixed =
                instructions(audience, "", public_url).chars().count() + PARAGRAPH_BREAK.len();
            max.min(INSTRUCTIONS_MAX_CHARACTERS.saturating_sub(fixed))
        })
}

/// The block a team pastes into a repository's `CLAUDE.md`, so an agent working there reads the
/// guidance and the rules whether or not it is connected yet, and is told how to connect. It
/// names its version, so a team can tell when its copy is behind.
pub fn claude_md(version: &GuidanceVersion, public_url: &Url) -> String {
    let origin = public_url.origin().ascii_serialization();
    let mcp_url = mcp_resource(public_url);
    let mut block = format!(
        "<!-- Elysium's agent guidance, version {number}. Copy it again from \
         {origin}/settings/agent-guidance when it changes. -->\n\
         ## Elysium\n\n\
         The company's knowledge base is Elysium, at {origin}. Connect Claude Code to it with:\n\n\
         ```sh\n\
         claude mcp add --transport http elysium {mcp_url}\n\
         ```\n\n",
        number = version.version,
    );
    if !version.content.is_empty() {
        block.push_str(&version.content);
        block.push_str(PARAGRAPH_BREAK);
    }
    block.push_str(&rules(public_url));
    block.push('\n');
    block
}

/// Why a change to the guidance was refused.
#[derive(Debug, thiserror::Error)]
pub enum GuidanceError {
    /// Includes [`DieselError::NotFound`] for a version to restore that does not exist.
    #[error(transparent)]
    Database(#[from] DieselError),
    #[error(
        "the guidance is {length} characters, longer than the {max} that every agent reads whole"
    )]
    TooLong { length: usize, max: usize },
    /// Another version was saved since the one the change was made against.
    #[error("the guidance changed since this was opened")]
    Changed,
}

/// One version as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionResponse {
    version: i32,
    content: String,
    /// The version a restore copied the text of.
    restored_from: Option<i32>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<GuidanceVersion> for VersionResponse {
    fn from(version: GuidanceVersion) -> Self {
        Self {
            version: version.version,
            content: version.content,
            restored_from: version.restored_from,
            created_by: version.created_by,
            created_at: version.created_at,
        }
    }
}

/// The guidance as Settings, Agent guidance shows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentGuidanceResponse {
    /// The version every agent reads now.
    current: VersionResponse,
    /// Every version, newest first, the current one among them.
    versions: Vec<VersionResponse>,
    /// What Elysium tells every agent after the guidance.
    rules: String,
    /// The current version as a block for a repository's `CLAUDE.md`.
    claude_md: String,
    /// The longest guidance this deployment takes.
    max_characters: usize,
}

/// The current guidance, kept in memory for every session that starts, and every change to it.
/// Cheap to clone; clones share the version kept.
#[derive(Clone)]
pub struct AgentGuidance {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for AgentGuidance {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pool has nothing useful to print.
        formatter
            .debug_struct("AgentGuidance")
            .field("public_url", &self.inner.public_url.as_str())
            .field("kept_version", &self.kept().map(|kept| kept.version))
            .finish_non_exhaustive()
    }
}

struct Inner {
    database: Pool,
    public_url: Url,
    /// The newest version this process has read or written, once it has.
    current: RwLock<Option<Arc<GuidanceVersion>>>,
}

impl AgentGuidance {
    pub fn new(database: Pool, public_url: Url) -> Self {
        Self {
            inner: Arc::new(Inner {
                database,
                public_url,
                current: RwLock::new(None),
            }),
        }
    }

    /// Everything `audience` reads as its session starts, with the current version's guidance.
    ///
    /// # Errors
    /// Fails when the current version is not kept yet and Postgres cannot be read.
    pub async fn instructions(&self, audience: Audience) -> anyhow::Result<String> {
        let current = self.current().await?;
        Ok(instructions(
            audience,
            &current.content,
            &self.inner.public_url,
        ))
    }

    /// The version kept, or the current one read from Postgres and kept, the first time.
    async fn current(&self) -> anyhow::Result<Arc<GuidanceVersion>> {
        if let Some(kept) = self.kept() {
            return Ok(kept);
        }
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let current = agent_guidance::current(&mut connection)
            .await
            .context("the current agent guidance could not be read")?;
        Ok(self.remember(current))
    }

    /// Every version, the rules, and the `CLAUDE.md` block, as Settings shows them.
    ///
    /// # Errors
    /// Propagates any database error.
    pub async fn read(
        &self,
        connection: &mut AsyncPgConnection,
    ) -> Result<AgentGuidanceResponse, DieselError> {
        let versions = agent_guidance::versions(connection).await?;
        let Some(newest) = versions.first().cloned() else {
            // The migration seeds version 1 and nothing removes a version.
            return Err(DieselError::NotFound);
        };
        let public_url = &self.inner.public_url;
        let claude_md = claude_md(&newest, public_url);
        self.remember(newest.clone());
        Ok(AgentGuidanceResponse {
            current: newest.into(),
            versions: versions.into_iter().map(VersionResponse::from).collect(),
            rules: rules(public_url),
            claude_md,
            max_characters: content_max_characters(public_url),
        })
    }

    /// Saves `content`, trimmed, as the next version by `author`, made against `base_version`.
    /// Text the current version already holds records nothing and answers `None`.
    ///
    /// # Errors
    /// [`GuidanceError::TooLong`] past [`content_max_characters`]; [`GuidanceError::Changed`]
    /// when `base_version` is no longer the current one, or another save took the next number
    /// meanwhile; any other database error.
    pub async fn save(
        &self,
        connection: &mut AsyncPgConnection,
        content: &str,
        base_version: i32,
        author: Uuid,
    ) -> Result<Option<GuidanceVersion>, GuidanceError> {
        let content = content.trim();
        self.check_length(content)?;
        let current = agent_guidance::current(connection).await?;
        if current.version != base_version {
            return Err(GuidanceError::Changed);
        }
        if current.content == content {
            return Ok(None);
        }
        let new = NewVersion {
            version: current.version + 1,
            content,
            restored_from: None,
            created_by: author,
        };
        self.write(connection, &new).await.map(Some)
    }

    /// Makes `version`'s text current again, as the next version by `author`, naming the one it
    /// copied. A version whose text is already current records nothing and answers `None`.
    ///
    /// # Errors
    /// [`DieselError::NotFound`] for a version that does not exist; [`GuidanceError::TooLong`]
    /// for one longer than this deployment now takes; [`GuidanceError::Changed`] when another
    /// change took the next number meanwhile; any other database error.
    pub async fn restore(
        &self,
        connection: &mut AsyncPgConnection,
        version: i32,
        author: Uuid,
    ) -> Result<Option<GuidanceVersion>, GuidanceError> {
        let restored = agent_guidance::find(connection, version).await?;
        self.check_length(&restored.content)?;
        let current = agent_guidance::current(connection).await?;
        if current.content == restored.content {
            return Ok(None);
        }
        let new = NewVersion {
            version: current.version + 1,
            content: &restored.content,
            restored_from: Some(version),
            created_by: author,
        };
        self.write(connection, &new).await.map(Some)
    }

    fn check_length(&self, content: &str) -> Result<(), GuidanceError> {
        let length = content.chars().count();
        let max = content_max_characters(&self.inner.public_url);
        if length > max {
            return Err(GuidanceError::TooLong { length, max });
        }
        Ok(())
    }

    /// Adds `new` and keeps it as the version every next session reads.
    async fn write(
        &self,
        connection: &mut AsyncPgConnection,
        new: &NewVersion<'_>,
    ) -> Result<GuidanceVersion, GuidanceError> {
        let written =
            agent_guidance::insert(connection, new)
                .await
                .map_err(|error| match error {
                    DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                        GuidanceError::Changed
                    }
                    other => other.into(),
                })?;
        self.remember(written.clone());
        Ok(written)
    }

    /// The version kept, once one is.
    fn kept(&self) -> Option<Arc<GuidanceVersion>> {
        self.inner
            .current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Keeps `version` unless a newer one is kept already, and answers whichever is kept. Never
    /// going back is what makes a read that began before a save harmless: it finishes with the
    /// older version after the save kept the newer one, and is not kept.
    ///
    /// The poison is ignored: the value is replaced whole or not at all, so a panic elsewhere
    /// never leaves it half written.
    fn remember(&self, version: GuidanceVersion) -> Arc<GuidanceVersion> {
        let mut kept = self
            .inner
            .current
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(newer) = kept.as_ref().filter(|kept| kept.version >= version.version) {
            return Arc::clone(newer);
        }
        let version = Arc::new(version);
        *kept = Some(Arc::clone(&version));
        version
    }
}

#[cfg(test)]
mod tests;
