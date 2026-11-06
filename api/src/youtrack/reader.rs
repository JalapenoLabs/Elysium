// Copyright © 2026 Jalapeno Labs

//! Reads every active YouTrack connector on an interval and puts what it reads on the timeline.
//!
//! # A pass
//!
//! For each active connector: sign in with its account's token, list the issues its chosen
//! projects updated since its cursor, most recent first, and the comments on each. A new issue
//! and each new comment become a timeline record (one already recorded for the connector is
//! skipped, by its external id, `issue:<id>` or `comment:<id>`). A comment that wants its
//! person's attention also raises an inbox action item, recorded as the watcher of YouTrack
//! (`watcher:youtrack`), in the same transaction as its record: one someone else wrote, on an
//! issue the user is assigned or reported, or that mentions `@<login>`. Other changes to an
//! issue, such as its state, are not recorded: following an issue's state is what linking it to
//! an item is for.
//!
//! An issue raises nothing here. One assigned to the user comes in through the link watcher, as
//! every issue connector's do, as an item linked to it (`crate::action_items::assigned`).
//!
//! # The first read
//!
//! A connector's first read records nothing: it only moves the cursor to the newest change, so
//! connecting a site does not fill the timeline and the inbox with its history. The cursor is
//! `{"baselined": true, "since": <ms>}`, the newest update it has read, in YouTrack's own clock.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use super::{Comment, Issue, Site, Youtrack, YoutrackError, issue_url};
use crate::connectors::activity::Activity;
use crate::connectors::ingest::{raised_item, record};
use crate::connectors::kinds::youtrack::{Content, project_query};
use crate::connectors::{ConnectorKind, Read, SignInState};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::connector::{self, Connector, PollOutcome};
use crate::models::connector_run::ConnectorRun;
use crate::models::token_account;
use crate::realtime::EventBus;

/// How often every connector is read. Two minutes keeps a comment prompt without asking a site
/// for its issues more often than a person refreshing it would.
const READ_INTERVAL: Duration = Duration::from_mins(2);

/// How many issues one page of a read asks for.
const PAGE_SIZE: u32 = 50;

/// How many pages one read follows, so 200 changed issues at most. A site that changed more
/// than that between two reads two minutes apart is being migrated, not worked in; the cursor
/// still moves past it.
const PAGE_LIMIT: u32 = 4;

/// How many comments are read per changed issue, oldest first, as YouTrack lists them.
const COMMENT_LIMIT: u32 = 500;

#[derive(Clone)]
pub struct YoutrackReader {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    cipher: Arc<Cipher>,
    youtrack: Youtrack,
    activity: Activity,
}

/// What a [`YoutrackReader`] needs.
pub struct ReaderDeps {
    pub database: Pool,
    pub events: EventBus,
    pub cipher: Arc<Cipher>,
    pub youtrack: Youtrack,
    pub activity: Activity,
}

impl std::fmt::Debug for YoutrackReader {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("YoutrackReader")
            .finish_non_exhaustive()
    }
}

impl YoutrackReader {
    pub fn new(deps: ReaderDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                cipher: deps.cipher,
                youtrack: deps.youtrack,
                activity: deps.activity,
            }),
        }
    }

    /// The client every site is read through, for checks and options.
    pub fn youtrack(&self) -> &Youtrack {
        &self.inner.youtrack
    }

    /// Reads one connector now, as a pass would, for a sync someone asked for.
    pub async fn read_now(&self, connector: &Connector) {
        if let Err(error) = self.read_connector(connector).await {
            event!(
                name: "youtrack.sync.failed",
                Level::WARN,
                connector.id = %connector.id,
                error.message = %format!("{error:#}"),
                "cannot record a YouTrack connector's read",
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
                    name: "youtrack.pass.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot list the YouTrack connectors to read",
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
        Ok(connector::list_active(&mut connection, &[ConnectorKind::Youtrack.as_str()]).await?)
    }

    /// Reads one connector and records what came of it, a failed read included.
    async fn read_connector(&self, connector: &Connector) -> anyhow::Result<()> {
        let started = Utc::now();
        let (outcome, run) = match self.read_and_record(connector).await {
            Ok((cursor, recorded)) => (
                PollOutcome {
                    sign_in_state: SignInState::SignedIn.as_str().to_owned(),
                    cursor,
                    last_polled_at: Some(Utc::now()),
                    last_error: None,
                },
                (recorded > 0).then(|| ConnectorRun::succeeded(connector.id, started, recorded)),
            ),
            Err(error) => {
                let sign_in = match error.downcast_ref::<YoutrackError>() {
                    Some(YoutrackError::Unauthorized(_)) => SignInState::SignedOut,
                    _other => SignInState::Unknown,
                };
                let message = format!("{error:#}");
                (
                    PollOutcome {
                        sign_in_state: sign_in.as_str().to_owned(),
                        cursor: connector.cursor.clone(),
                        last_polled_at: Some(Utc::now()),
                        last_error: Some(message.clone()),
                    },
                    Some(ConnectorRun::failed(connector.id, started, &message)),
                )
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

    /// Reads what changed since the connector's cursor and stores it. Answers the next cursor
    /// and how many records were stored.
    async fn read_and_record(&self, connector: &Connector) -> anyhow::Result<(Value, usize)> {
        let account_id = connector
            .token_account_id
            .context("a YouTrack connector reads with a token, and this one has none")?;
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let account = token_account::find(&mut connection, account_id).await?;
        drop(connection);
        let site_url = account
            .site_url
            .clone()
            .context("a YouTrack account names its site, and this one has none")?;
        let token = account
            .credential(&self.inner.cipher)
            .map_err(|_open_error| anyhow::anyhow!("the stored token cannot be decrypted"))?;
        let site = Site {
            url: &site_url,
            token: &token,
        };
        let content: Content = serde_json::from_value(connector.content.clone())
            .context("the connector's content is not a YouTrack selection")?;

        let youtrack = &self.inner.youtrack;
        let me = youtrack.whoami(&site).await?;
        let projects = youtrack.list_projects(&site).await?;
        let Some(query) = project_query(&content.projects, &projects.items) else {
            // Every chosen project is gone or archived: nothing to read until the selection
            // changes, which the connector's checks say.
            return Ok((connector.cursor.clone(), 0));
        };
        let cursor = Cursor::of(connector);
        let changed = self.changed_issues(&site, &query, cursor.since).await?;
        let newest = changed.iter().map(|issue| issue.updated).max();
        // An empty site starts from now, so whatever it gets next is newer.
        let next = Cursor {
            since: newest.max(cursor.since).or_else(|| Some(Utc::now())),
        };
        if cursor.since.is_none() {
            return Ok((next.to_value(), 0));
        }

        let mut reads: Vec<Read> = Vec::new();
        // Oldest first, so the timeline reads in order.
        for issue in changed.iter().rev() {
            let comments = youtrack.comments(&site, &issue.id, COMMENT_LIMIT).await?;
            reads.extend(reads_of(
                &site_url,
                &me.login,
                issue,
                &comments,
                cursor.since,
            ));
        }
        let recorded = self.store(connector, &reads).await?;
        Ok((next.to_value(), recorded))
    }

    /// Issues updated after `since`, newest first, following up to [`PAGE_LIMIT`] pages.
    async fn changed_issues(
        &self,
        site: &Site<'_>,
        query: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Issue>, YoutrackError> {
        let mut changed: Vec<Issue> = Vec::new();
        for page in 0..PAGE_LIMIT {
            let issues = self
                .inner
                .youtrack
                .search(site, query, page * PAGE_SIZE, PAGE_SIZE)
                .await?;
            let full_page = issues.len() == PAGE_SIZE as usize;
            let mut reached_cursor = false;
            for issue in issues {
                if since.is_some_and(|since| issue.updated <= since) {
                    reached_cursor = true;
                    break;
                }
                changed.push(issue);
            }
            // A first read only needs the newest change, which the first page holds.
            if reached_cursor || !full_page || since.is_none() {
                break;
            }
        }
        Ok(changed)
    }

    /// Stores each read and the item it raises, and publishes them. Answers how many records
    /// were stored.
    async fn store(&self, connector: &Connector, reads: &[Read]) -> anyhow::Result<usize> {
        let mut recorded = 0;
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        for read in reads {
            let raised = read.wants_attention.then(|| raised_item(read));
            let stored = record(
                &mut connection,
                &self.inner.events,
                connector,
                ConnectorKind::Youtrack,
                read,
                raised,
                // Issues and comments are no conversation's messages.
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

/// Where a connector's reading has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cursor {
    /// The newest update read so far, in YouTrack's clock; `None` until the first read, which
    /// is what baselines the connector.
    since: Option<DateTime<Utc>>,
}

impl Cursor {
    fn of(connector: &Connector) -> Self {
        Self {
            since: connector.cursor["since"]
                .as_i64()
                .and_then(DateTime::from_timestamp_millis),
        }
    }

    fn to_value(self) -> Value {
        json!({
            "baselined": self.since.is_some(),
            "since": self.since.map(|since| since.timestamp_millis()),
        })
    }
}

/// What one changed issue contributes to the timeline: itself when it is new since `since`, and
/// each comment written since then.
fn reads_of(
    site_url: &str,
    me: &str,
    issue: &Issue,
    comments: &[Comment],
    since: Option<DateTime<Utc>>,
) -> Vec<Read> {
    let is_new = |moment: DateTime<Utc>| since.is_none_or(|since| moment > since);
    let url = issue_url(site_url, &issue.key);
    let is_mine = |user: Option<&super::User>| user.is_some_and(|user| user.login == me);
    let assigned_to_me = is_mine(issue.assignee.as_ref());
    let reported_by_me = is_mine(issue.reporter.as_ref());
    let name_of = |user: Option<&super::User>| {
        user.map_or_else(|| "YouTrack".to_owned(), |user| user.full_name.clone())
    };

    let mut reads: Vec<Read> = Vec::new();
    if is_new(issue.created) {
        reads.push(Read {
            external_id: format!("issue:{}", issue.id),
            occurred_at: Some(issue.created),
            actor: name_of(issue.reporter.as_ref()),
            title: format!("{} {}", issue.key, issue.summary),
            body: issue.description.clone(),
            url: Some(url.clone()),
            // An issue assigned to the user comes in as an item linked to it, through the link
            // watcher (`crate::action_items::assigned`), so its record raises nothing of its own.
            wants_attention: false,
            metadata: json!({}),
            // An issue and its comments are not a conversation.
            conversation: None,
        });
    }
    let mention = format!("@{me}");
    for comment in comments.iter().filter(|comment| is_new(comment.created)) {
        let by_me = is_mine(comment.author.as_ref());
        reads.push(Read {
            external_id: format!("comment:{}", comment.id),
            occurred_at: Some(comment.created),
            actor: name_of(comment.author.as_ref()),
            title: format!("Comment on {} {}", issue.key, issue.summary),
            body: comment.text.clone(),
            url: Some(format!("{url}#focus=Comments-{}.0-0", comment.id)),
            wants_attention: !by_me
                && (assigned_to_me || reported_by_me || comment.text.contains(&mention)),
            metadata: json!({}),
            // An issue and its comments are not a conversation.
            conversation: None,
        });
    }
    reads
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::youtrack::User;

    fn person(login: &str) -> User {
        User {
            login: login.to_owned(),
            full_name: format!("{login} (full name)"),
        }
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("a moment")
    }

    fn issue(reporter: &str, assignee: Option<&str>, created: i64) -> Issue {
        Issue {
            id: "2-1".to_owned(),
            key: "JL-1".to_owned(),
            summary: "Ship it".to_owned(),
            description: "Details".to_owned(),
            created: at(created),
            updated: at(created),
            resolved: false,
            reporter: Some(person(reporter)),
            assignee: assignee.map(person),
            project_id: "0-1".to_owned(),
            project_short_name: "JL".to_owned(),
            state: None,
            priority: None,
        }
    }

    fn comment(id: &str, author: &str, text: &str, created: i64) -> Comment {
        Comment {
            id: id.to_owned(),
            text: text.to_owned(),
            created: at(created),
            author: Some(person(author)),
        }
    }

    const SITE: &str = "https://acme.youtrack.cloud";

    #[test]
    fn a_new_issue_assigned_to_me_is_recorded_and_left_to_the_watcher_to_bring_in() {
        let reads = reads_of(
            SITE,
            "alex",
            &issue("bea", Some("alex"), 20),
            &[],
            Some(at(10)),
        );

        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].external_id, "issue:2-1");
        assert_eq!(reads[0].title, "JL-1 Ship it");
        assert_eq!(
            reads[0].url.as_deref(),
            Some("https://acme.youtrack.cloud/issue/JL-1")
        );
        assert!(!reads[0].wants_attention);
    }

    #[test]
    fn an_issue_created_before_the_cursor_is_not_recorded_again() {
        let reads = reads_of(
            SITE,
            "alex",
            &issue("bea", Some("alex"), 5),
            &[],
            Some(at(10)),
        );
        assert!(reads.is_empty());
    }

    #[test]
    fn comments_want_attention_on_my_issues_or_when_they_mention_me_but_never_my_own() {
        let comments = [
            comment("4-1", "bea", "Before the cursor", 5),
            comment("4-2", "bea", "Can you look?", 20),
            comment("4-3", "alex", "On it", 21),
        ];
        let reads = reads_of(
            SITE,
            "alex",
            &issue("bea", Some("alex"), 1),
            &comments,
            Some(at(10)),
        );

        let seen: Vec<(&str, bool)> = reads
            .iter()
            .map(|read| (read.external_id.as_str(), read.wants_attention))
            .collect();
        assert_eq!(seen, [("comment:4-2", true), ("comment:4-3", false)]);
        assert_eq!(
            reads[0].url.as_deref(),
            Some("https://acme.youtrack.cloud/issue/JL-1#focus=Comments-4-2.0-0")
        );

        let elsewhere = reads_of(
            SITE,
            "alex",
            &issue("bea", Some("carl"), 1),
            &[
                comment("4-4", "carl", "Fixed", 20),
                comment("4-5", "carl", "@alex please review", 21),
            ],
            Some(at(10)),
        );
        let attention: Vec<bool> = elsewhere.iter().map(|read| read.wants_attention).collect();
        assert_eq!(attention, [false, true]);
    }

    /// What a connector has recorded: its records' external ids, oldest first, and how many of
    /// them raised an item.
    async fn recorded(
        connection: &mut diesel_async::AsyncPgConnection,
        connector_id: uuid::Uuid,
    ) -> (Vec<String>, usize) {
        use diesel::prelude::*;
        use diesel_async::RunQueryDsl;

        use crate::database::schema::timeline_records;

        let rows: Vec<(Option<String>, Option<uuid::Uuid>)> = timeline_records::table
            .filter(timeline_records::connector_id.eq(connector_id))
            .order(timeline_records::occurred_at.asc())
            .select((
                timeline_records::external_id,
                timeline_records::action_item_id,
            ))
            .load(connection)
            .await
            .expect("records");
        let raised = rows.iter().filter(|(_id, item)| item.is_some()).count();
        (
            rows.into_iter().filter_map(|(id, _item)| id).collect(),
            raised,
        )
    }

    /// `bea` comments on `JL-1`, which moves its update time, and files `JL-2` for me. Kept out
    /// of the async test, so the fake's lock is never held across an await.
    fn someone_comments_and_files_an_issue(shared: &crate::youtrack::tests::Shared) {
        use crate::youtrack::tests::{FakeComment, sample_issue};

        let mut site = shared.lock().expect("lock");
        site.issues[0].updated = 2_000_000;
        site.issues.push(sample_issue("2-2", "JL-2", 3_000_000));
        site.comments.push(FakeComment::new(
            "2-1",
            "4-1",
            "Can you take a look?",
            2_000_000,
            "bea",
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_first_read_baselines_and_later_reads_record_only_what_is_new() {
        use crate::models::connector::{AccountRef, NewConnector};
        use crate::models::token_account::NewTokenAccount;
        use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};
        use crate::youtrack::tests::{
            FakeSite, ME, SITE_URL, TOKEN, fake_youtrack_site, sample_issue,
        };

        let (url, mut connection) = migrated_database().await;
        let cipher = Arc::new(cipher());
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("pool");
        let events = EventBus::in_process();
        let (youtrack, shared) = fake_youtrack_site(FakeSite {
            issues: vec![sample_issue("2-1", "JL-1", 1_000_000)],
            ..FakeSite::default()
        })
        .await;
        let reader = YoutrackReader::new(ReaderDeps {
            database: database.clone(),
            events: events.clone(),
            cipher: Arc::clone(&cipher),
            youtrack,
            activity: Activity::new(database, events),
        });

        let mut new_account = NewTokenAccount::new(
            &cipher,
            "youtrack",
            "YouTrack",
            &secrecy::SecretString::from(TOKEN),
            ME,
            TEST_PERSON_ID,
        );
        new_account.site_url = Some(SITE_URL.to_owned());
        let account = token_account::create(&mut connection, new_account)
            .await
            .expect("account");
        let mut new_connector =
            NewConnector::new("youtrack", AccountRef::Token(account.id), TEST_PERSON_ID);
        new_connector.content = json!({ "projects": "*" });
        let stored = connector::create(&mut connection, new_connector)
            .await
            .expect("connector");

        // The first read records the site's history as read, and nothing else.
        reader.read_now(&stored).await;
        let baselined = connector::find(&mut connection, stored.id)
            .await
            .expect("found");
        assert_eq!(
            baselined.cursor,
            json!({ "baselined": true, "since": 1_000_000 })
        );
        assert_eq!(baselined.sign_in_state, "signed_in");
        assert_eq!(recorded(&mut connection, stored.id).await, (Vec::new(), 0));

        // Someone comments on the old issue and files a new one for me.
        someone_comments_and_files_an_issue(&shared);
        reader.read_now(&baselined).await;
        let (ids, raised) = recorded(&mut connection, stored.id).await;
        assert_eq!(ids, ["comment:4-1", "issue:2-2"]);
        assert_eq!(
            raised, 1,
            "the comment is someone else's on my issue; the issue is the watcher's to bring in"
        );

        // Nothing new: nothing recorded twice.
        let advanced = connector::find(&mut connection, stored.id)
            .await
            .expect("found");
        assert_eq!(advanced.cursor["since"], json!(3_000_000));
        reader.read_now(&advanced).await;
        assert_eq!(recorded(&mut connection, stored.id).await.0.len(), 2);
    }

    #[test]
    fn a_cursor_is_baselined_only_once_it_holds_a_moment() {
        let cursor = Cursor {
            since: Some(at(42)),
        };
        assert_eq!(
            cursor.to_value(),
            json!({ "baselined": true, "since": 42_000 })
        );
        assert_eq!(
            Cursor { since: None }.to_value(),
            json!({ "baselined": false, "since": null })
        );
    }
}
