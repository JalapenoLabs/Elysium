// Copyright © 2026 Jalapeno Labs

//! Why a batch exists, in the description of every pull request its worker opens: the same
//! chain its page shows (`crate::models::work_batch::why`), from the meeting quote or issue that
//! asked for it to the item and the batch, each a link. See `docs/engine.md`, Why a batch exists.
//!
//! The worker never writes it. Recording a pull request (`worker_pull_request`) reads the
//! description GitHub holds, puts the chain in it ([`section`], [`splice`]), and writes it back
//! ([`write`]); recording it again puts back a chain an edit removed. The chain sits between two
//! HTML comments, each alone on its line ([`START`], [`END`]), at the end of what the worker
//! wrote, which it never changes, and it replaces its earlier self in place.
//!
//! # What it may say
//!
//! A pull request's description is read by whoever reads its repository. In a private repository
//! that is the company, so the chain carries what its page does: titles, a meeting's quote, who
//! wrote a message. In a public one it is anyone, and nothing ingested or written inside Elysium
//! leaves the company (`docs/knowledge.md`, Who sees what), so the chain carries only what kind
//! each link is, each opening in Elysium, which answers its members alone ([`Audience`]). Not even
//! an issue's key or address: a key names a private repository or the company's tracker, and an
//! address is the tracker's.
//!
//! # Why it cannot break a pull request
//!
//! - Editing a description leaves the head commit and its checks as they are, so a merge decision
//!   (`crate::worker_pool::merging::decide`) reads nothing new; only a workflow its repository runs
//!   on an edited description runs again.
//! - Every text the chain quotes is a link's text, with Markdown's punctuation escaped, so nothing
//!   quoted becomes Markdown, HTML, a mention, an issue reference, or a closing keyword that
//!   would close an issue as it merges: GitHub reads none of those inside a link.
//! - It is bounded ([`SECTION_MAX_CHARACTERS`]), and a description it would carry past GitHub's
//!   limit is left as it is ([`DescriptionError::TooLong`]).

use std::fmt::Write as _;

use secrecy::SecretString;
use url::Url;

use crate::github::issues::IssueRef;
use crate::github::pulls::PullRequest;
use crate::github::{Github, GithubError};
use crate::models::work_batch::why::{Link, Why};

/// The line the chain starts on.
pub const START: &str = "<!-- elysium:why -->";

/// The line the chain ends on.
pub const END: &str = "<!-- /elysium:why -->";

/// The longest description GitHub stores, in characters.
pub const GITHUB_MAX_CHARACTERS: usize = 65_536;

/// The longest the chain grows, in characters: room for a few dozen links, and a small part of
/// GitHub's limit, so a description the worker wrote at length still has the chain added.
pub const SECTION_MAX_CHARACTERS: usize = 8_000;

/// The most of a title the chain quotes.
const TITLE_MAX_CHARACTERS: usize = 200;

/// The most of a quote, such as a meeting's words, the chain carries.
const QUOTE_MAX_CHARACTERS: usize = 300;

/// What a link reads as in a public repository, where it may say nothing of what it opens.
const OPEN_IN_ELYSIUM: &str = "Open in Elysium";

/// What a segment of a private meeting reads as, wherever the chain is written.
const PRIVATE_MEETING: &str = "A private meeting";

/// The web app's paths, as `frontend/src/urls.ts` spells them.
const KNOWLEDGE_DOCUMENT_PATH: &str = "/knowledge/documents/";
const ACTION_ITEM_PATH: &str = "/action-items/";
const WORK_BATCH_PATH: &str = "/work-queue/";
const PROJECT_PATH: &str = "/projects/";

/// The project page's tabs a message and a finding open on, as `frontend/src/urls.ts` names them.
const MANAGER_TAB: &str = "manager";
const FINDINGS_TAB: &str = "findings";

/// What a record is called by where it came from, as the web app's timeline names it
/// (`frontend/src/locales/en-US/timeline.json`, `sources`).
const SOURCE_NAMES: [(&str, &str); 9] = [
    ("meeting", "Meeting"),
    ("outlook", "Outlook"),
    ("gmail", "Gmail"),
    ("teams", "Teams"),
    ("slack", "Slack"),
    ("mail", "Mail"),
    ("project_manager", "Project Manager"),
    ("turn_summary", "Agent turn"),
    ("merged_pull_request", "Merged pull request"),
];

/// Who reads a pull request's description, which decides what the chain may say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// Those its private repository is shared with: the company.
    Company,
    /// Anyone, in a public repository.
    Anyone,
}

impl Audience {
    /// Who reads `pull_request`'s description.
    pub const fn of(pull_request: &PullRequest) -> Self {
        if pull_request.repository_is_private {
            return Self::Company;
        }
        Self::Anyone
    }
}

/// Why a description was not written.
#[derive(Debug, thiserror::Error)]
pub enum DescriptionError {
    /// With the chain the description would pass GitHub's limit.
    #[error(
        "the description is too long to hold why its batch exists within GitHub's \
         {GITHUB_MAX_CHARACTERS} characters"
    )]
    TooLong,
    /// The project's token can no longer see the pull request.
    #[error("the project's GitHub token can no longer see the pull request")]
    NotFound,
    #[error(transparent)]
    Github(#[from] GithubError),
}

/// Whether writing a description changed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// The chain was added, or brought up to date.
    Changed,
    /// The description already carried this chain.
    Unchanged,
}

/// Puts a batch's chain, `why`, in the description of the pull request `reference` names, through
/// `token`, linking into Elysium at `public_url`.
///
/// The pull request is read here, immediately before the description is composed from it, so
/// what the worker or a person wrote since it was recorded is kept, and the audience follows the
/// repository's visibility as it is now. GitHub offers no conditional write of a description, so
/// an edit landing between this read and the write is the one it can lose; the window is one
/// round trip, and recording the pull request again puts the chain back.
///
/// # Errors
/// [`DescriptionError::NotFound`] when the token can no longer see it;
/// [`DescriptionError::TooLong`] when the description cannot hold the chain; GitHub refusing the
/// read or the write.
pub async fn write(
    github: &Github,
    token: &SecretString,
    reference: &IssueRef,
    why: &Why,
    public_url: &Url,
) -> Result<Written, DescriptionError> {
    let current = github
        .pull_request(token, reference)
        .await?
        .ok_or(DescriptionError::NotFound)?;
    let chain = section(why, public_url, Audience::of(&current));
    let description = splice(&current.description, &chain);
    if description == current.description {
        return Ok(Written::Unchanged);
    }
    if description.chars().count() > GITHUB_MAX_CHARACTERS {
        return Err(DescriptionError::TooLong);
    }
    github
        .update_pull_request_description(token, reference, &description)
        .await?;
    Ok(Written::Changed)
}

/// A batch's chain, `why`, as a description carries it, from [`START`] to [`END`], for
/// `audience`, linking into Elysium at `public_url`. Pull requests are left out: the description
/// is one of them.
pub fn section(why: &Why, public_url: &Url, audience: Audience) -> String {
    let origin = public_url.origin().ascii_serialization();
    let batch = why.batch();
    let targets = Targets {
        batch: format!("{origin}{WORK_BATCH_PATH}{}", batch.id),
        project: format!("{origin}{PROJECT_PATH}{}", batch.project_id),
        origin,
    };

    let mut lines: Vec<String> = Vec::new();
    for link in &why.chain {
        if let Some(line) = line(link, &targets, audience) {
            lines.push(line);
        }
    }
    let batch_text = said(audience, &batch.title, TITLE_MAX_CHARACTERS);
    let closing = format!(
        "- Work batch: {}\n{END}",
        markdown_link(&batch_text, &targets.batch)
    );

    // Bytes, which are never fewer than characters, so the section stays within its bound.
    let mut section = format!("{START}\n### Why this exists\n");
    let budget =
        SECTION_MAX_CHARACTERS.saturating_sub(section.len() + closing.len() + LEFT_OUT_MAX);
    let mut used = 0_usize;
    let mut left_out = 0_usize;
    for line in &lines {
        // Once one is left out, so is everything after it, so the chain keeps its order.
        if left_out > 0 || used + line.len() + 1 > budget {
            left_out += 1;
            continue;
        }
        used += line.len() + 1;
        section.push_str(line);
        section.push('\n');
    }
    if left_out > 0 {
        let _ = writeln!(
            section,
            "- {left_out} more on its work batch's page in Elysium"
        );
    }
    section.push_str(&closing);
    section
}

/// The most the line naming what was left out takes.
const LEFT_OUT_MAX: usize = 80;

/// Where a chain's links into Elysium go.
struct Targets {
    /// The web app's origin.
    origin: String,
    /// The batch's page.
    batch: String,
    /// The batch's project's page, which a tab is added to.
    project: String,
}

/// One link's line, and for a record its quote beneath; `None` for the batch, which closes the
/// section, and its pull requests, which it is in.
fn line(link: &Link, targets: &Targets, audience: Audience) -> Option<String> {
    let origin = &targets.origin;
    let entry = match link {
        Link::Finding(finding) => {
            let text = said(
                audience,
                &format!("{}, accepted by {}", finding.title, finding.accepted_by),
                TITLE_MAX_CHARACTERS,
            );
            let url = format!("{}?tab={FINDINGS_TAB}", targets.project);
            format!("- Finding: {}", markdown_link(&text, &url))
        }
        Link::Message(message) => {
            let text = said(
                audience,
                &format!("{}: “{}”", message.author, message.body),
                QUOTE_MAX_CHARACTERS,
            );
            let url = format!("{}?tab={MANAGER_TAB}", targets.project);
            format!(
                "- Message to its Project Manager: {}",
                markdown_link(&text, &url)
            )
        }
        // A segment of a private meeting says that a meeting asked, opening with the rest of the
        // chain on the batch's page for whoever may read it.
        Link::Record(record) if record.withheld => {
            let text = said(audience, PRIVATE_MEETING, TITLE_MAX_CHARACTERS);
            format!("- Meeting: {}", markdown_link(&text, &targets.batch))
        }
        Link::Record(record) => {
            // A record not indexed yet opens with the rest of the chain, on the batch's page.
            let url = record.document_id.map_or_else(
                || targets.batch.clone(),
                |document| format!("{origin}{KNOWLEDGE_DOCUMENT_PATH}{document}"),
            );
            let source = SOURCE_NAMES
                .iter()
                .find(|(kind, _name)| *kind == record.source_kind)
                .map_or("Record", |(_kind, name)| *name);
            let title = match &record.meeting_title {
                Some(meeting) => format!("{meeting}: {}", record.title),
                None => record.title.clone(),
            };
            let text = said(audience, &title, TITLE_MAX_CHARACTERS);
            let mut entry = format!("- {source}: {}", markdown_link(&text, &url));
            if audience == Audience::Company && !record.excerpt.trim().is_empty() {
                let quote = flattened(&format!("“{}”", record.excerpt), QUOTE_MAX_CHARACTERS);
                let _ = write!(entry, "\n  > {}", markdown_link(&quote, &url));
            }
            entry
        }
        Link::Issue(issue) => {
            // Anyone else opens it with the rest of the chain, on the batch's page.
            let (text, url) = match audience {
                Audience::Company => (
                    flattened(
                        &format!("{} {}", issue.key, issue.title),
                        TITLE_MAX_CHARACTERS,
                    ),
                    issue.url.as_str(),
                ),
                Audience::Anyone => (OPEN_IN_ELYSIUM.to_owned(), targets.batch.as_str()),
            };
            format!(
                "- {} issue: {}",
                issue.provider.name(),
                markdown_link(&text, url)
            )
        }
        Link::Item(item) => {
            let url = format!("{origin}{ACTION_ITEM_PATH}{}", item.id);
            let text = said(audience, &item.title, TITLE_MAX_CHARACTERS);
            format!("- Action item: {}", markdown_link(&text, &url))
        }
        Link::Batch(_) | Link::PullRequest(_) => return None,
    };
    Some(entry)
}

/// What a link says of `text` to `audience`: the text, cut to `max` characters, for the
/// company; that it opens in Elysium for anyone else.
fn said(audience: Audience, text: &str, max: usize) -> String {
    match audience {
        Audience::Company => flattened(text, max),
        Audience::Anyone => OPEN_IN_ELYSIUM.to_owned(),
    }
}

/// `text` on one line, its whitespace each one space, cut to `max` characters.
fn flattened(text: &str, max: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let joined = words.join(" ");
    let mut cut: String = joined.chars().take(max).collect();
    if cut.len() < joined.len() {
        cut.push('…');
    }
    cut
}

/// A Markdown link to `url` reading `text`, every ASCII punctuation mark in `text` escaped so it
/// stays plain text, and `url`'s parentheses and spaces encoded so they end nothing.
fn markdown_link(text: &str, url: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    let url = url
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29");
    format!("[{escaped}]({url})")
}

/// `description` carrying `section`: in place of the chain it carries already, from a line that
/// is [`START`] to the next that is [`END`], or after what it says, a blank line between. The
/// rest of it is left exactly as it was, line endings included.
pub fn splice(description: &str, section: &str) -> String {
    if let Some((start, end)) = marked(description) {
        let mut spliced = String::with_capacity(description.len() + section.len());
        spliced.push_str(&description[..start]);
        spliced.push_str(section);
        spliced.push_str(&description[end..]);
        return spliced;
    }
    let kept = description.trim_end();
    if kept.is_empty() {
        return section.to_owned();
    }
    format!("{kept}\n\n{section}")
}

/// Where the chain `description` carries is, by byte: the start of its [`START`] line to the end
/// of its [`END`] line's text.
fn marked(description: &str) -> Option<(usize, usize)> {
    let start = line_of(description, START, 0)?;
    let end = line_of(description, END, start + START.len())?;
    Some((start, end + END.len()))
}

/// Where the first line from byte `from` on that is exactly `marker` starts. A marker inside a
/// line, such as one quoted in prose, is not one.
fn line_of(description: &str, marker: &str, from: usize) -> Option<usize> {
    let mut search = from;
    while let Some(found) = description[search..].find(marker) {
        let at = search + found;
        let after = at + marker.len();
        let starts_line = at == 0 || description[..at].ends_with('\n');
        let rest = &description[after..];
        let ends_line = rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n");
        if starts_line && ends_line {
            return Some(at);
        }
        search = after;
    }
    None
}

#[cfg(test)]
mod tests;
