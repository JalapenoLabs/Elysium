// Copyright © 2026 Jalapeno Labs

//! Reads every active Confluence connector on an interval and puts what changed on the
//! timeline. See `docs/connectors.md`, Confluence.
//!
//! # A read
//!
//! A connector's read asks Confluence for the pages and comments in its spaces changed since
//! the read before ([`Confluence::changes`], bounded to those spaces), oldest first. Each
//! change is one timeline record: a page or comment at one version (`page:<id>:<version>`), so
//! every edit is on the timeline once and reading the same change again records nothing. Its
//! body is rendered as plain text ([`super::storage`]).
//!
//! # What raises an inbox item
//!
//! A change by someone else that mentions the account's person, or a comment with a question
//! on a page the person created, raises an inbox item for them, recorded as the watcher of
//! Confluence (`watcher:confluence`). A page or comment raises one at most once, so editing it
//! again does not raise another. Elysia's triage, on `timelineRecord.created`, reads the rest.
//!
//! # The window
//!
//! Each read looks back from now to where the last one finished, plus [`OVERLAP`], in whole
//! minutes through CQL's relative `now("-Nm")`, because a written date is read in the
//! account's own time zone. The overlap and the per-version ids make reading a change twice
//! harmless. A read reads at most [`MAX_CHANGES_PER_READ`]; one that stops there finishes at its
//! last change, and the next continues from it.
//!
//! # The first read
//!
//! A new connector's first reads look back [`BASELINE_WINDOW`] and record what they find
//! without raising anything, as a new mailbox's first read does, so connecting Confluence
//! does not fill the inbox with a week of mentions. Once a read reaches the end of what
//! matched, the cursor says so (`{"baselined": true}`).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use super::storage::{self, Rendered};
use super::{Change, ChangeQuery, Changes, Confluence, ConfluenceError, ContentType, SpaceScope};
use crate::connectors::activity::Activity;
use crate::connectors::ingest;
use crate::connectors::{ConnectorKind, Read, SignInState};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::action_item::{ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::connector::{self, Connector, PollOutcome};
use crate::models::connector_run::ConnectorRun;
use crate::models::timeline_record;
use crate::realtime::EventBus;
use crate::routes::v1::jira_credentials::open_with;

/// How often every Confluence connector is read. Pages change far less often than mail, and
/// five minutes keeps a mention prompt without searching a site more than it needs.
const READ_INTERVAL: Duration = Duration::from_mins(5);

/// How far before the last read's end the next one starts, so a change Confluence indexed a
/// little late is still found. Reading it twice records it once.
const OVERLAP: chrono::Duration = chrono::Duration::minutes(5);

/// How far a new connector's first reads look back.
const BASELINE_WINDOW: chrono::Duration = chrono::Duration::days(7);

/// The furthest any read looks back, so a connector paused for months catches up on the last
/// month rather than its whole history.
const MAX_WINDOW: chrono::Duration = chrono::Duration::days(30);

/// The most changes one read records. Ten pages of a search; the next read continues.
const MAX_CHANGES_PER_READ: usize = 500;

/// The longest body a record keeps. A long page's start is what a summary needs.
const BODY_MAX_CHARACTERS: usize = 20_000;

/// The longest title an inbox item is given; the record keeps the whole title.
const ITEM_TITLE_MAX_CHARACTERS: usize = 200;

/// How much of a body an inbox item's notes quote.
const ITEM_NOTES_MAX_CHARACTERS: usize = 2_000;

/// Why a change wants its person's attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// It mentions them.
    Mentioned,
    /// It is a comment asking a question on a page they created.
    Asked,
}

/// What the reader is built from.
pub struct ReaderDeps {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub events: EventBus,
    pub activity: Activity,
    pub confluence: Confluence,
}

/// Reads Confluence connectors. Cheap to clone; clones share one reader.
#[derive(Clone)]
pub struct Reader {
    inner: Arc<ReaderDeps>,
}

impl std::fmt::Debug for Reader {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Reader").finish_non_exhaustive()
    }
}

impl Reader {
    pub fn new(deps: ReaderDeps) -> Self {
        Self {
            inner: Arc::new(deps),
        }
    }

    /// The client every connector is read through, for checks that call Confluence.
    pub fn confluence(&self) -> &Confluence {
        &self.inner.confluence
    }

    /// Reads one connector now, as a pass would, for a sync someone asked for.
    pub async fn read_now(&self, connector: &Connector) {
        if let Err(error) = self.read_connector(connector).await {
            event!(
                name: "confluence.sync.failed",
                Level::WARN,
                connector.id = %connector.id,
                error.message = %format!("{error:#}"),
                "cannot record a Confluence connector's read",
            );
        }
    }

    /// Runs a pass every [`READ_INTERVAL`] until `shutdown` is cancelled.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let reader = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(READ_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    _tick = interval.tick() => reader.pass().await,
                }
            }
        })
    }

    async fn pass(&self) {
        let connectors = match self.active_connectors().await {
            Ok(connectors) => connectors,
            Err(error) => {
                event!(
                    name: "confluence.pass.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot list the Confluence connectors to read",
                );
                return;
            }
        };
        for connector in connectors {
            self.read_now(&connector).await;
        }
    }

    async fn active_connectors(&self) -> anyhow::Result<Vec<Connector>> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        Ok(connector::list_active(&mut connection, &[ConnectorKind::Confluence.as_str()]).await?)
    }

    /// Reads one connector and records what came of it, a failed read included.
    async fn read_connector(&self, connector: &Connector) -> anyhow::Result<()> {
        let credential_id = connector.jira_credential_id.context(
            "a Confluence connector reads through an Atlassian account, and this one has none",
        )?;
        let scope: SpaceScope = serde_json::from_value(connector.content["spaces"].clone())
            .context("the connector's spaces cannot be read")?;
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let opened = open_with(&mut connection, &self.inner.cipher, credential_id)
            .await
            .map_err(|error| anyhow::anyhow!("cannot open the Atlassian account: {error}"))?;
        drop(connection);

        let started = Utc::now();
        let cursor = Cursor::of(connector);
        let query = ChangeQuery {
            scope: &scope,
            types: &[ContentType::Page, ContentType::Comment],
            since_minutes: cursor.window_minutes(started),
            max_items: MAX_CHANGES_PER_READ,
        };
        let mut run = None;
        let outcome = match self.inner.confluence.changes(&opened.site(), &query).await {
            Ok(changes) => {
                let recorded = self
                    .record_changes(
                        connector,
                        &changes,
                        &opened.credential.account_id,
                        cursor.baselined,
                    )
                    .await?;
                if recorded > 0 {
                    run = Some(ConnectorRun::succeeded(connector.id, started, recorded));
                }
                PollOutcome {
                    sign_in_state: SignInState::SignedIn.as_str().to_owned(),
                    cursor: cursor.after(&changes, started),
                    last_polled_at: Some(Utc::now()),
                    last_error: None,
                }
            }
            Err(error) => {
                run = Some(ConnectorRun::failed(
                    connector.id,
                    started,
                    &error.to_string(),
                ));
                let sign_in = match error {
                    ConfluenceError::Unauthorized(_) => SignInState::SignedOut,
                    ConfluenceError::Invalid(_) | ConfluenceError::Refused(_) => {
                        SignInState::Unknown
                    }
                };
                PollOutcome {
                    sign_in_state: sign_in.as_str().to_owned(),
                    cursor: connector.cursor.clone(),
                    last_polled_at: Some(Utc::now()),
                    last_error: Some(error.to_string()),
                }
            }
        };

        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let updated = connector::record_poll(&mut connection, connector.id, outcome).await?;
        drop(connection);
        let changed = updated.sign_in_state != connector.sign_in_state
            || updated.last_error != connector.last_error;
        match run {
            // Recording a run announces the change too.
            Some(run) => self.inner.activity.record(run).await,
            None if changed => self.inner.activity.changed(connector.id),
            None => {}
        }
        Ok(())
    }

    /// Stores each change as a record, oldest first, raising an inbox item for each that wants
    /// its person's attention once the connector is baselined. Answers how many it stored.
    async fn record_changes(
        &self,
        connector: &Connector,
        changes: &Changes,
        own_account_id: &str,
        baselined: bool,
    ) -> anyhow::Result<usize> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let mut recorded = 0;
        for change in &changes.items {
            let rendered = storage::render(&change.storage);
            let wants = attention(change, &rendered, own_account_id);
            let mut raised = None;
            if baselined && let Some(reason) = wants {
                let already = timeline_record::has_raised_item(
                    &mut connection,
                    connector.id,
                    &content_prefix(change),
                )
                .await?;
                if !already {
                    raised = Some(raised_item(change, &rendered, reason));
                }
            }
            let read = to_read(change, &rendered, wants.is_some());
            let stored = ingest::record(
                &mut connection,
                &self.inner.events,
                connector,
                ConnectorKind::Confluence,
                &read,
                raised,
                // Pages and comments are no conversation's messages.
                false,
            )
            .await?;
            if stored {
                recorded += 1;
            }
        }
        Ok(recorded)
    }
}

/// Where a connector's reading has got to: whether its first reads are done, and the moment
/// its last read covered changes up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cursor {
    baselined: bool,
    through: Option<DateTime<Utc>>,
}

impl Cursor {
    fn of(connector: &Connector) -> Self {
        Self::from_value(&connector.cursor)
    }

    /// The cursor as `connectors.cursor` stores it. A missing or unreadable part reads as a
    /// connector that has not started.
    fn from_value(stored: &Value) -> Self {
        Self {
            baselined: stored["baselined"].as_bool().unwrap_or(false),
            through: stored["through"]
                .as_str()
                .and_then(|through| DateTime::parse_from_rfc3339(through).ok())
                .map(|through| through.with_timezone(&Utc)),
        }
    }

    /// How many minutes back a read starting `now` looks: to the last read's end plus the
    /// overlap, or a new connector's baseline, and never more than [`MAX_WINDOW`].
    fn window_minutes(self, now: DateTime<Utc>) -> i64 {
        let window = self
            .through
            .map_or(BASELINE_WINDOW, |through| now - through + OVERLAP)
            .min(MAX_WINDOW);
        // Rounded up, so the whole of the last minute is inside the window.
        (window.num_seconds().max(0) + 59) / 60
    }

    /// The cursor after a read that started `started` and found `changes`.
    ///
    /// A read that reached the end of what matched covered everything up to when it started,
    /// and baselines the connector. One that stopped at its limit covered up to its last
    /// change, and the next read continues from there.
    fn after(self, changes: &Changes, started: DateTime<Utc>) -> Value {
        let through = if changes.complete {
            Some(started)
        } else {
            changes
                .items
                .iter()
                .filter_map(|change| change.modified_at)
                .max()
                .or(self.through)
        };
        json!({
            "baselined": self.baselined || changes.complete,
            "through": through.map(|through| through.to_rfc3339()),
        })
    }
}

/// Why `change` wants `own_account_id`'s attention, if it does.
///
/// Their own changes never do. Otherwise a mention of them does, and a comment with a question
/// on a page they created does.
pub fn attention(change: &Change, rendered: &Rendered, own_account_id: &str) -> Option<Attention> {
    let by_them = change
        .modified_by
        .as_ref()
        .is_some_and(|by| by.account_id == own_account_id);
    if by_them {
        return None;
    }
    if rendered
        .mentions
        .iter()
        .any(|mention| mention == own_account_id)
    {
        return Some(Attention::Mentioned);
    }
    let on_their_page = change.content_type == ContentType::Comment
        && change
            .container
            .as_ref()
            .and_then(|container| container.created_by.as_deref())
            == Some(own_account_id);
    if on_their_page && rendered.text.contains('?') {
        return Some(Attention::Asked);
    }
    None
}

/// The prefix every version of `change`'s page or comment is recorded under.
fn content_prefix(change: &Change) -> String {
    format!("{}:{}:", change.content_type.as_str(), change.id)
}

/// A record's external id read back as the prefix every version of its page or comment shares
/// ([`content_prefix`]) and the version it is, such as `("page:42:", 7)`; `None` for an id this
/// reader did not write.
pub fn version_of(external_id: &str) -> Option<(&str, u64)> {
    let split = external_id.rfind(':')? + 1;
    let version = external_id[split..].parse().ok()?;
    Some((&external_id[..split], version))
}

/// The title a change is shown under: its own, or for an untitled comment, its page's.
fn title_of(change: &Change) -> String {
    if !change.title.is_empty() {
        return change.title.clone();
    }
    change
        .container
        .as_ref()
        .map_or_else(String::new, |container| format!("Re: {}", container.title))
}

/// A change as the timeline records it: one record per page or comment version.
fn to_read(change: &Change, rendered: &Rendered, wants_attention: bool) -> Read {
    Read {
        external_id: format!("{}{}", content_prefix(change), change.version),
        occurred_at: change.modified_at,
        actor: change
            .modified_by
            .as_ref()
            .map(|by| by.display_name.clone())
            .unwrap_or_default(),
        title: title_of(change),
        body: rendered.text.chars().take(BODY_MAX_CHARACTERS).collect(),
        url: change.url.clone(),
        wants_attention,
        metadata: json!({
            "contentType": change.content_type.as_str(),
            "contentId": change.id,
            "version": change.version,
            "spaceKey": change.space_key,
            "spaceName": change.space_name,
            "pageId": change.container.as_ref().map(|container| container.id.clone()),
        }),
        // A page and its comments are not a conversation.
        conversation: None,
    }
}

/// The inbox item a change raises for its person.
fn raised_item(change: &Change, rendered: &Rendered, reason: Attention) -> NewActionItem {
    let title = title_of(change);
    let headline = match reason {
        Attention::Mentioned => format!("Mentioned in {title}"),
        Attention::Asked => format!("Question on {title}"),
    };
    let author = change
        .modified_by
        .as_ref()
        .map_or("Someone", |by| by.display_name.as_str());
    let quoted: String = rendered
        .text
        .chars()
        .take(ITEM_NOTES_MAX_CHARACTERS)
        .collect();
    let mut notes = format!("{author} in {}\n\n{quoted}", change.space_name);
    if let Some(url) = &change.url {
        notes.push_str("\n\n");
        notes.push_str(url);
    }
    NewActionItem {
        title: headline.chars().take(ITEM_TITLE_MAX_CHARACTERS).collect(),
        notes,
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Container, Person};
    use super::*;

    const ME: &str = "me";

    fn change(content_type: ContentType, by: &str, storage: &str) -> Change {
        Change {
            id: "101".to_owned(),
            content_type,
            title: "Launch plan".to_owned(),
            space_key: "ENG".to_owned(),
            space_name: "Engineering".to_owned(),
            version: 3,
            modified_at: None,
            modified_by: Some(Person {
                account_id: by.to_owned(),
                display_name: "Grace".to_owned(),
            }),
            created_by: None,
            storage: storage.to_owned(),
            url: Some("https://acme.atlassian.net/wiki/spaces/ENG/pages/101".to_owned()),
            container: None,
        }
    }

    fn on_page_by(mut comment: Change, creator: &str) -> Change {
        comment.container = Some(Container {
            id: "100".to_owned(),
            title: "Launch plan".to_owned(),
            created_by: Some(creator.to_owned()),
        });
        comment
    }

    fn mentioning(account_id: &str) -> String {
        format!("<p>Ping <ac:link><ri:user ri:account-id=\"{account_id}\"/></ac:link></p>")
    }

    fn attention_of(change: &Change) -> Option<Attention> {
        attention(change, &storage::render(&change.storage), ME)
    }

    #[test]
    fn a_records_external_id_reads_back_as_its_content_and_version() {
        let page = change(ContentType::Page, "grace", "<p>Ship it</p>");
        let read = to_read(&page, &storage::render(&page.storage), false);

        assert_eq!(read.external_id, "page:101:3");
        assert_eq!(version_of(&read.external_id), Some(("page:101:", 3)));
        assert_eq!(version_of("page:101:"), None);
        assert_eq!(version_of("C1/1730000000.000100"), None);
    }

    #[test]
    fn a_mention_of_the_person_by_someone_else_wants_their_attention() {
        let page = change(ContentType::Page, "grace", &mentioning(ME));
        assert_eq!(attention_of(&page), Some(Attention::Mentioned));
    }

    #[test]
    fn a_mention_of_someone_else_does_not() {
        let page = change(ContentType::Page, "grace", &mentioning("ada"));
        assert_eq!(attention_of(&page), None);
    }

    #[test]
    fn the_persons_own_change_never_does() {
        let page = change(ContentType::Page, ME, &mentioning(ME));
        assert_eq!(attention_of(&page), None);
    }

    #[test]
    fn a_question_on_the_persons_own_page_does() {
        let comment = on_page_by(
            change(ContentType::Comment, "grace", "<p>Ship Monday?</p>"),
            ME,
        );
        assert_eq!(attention_of(&comment), Some(Attention::Asked));
    }

    #[test]
    fn a_comment_without_a_question_or_on_someone_elses_page_does_not() {
        let statement = on_page_by(
            change(ContentType::Comment, "grace", "<p>Looks good.</p>"),
            ME,
        );
        assert_eq!(attention_of(&statement), None);
        let elsewhere = on_page_by(
            change(ContentType::Comment, "grace", "<p>Ship Monday?</p>"),
            "ada",
        );
        assert_eq!(attention_of(&elsewhere), None);
    }

    #[test]
    fn each_version_is_its_own_record_under_its_content() {
        let page = change(ContentType::Page, "grace", "<p>Hello</p>");
        let read = to_read(&page, &storage::render(&page.storage), false);
        assert_eq!(read.external_id, "page:101:3");
        assert_eq!(read.body, "Hello");
        assert_eq!(read.actor, "Grace");
        assert_eq!(read.metadata["spaceKey"], "ENG");
        assert_eq!(content_prefix(&page), "page:101:");
    }

    #[test]
    fn a_raised_item_names_why_who_where_and_links_back() {
        let page = change(ContentType::Page, "grace", &mentioning(ME));
        let item = raised_item(&page, &storage::render(&page.storage), Attention::Mentioned);
        assert_eq!(item.title, "Mentioned in Launch plan");
        assert_eq!(item.state, ActionItemState::Inbox);
        assert!(item.notes.starts_with("Grace in Engineering\n\nPing @user"));
        assert!(item.notes.ends_with("/wiki/spaces/ENG/pages/101"));
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("a time")
            .with_timezone(&Utc)
    }

    #[test]
    fn a_new_connector_looks_back_its_baseline_and_a_long_pause_at_most_a_month() {
        let fresh = Cursor {
            baselined: false,
            through: None,
        };
        let now = at("2026-11-02T12:00:00Z");
        assert_eq!(fresh.window_minutes(now), 7 * 24 * 60);

        let paused = Cursor {
            baselined: true,
            through: Some(at("2026-01-01T00:00:00Z")),
        };
        assert_eq!(paused.window_minutes(now), 30 * 24 * 60);
    }

    #[test]
    fn a_read_looks_back_to_the_last_ones_end_plus_the_overlap_rounded_up() {
        let cursor = Cursor {
            baselined: true,
            through: Some(at("2026-11-02T11:54:30Z")),
        };
        assert_eq!(cursor.window_minutes(at("2026-11-02T12:00:00Z")), 11);
    }

    #[test]
    fn a_complete_read_covers_up_to_its_start_and_baselines() {
        let cursor = Cursor {
            baselined: false,
            through: None,
        };
        let started = at("2026-11-02T12:00:00Z");
        let changes = Changes {
            items: Vec::new(),
            complete: true,
        };
        assert_eq!(
            cursor.after(&changes, started),
            json!({ "baselined": true, "through": "2026-11-02T12:00:00+00:00" })
        );
    }

    #[test]
    fn a_read_cut_short_continues_from_its_last_change_and_stays_unbaselined() {
        let cursor = Cursor {
            baselined: false,
            through: None,
        };
        let mut first = change(ContentType::Page, "grace", "");
        first.modified_at = Some(at("2026-10-30T09:00:00Z"));
        let mut last = change(ContentType::Page, "grace", "");
        last.modified_at = Some(at("2026-10-31T09:00:00Z"));
        let changes = Changes {
            items: vec![first, last],
            complete: false,
        };
        assert_eq!(
            cursor.after(&changes, at("2026-11-02T12:00:00Z")),
            json!({ "baselined": false, "through": "2026-10-31T09:00:00+00:00" })
        );
    }

    #[test]
    fn a_cursor_written_after_a_read_is_read_back_by_the_next() {
        let cursor = Cursor {
            baselined: true,
            through: Some(at("2026-11-02T12:00:00Z")),
        };
        let changes = Changes {
            items: Vec::new(),
            complete: true,
        };
        let stored = cursor.after(&changes, at("2026-11-02T12:00:00Z"));
        assert_eq!(Cursor::from_value(&stored), cursor);
        assert_eq!(
            Cursor::from_value(&json!({})),
            Cursor {
                baselined: false,
                through: None,
            }
        );
    }
}
