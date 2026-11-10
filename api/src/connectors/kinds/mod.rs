// Copyright © 2026 Jalapeno Labs

//! Every connector kind, defined once. See `docs/connectors.md`.
//!
//! Each kind is one module that declares its [`Definition`] (whose it is, what it provides, the
//! account it signs in with, what it reads, its ordered checks, and how what it reads is
//! answered) and fills the hub's verbs for it: running its checks, creating it with its
//! sign-in, listing the options its content picks from, changing that content, syncing it now,
//! removing it with its sign-in, and, for a kind that sends, where a reply goes and sending it.
//! This module is the one place that matches on the kind, so routes and the web app never do.
//!
//! # A connector owns its sign-in
//!
//! Every connector signs in on its own: a new connector always comes with a new account, which
//! no other connector reads through, and removing the connector removes it. A kind read through
//! a browser starts its browser with it ([`browser`]); every other kind's account is checked
//! with its provider before anything is stored.
//!
//! # Adding a kind
//!
//! A new kind adds a [`ConnectorKind`], a module here with its definition and verbs, a row in
//! [`DEFINITIONS`] and in each dispatch below, its name to the web app's `CONNECTOR_KINDS`, its
//! names, description, and check sentences to its `connectors` translations, and its logo to its
//! `connectorLogos`. What it
//! [`Provides`] is how the loops that read a kind of thing find it, such as
//! `crate::meetings` finding every kind that provides meetings, and the link watcher bringing in
//! what is assigned to the person of every kind that provides issues, through the provider
//! [`link_provider`] names for it. A kind whose account is a new
//! provider adds an [`AccountKind`] and, when its sign-in is one API key, keeps that key in
//! `token_accounts`.

pub mod browser;
pub mod confluence;
pub mod fireflies;
pub mod fireflies_browser;
pub mod github;
pub mod jira;
pub mod slack;
pub mod teams;
pub mod webmail;
pub mod youtrack;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{Level, event};
use uuid::Uuid;

use super::checks::{self, CheckRun, CheckSpec, Checks, Outcome};
use super::{AccountKind, ConnectorError, ConnectorKind, Outgoing, Scope, Sent};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::action_item_link::{LinkCredential, LinkProvider};
use crate::models::connector::{AccountRef, Connector};
use crate::models::connector_check::{self, CheckResult};
use crate::models::draft::ReplyPlace;
use crate::models::timeline_record::TimelineRecord;
use crate::state::AppState;

/// What the hub knows about a kind. Served as is; the web app names every id from its
/// translations, so this carries structure only.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub kind: ConnectorKind,
    pub scope: Scope,
    /// The kinds of thing it reads.
    pub provides: &'static [Provides],
    /// The account it signs in with, which each of its connectors owns.
    pub account: AccountKind,
    /// The page its browser opens at, for a kind that reads through one.
    pub start_url: Option<&'static str>,
    /// What it reads, which its connector's form asks for.
    pub content: &'static [ContentField],
    /// Its checks, in the order they run.
    pub checks: &'static [CheckSpec],
    /// How what it reads is answered, as its person.
    pub replies: Replies,
}

/// How a kind's records are answered as their person, which the web app's reply box follows.
/// Every send is a draft its person sends (`crate::drafts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "through", rename_all = "snake_case")]
pub enum Replies {
    /// Its records are not answered from Elysium.
    Never,
    /// Through the connector itself, signed in as its person, in any of `places`. The kind
    /// fills [`reply_target`] and [`send`].
    Connector { places: &'static [ReplyPlace] },
    /// By mail, from a mailbox of the person's own (`crate::mail`), whatever the connector reads
    /// through.
    Mailbox,
}

/// A kind of thing a connector reads, so a loop that reads one kind of thing, or a list that
/// groups connectors by it, finds every kind that provides it without naming them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provides {
    /// Issues and pull requests.
    Issues,
    /// Pages and their comments.
    Pages,
    Mail,
    /// Chats and channel messages.
    Messages,
    /// Recorded meetings and their transcripts.
    Meetings,
}

/// One thing a connector's form asks for.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentField {
    pub id: &'static str,
    pub field: ContentFieldType,
    pub required: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentFieldType {
    /// Picks from what the account reaches, which `POST /connectors/options` lists. `"*"`
    /// picks everything, including what is added later.
    Selection,
    /// One of the workspace's storage locations, by id.
    StorageLocation,
    /// On or off, a JSON boolean. A new connector's form starts it at `on_by_default`, and the
    /// kind reads a connector that never saved it the same way.
    Toggle {
        #[serde(rename = "onByDefault")]
        on_by_default: bool,
    },
}

/// What the hub knows about a kind of account, so a new one's form can be drawn.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDefinition {
    pub kind: AccountKind,
    /// What a new account asks for; empty for a browser, which is signed in to on its stream.
    pub fields: &'static [AccountField],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountField {
    pub id: &'static str,
    pub field: AccountFieldType,
    pub required: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccountFieldType {
    Text,
    Email,
    Url,
    /// Written once and never returned.
    Secret,
    Choice {
        options: &'static [&'static str],
    },
}

/// Every kind, in the order the hub lists them.
pub const DEFINITIONS: [&Definition; 10] = [
    &jira::DEFINITION,
    &confluence::DEFINITION,
    &github::DEFINITION,
    &webmail::OUTLOOK,
    &webmail::GMAIL,
    &teams::DEFINITION,
    &slack::DEFINITION,
    &fireflies::DEFINITION,
    &fireflies_browser::DEFINITION,
    &youtrack::DEFINITION,
];

/// Every kind of account, with what a new one asks for.
pub const ACCOUNT_DEFINITIONS: [&AccountDefinition; 5] = [
    &jira::ACCOUNT,
    &github::ACCOUNT,
    &webmail::ACCOUNT,
    &fireflies::ACCOUNT,
    &youtrack::ACCOUNT,
];

pub fn definition(kind: ConnectorKind) -> &'static Definition {
    DEFINITIONS
        .into_iter()
        .find(|definition| definition.kind == kind)
        .expect("every kind has a definition in DEFINITIONS")
}

/// Every kind that provides `what`, in the hub's order.
pub fn providing(what: Provides) -> Vec<ConnectorKind> {
    DEFINITIONS
        .into_iter()
        .filter(|definition| definition.provides.contains(&what))
        .map(|definition| definition.kind)
        .collect()
}

/// Every kind read through a browser of its own, in the hub's order: those `ingest` reads
/// (`ConnectorKind::BROWSER_READ`) and those another loop reads, such as Fireflies (browser),
/// which the meetings loop does. Every one of their browsers is refreshed daily
/// (`crate::connectors::refresh`).
pub fn read_through_a_browser() -> Vec<ConnectorKind> {
    DEFINITIONS
        .into_iter()
        .filter(|definition| definition.account == AccountKind::Browser)
        .map(|definition| definition.kind)
        .collect()
}

/// The link provider a kind's issues are read through, for every kind that provides issues and
/// no other. Declaring [`Provides::Issues`] is all a kind does to have the link watcher bring in
/// what is assigned to its person (`crate::action_items::assigned`); a test fails when a kind
/// providing issues has no provider here.
pub const fn link_provider(kind: ConnectorKind) -> Option<LinkProvider> {
    match kind {
        ConnectorKind::Jira => Some(LinkProvider::Jira),
        ConnectorKind::Github => Some(LinkProvider::Github),
        ConnectorKind::Youtrack => Some(LinkProvider::Youtrack),
        ConnectorKind::Confluence
        | ConnectorKind::Outlook
        | ConnectorKind::Gmail
        | ConnectorKind::Teams
        | ConnectorKind::Slack
        | ConnectorKind::Fireflies
        | ConnectorKind::FirefliesBrowser => None,
    }
}

/// The credential an issue connector's account is to its link provider, through which what is
/// assigned to its person is read. `None` for a connector of a kind that provides no issues.
pub fn issue_credential(connector: &Connector) -> Option<LinkCredential> {
    let provider = link_provider(ConnectorKind::parse(&connector.kind)?)?;
    let id = match connector.account()? {
        AccountRef::Atlassian(id) | AccountRef::Github(id) | AccountRef::Token(id) => id,
        AccountRef::Browser(_id) => return None,
    };
    Some(LinkCredential { provider, id })
}

/// The kind a stored connector names.
///
/// # Errors
/// [`ApiError::Internal`] for a kind this build does not know, which only a database written
/// by a newer build holds.
pub fn kind_of(connector: &Connector) -> Result<ConnectorKind, ApiError> {
    ConnectorKind::parse(&connector.kind).ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "a connector of unknown kind {:?}",
            connector.kind
        ))
    })
}

/// The kinds every member of the workspace sees.
pub fn workspace_kinds() -> Vec<&'static str> {
    DEFINITIONS
        .into_iter()
        .filter(|definition| definition.scope == Scope::Workspace)
        .map(|definition| definition.kind.as_str())
        .collect()
}

/// Whether `person` sees and manages `connector`: a workspace kind is everyone's, a personal
/// kind its creator's alone.
pub fn is_visible(connector: &Connector, person: Uuid) -> bool {
    ConnectorKind::parse(&connector.kind).is_some_and(|kind| {
        definition(kind).scope == Scope::Workspace || connector.created_by == person
    })
}

/// The account a request names: a new one, in its kind's own fields, for a connector being set
/// up; or, for listing what a stored connector's content can pick from, the account that
/// connector owns.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AccountChoice {
    Existing { id: Uuid },
    New { new: Value },
}

/// What a check run checks: a stored connector, or one being set up.
#[derive(Debug)]
pub enum Source<'connector> {
    Stored(&'connector Connector),
    Draft {
        account: AccountChoice,
        content: Value,
    },
}

/// One kind's checks, ready to run.
pub enum KindChecks {
    Jira(jira::JiraChecks),
    Confluence(confluence::ConfluenceChecks),
    Github(github::GithubChecks),
    Webmail(webmail::WebmailChecks),
    Teams(teams::TeamsChecks),
    Slack(slack::SlackChecks),
    Fireflies(fireflies::FirefliesChecks),
    FirefliesBrowser(fireflies_browser::FirefliesBrowserChecks),
    Youtrack(youtrack::YoutrackChecks),
}

impl std::fmt::Debug for KindChecks {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("KindChecks(..)")
    }
}

impl Checks for KindChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match self {
            Self::Jira(checks) => checks.check(id).await,
            Self::Confluence(checks) => checks.check(id).await,
            Self::Github(checks) => checks.check(id).await,
            Self::Webmail(checks) => checks.check(id).await,
            Self::Teams(checks) => checks.check(id).await,
            Self::Slack(checks) => checks.check(id).await,
            Self::Fireflies(checks) => checks.check(id).await,
            Self::FirefliesBrowser(checks) => checks.check(id).await,
            Self::Youtrack(checks) => checks.check(id).await,
        }
    }
}

/// `kind`'s checks against `source`, opened as `current`.
///
/// # Errors
/// [`ApiError::BadRequest`] for an account or content the kind cannot use, [`ApiError::NotFound`]
/// for an account `current` does not have, and whatever opening a stored account answers.
pub async fn prepare_checks(
    state: &AppState,
    current: &CurrentUser,
    kind: ConnectorKind,
    source: Source<'_>,
) -> Result<KindChecks, ApiError> {
    Ok(match kind {
        ConnectorKind::Jira => KindChecks::Jira(jira::prepare_checks(state, source).await?),
        ConnectorKind::Confluence => {
            KindChecks::Confluence(confluence::prepare_checks(state, source).await?)
        }
        ConnectorKind::Github => KindChecks::Github(github::prepare_checks(state, source).await?),
        ConnectorKind::Outlook | ConnectorKind::Gmail => {
            KindChecks::Webmail(webmail::prepare_checks(state, kind, source).await?)
        }
        ConnectorKind::Teams => KindChecks::Teams(teams::prepare_checks(state, source).await?),
        ConnectorKind::Slack => KindChecks::Slack(slack::prepare_checks(state, source).await?),
        ConnectorKind::Fireflies => {
            KindChecks::Fireflies(fireflies::prepare_checks(state, source).await?)
        }
        ConnectorKind::FirefliesBrowser => {
            KindChecks::FirefliesBrowser(fireflies_browser::prepare_checks(state, source).await?)
        }
        ConnectorKind::Youtrack => {
            KindChecks::Youtrack(youtrack::prepare_checks(state, current, source).await?)
        }
    })
}

/// Starts running `checks` for `current` and answers the run, which the web app follows over
/// the event stream. A stored connector's results are kept once the run ends.
pub fn start_checks(
    state: &AppState,
    current: &CurrentUser,
    kind: ConnectorKind,
    connector_id: Option<Uuid>,
    mut checks: KindChecks,
) -> CheckRun {
    let specs = definition(kind).checks;
    let runs = state.connectors.check_runs.clone();
    let run = runs.start(kind, connector_id, current.id(), specs);
    let run_id = run.id;
    let database = state.database.clone();
    let activity = state.connectors.activity.clone();
    tokio::spawn(async move {
        let outcomes = checks::run_in_order(specs, &mut checks, |check_id, outcome| {
            runs.update(run_id, check_id, outcome);
        })
        .await;
        if let Some(connector_id) = connector_id {
            let ran_at = chrono::Utc::now();
            let results: Vec<CheckResult> = outcomes
                .into_iter()
                .map(|(check_id, outcome)| CheckResult {
                    connector_id,
                    check_id: check_id.to_owned(),
                    state: outcome.state.as_str().to_owned(),
                    count: outcome.count.and_then(|count| i64::try_from(count).ok()),
                    reason: outcome.reason.map(str::to_owned),
                    message: outcome
                        .message
                        .map(|message| message.chars().take(2048).collect()),
                    blocked_by: outcome.blocked_by.map(str::to_owned),
                    ran_at,
                })
                .collect();
            let stored = async {
                let mut connection = database.get().await?;
                connector_check::record(&mut connection, &results).await?;
                anyhow::Ok(())
            };
            if let Err(error) = stored.await {
                event!(
                    name: "connectors.checks.unrecorded",
                    Level::WARN,
                    connector.id = %connector_id,
                    error.message = %format!("{error:#}"),
                    "a connector's checks ran but their results could not be kept",
                );
            }
            activity.changed(connector_id);
        }
        runs.finish(run_id);
    });
    run
}

/// Creates a connector of `kind` for `current`, with the new account it signs in with and
/// `content`. A kind read through a browser starts its browser; any other checks its account
/// with its provider first.
///
/// # Errors
/// [`ApiError::BadRequest`] for an account or content the kind cannot use, including one that
/// names an existing account (no account is ever shared), [`ApiError::CodedConflict`] when the
/// host has no browser port left, and whatever the provider answered while checking it.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    kind: ConnectorKind,
    account: AccountChoice,
    content: Value,
) -> Result<Connector, ApiError> {
    let AccountChoice::New { new } = account else {
        return Err(ApiError::BadRequest(
            "a connector signs in on its own; give its new account, not an existing one".to_owned(),
        ));
    };
    match kind {
        ConnectorKind::Jira => jira::create(state, current, new, content).await,
        ConnectorKind::Confluence => confluence::create(state, current, new, content).await,
        ConnectorKind::Github => github::create(state, current, new, content).await,
        ConnectorKind::Outlook | ConnectorKind::Gmail => {
            webmail::create(state, current, kind, content).await
        }
        ConnectorKind::Teams => teams::create(state, current, content).await,
        ConnectorKind::Slack => slack::create(state, current, content).await,
        ConnectorKind::Fireflies => fireflies::create(state, current, new, content).await,
        ConnectorKind::FirefliesBrowser => fireflies_browser::create(state, current, content).await,
        ConnectorKind::Youtrack => youtrack::create(state, current, new, content).await,
    }
}

/// Starts a browser for `connector`, one read through a browser that is waiting for one, and
/// answers it reading through that browser.
///
/// # Errors
/// [`ApiError::BadRequest`] for a kind that signs in with a token, [`ApiError::Conflict`] when it
/// already has its browser, and [`ApiError::CodedConflict`] when the host has no port left.
pub async fn sign_in(state: &AppState, connector: &Connector) -> Result<Connector, ApiError> {
    let kind = kind_of(connector)?;
    if definition(kind).account != AccountKind::Browser {
        return Err(ApiError::BadRequest(format!(
            "{} signs in with the account it was made with",
            kind.as_str()
        )));
    }
    browser::sign_in(state, kind, connector).await
}

/// Where a reply to `record`, which a connector of `kind` read, goes `place` in its
/// conversation, in the kind's own terms. A draft keeps it (`drafts.target`) and [`send`] reads
/// it back.
///
/// # Errors
/// [`ApiError::BadRequest`] for a kind that does not send through its connector, a place it
/// does not reply in, or a record that does not say where its message was said.
pub fn reply_target(
    kind: ConnectorKind,
    record: &TimelineRecord,
    place: ReplyPlace,
) -> Result<Value, ApiError> {
    let Replies::Connector { places } = definition(kind).replies else {
        return Err(ApiError::BadRequest(format!(
            "{} does not send through its connector",
            kind.as_str()
        )));
    };
    if !places.contains(&place) {
        return Err(ApiError::BadRequest(format!(
            "{} does not reply in that place",
            kind.as_str()
        )));
    }
    match kind {
        ConnectorKind::Slack => slack::reply_target(record, place),
        ConnectorKind::Teams => teams::reply_target(record, place),
        ConnectorKind::Jira
        | ConnectorKind::Confluence
        | ConnectorKind::Github
        | ConnectorKind::Outlook
        | ConnectorKind::Gmail
        | ConnectorKind::Fireflies
        | ConnectorKind::FirefliesBrowser
        | ConnectorKind::Youtrack => Err(ApiError::Internal(anyhow::anyhow!(
            "{} declares that it sends, and has no reply target",
            kind.as_str()
        ))),
    }
}

/// Sends `outgoing` through `connector`, signed in as its person, to the target
/// [`reply_target`] wrote. This is the one way anything is posted through a connector; a draft
/// its person sends reaches it through `crate::drafts::send`.
///
/// # Errors
/// [`ConnectorError::Refused`] naming why it was not sent, in words its person can act on, and
/// [`ConnectorError`] when the browser or the page fails.
pub async fn send(
    state: &AppState,
    connector: &Connector,
    outgoing: Outgoing<'_>,
) -> Result<Sent, ConnectorError> {
    let kind = ConnectorKind::parse(&connector.kind).ok_or_else(|| {
        ConnectorError::Unexpected(format!("a connector of unknown kind {:?}", connector.kind))
    })?;
    match kind {
        ConnectorKind::Slack => slack::send(state, connector, outgoing).await,
        ConnectorKind::Teams => teams::send(state, connector, outgoing).await,
        ConnectorKind::Jira
        | ConnectorKind::Confluence
        | ConnectorKind::Github
        | ConnectorKind::Outlook
        | ConnectorKind::Gmail
        | ConnectorKind::Fireflies
        | ConnectorKind::FirefliesBrowser
        | ConnectorKind::Youtrack => Err(ConnectorError::Refused(format!(
            "{} does not send through its connector",
            kind.as_str()
        ))),
    }
}

/// What each of `kind`'s selection fields can pick from through `account`:
/// `{ <field>: { options: [{ id, label, detail }], truncated } }`.
///
/// # Errors
/// As [`create`]; a kind with no selection fields answers an empty object.
pub async fn options(
    state: &AppState,
    current: &CurrentUser,
    kind: ConnectorKind,
    account: AccountChoice,
) -> Result<Value, ApiError> {
    match kind {
        ConnectorKind::Jira => jira::options(state, account).await,
        ConnectorKind::Confluence => confluence::options(state, account).await,
        ConnectorKind::Teams => teams::options(state, current, account).await,
        ConnectorKind::Slack => slack::options(state, current, account).await,
        ConnectorKind::Youtrack => youtrack::options(state, current, account).await,
        ConnectorKind::Github => github::options(state, account).await,
        ConnectorKind::Outlook
        | ConnectorKind::Gmail
        | ConnectorKind::Fireflies
        | ConnectorKind::FirefliesBrowser => Ok(Value::Object(serde_json::Map::new())),
    }
}

/// Replaces what `connector` reads.
///
/// # Errors
/// [`ApiError::BadRequest`] for content its kind cannot use or a kind that reads nothing it can
/// choose, and whatever the provider answered while checking it.
pub async fn update_content(
    state: &AppState,
    connector: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    match kind_of(connector)? {
        ConnectorKind::Jira => jira::update_content(state, connector, content).await,
        ConnectorKind::Confluence => confluence::update_content(state, connector, content).await,
        ConnectorKind::Fireflies => fireflies::update_content(state, connector, content).await,
        ConnectorKind::FirefliesBrowser => {
            fireflies_browser::update_content(state, connector, content).await
        }
        ConnectorKind::Teams => teams::update_content(state, connector, content).await,
        ConnectorKind::Slack => slack::update_content(state, connector, content).await,
        ConnectorKind::Youtrack => youtrack::update_content(state, connector, content).await,
        ConnectorKind::Github => github::update_content(state, connector, content).await,
        kind @ (ConnectorKind::Outlook | ConnectorKind::Gmail) => {
            Err(ApiError::BadRequest(format!(
                "{} reads everything its account reaches; there is nothing to choose",
                kind.as_str()
            )))
        }
    }
}

/// Starts a sync of `connector` now. The engine reads every kind: Jira and GitHub in the link
/// watcher's next pass, which is woken at once, and every other kind as its own read, which the
/// connector shows as syncing until it ends. YouTrack is both: its reader reads it, and the woken
/// watcher brings in what is assigned to its person.
///
/// # Errors
/// [`ApiError::Conflict`] when it is paused or already syncing.
pub async fn sync(state: &AppState, connector: &Connector) -> Result<(), ApiError> {
    if !connector.is_active {
        return Err(ApiError::Conflict(
            "the connector is paused; resume it to sync",
        ));
    }
    if connector.syncing_since.is_some() {
        return Err(ApiError::Conflict("the connector is already syncing"));
    }
    let kind = kind_of(connector)?;
    match kind {
        // The link watcher alone reads them, every account in one pass, woken below.
        ConnectorKind::Jira | ConnectorKind::Github => {}
        ConnectorKind::Outlook
        | ConnectorKind::Gmail
        | ConnectorKind::Teams
        | ConnectorKind::Slack
        | ConnectorKind::Confluence
        | ConnectorKind::Fireflies
        | ConnectorKind::FirefliesBrowser
        | ConnectorKind::Youtrack => {
            if !state.connectors.activity.request_sync(connector.id).await? {
                return Err(ApiError::Conflict("the connector is already syncing"));
            }
        }
    }
    // An issue connector's links, and what is assigned to its person, are read on the link
    // watcher's pass, so reading one now wakes it, YouTrack's beside its own reader.
    if link_provider(kind).is_some() {
        state.links.wake_watcher();
    }
    Ok(())
}

/// Removes `connector` and the account it owns: its token, or its browser and the profile its
/// person signed in on. What it recorded stays on the timeline.
///
/// # Errors
/// [`ApiError::NotFound`] when it was already removed, and whatever Docker answered while
/// removing a browser.
pub async fn remove(state: &AppState, connector: &Connector) -> Result<(), ApiError> {
    match kind_of(connector)? {
        ConnectorKind::Jira => jira::remove(state, connector).await,
        ConnectorKind::Github => github::remove(state, connector).await,
        ConnectorKind::Confluence => confluence::remove(state, connector).await,
        ConnectorKind::Outlook
        | ConnectorKind::Gmail
        | ConnectorKind::Teams
        | ConnectorKind::Slack
        | ConnectorKind::FirefliesBrowser => browser::remove(state, connector).await,
        ConnectorKind::Fireflies | ConnectorKind::Youtrack => {
            let id = connector.token_account_id.ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "a {} connector without its token account",
                    connector.kind
                ))
            })?;
            let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
            // The account's connector goes with it, by its foreign key.
            crate::models::token_account::delete(&mut connection, id).await?;
            Ok(())
        }
    }
}

/// A kind's content, parsed from what the client sent.
///
/// # Errors
/// [`ApiError::BadRequest`] naming what the kind expected.
fn parse_content<Content: serde::de::DeserializeOwned>(
    kind: ConnectorKind,
    content: Value,
) -> Result<Content, ApiError> {
    serde_json::from_value(content).map_err(|error| {
        ApiError::BadRequest(format!("{} content is not valid: {error}", kind.as_str()))
    })
}

/// A new account's fields, parsed from what the client sent.
///
/// # Errors
/// [`ApiError::BadRequest`] naming what the kind expected.
fn parse_new_account<Fields: serde::de::DeserializeOwned>(
    kind: AccountKind,
    fields: Value,
) -> Result<Fields, ApiError> {
    serde_json::from_value(fields).map_err(|error| {
        ApiError::BadRequest(format!(
            "a new {} account is not valid: {error}",
            kind.as_str()
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_kind_has_exactly_one_definition() {
        let defined: Vec<ConnectorKind> = DEFINITIONS
            .into_iter()
            .map(|definition| definition.kind)
            .collect();
        assert_eq!(defined, ConnectorKind::ALL);
    }

    #[test]
    fn every_kind_provides_something_and_meetings_come_from_teams_slack_and_fireflies() {
        for definition in DEFINITIONS {
            assert!(
                !definition.provides.is_empty(),
                "{} provides nothing",
                definition.kind.as_str()
            );
        }
        assert_eq!(
            providing(Provides::Meetings),
            [
                ConnectorKind::Teams,
                ConnectorKind::Slack,
                ConnectorKind::Fireflies,
                ConnectorKind::FirefliesBrowser
            ]
        );
    }

    #[test]
    fn exactly_the_kinds_that_provide_issues_are_read_through_a_link_provider() {
        for kind in ConnectorKind::ALL {
            assert_eq!(
                definition(kind).provides.contains(&Provides::Issues),
                link_provider(kind).is_some(),
                "{}",
                kind.as_str()
            );
        }
        assert_eq!(
            providing(Provides::Issues),
            [
                ConnectorKind::Jira,
                ConnectorKind::Github,
                ConnectorKind::Youtrack
            ]
        );
    }

    #[test]
    fn every_meeting_source_asks_whether_to_wait_for_approval() {
        // The correction step reads the same field of every meeting source
        // (`crate::meetings::waits_for_approval`), so none may go without it.
        for kind in providing(Provides::Meetings) {
            let asks = definition(kind).content.iter().any(|field| {
                field.id == crate::meetings::WAIT_FOR_APPROVAL_FIELD
                    && matches!(field.field, ContentFieldType::Toggle { .. })
                    && !field.required
            });
            assert!(asks, "{} has no waitForApproval toggle", kind.as_str());
        }
    }

    /// Every kind that sends through its connector, in the hub's order.
    fn sending() -> Vec<ConnectorKind> {
        DEFINITIONS
            .into_iter()
            .filter(|definition| matches!(definition.replies, Replies::Connector { .. }))
            .map(|definition| definition.kind)
            .collect()
    }

    #[test]
    fn slack_and_teams_send_through_their_connector_and_mail_through_a_mailbox() {
        assert_eq!(sending(), [ConnectorKind::Teams, ConnectorKind::Slack]);
        for definition in DEFINITIONS {
            let expected = match definition.kind {
                ConnectorKind::Teams | ConnectorKind::Slack => Replies::Connector {
                    places: &[ReplyPlace::Thread, ReplyPlace::Conversation],
                },
                ConnectorKind::Outlook | ConnectorKind::Gmail => Replies::Mailbox,
                ConnectorKind::Jira
                | ConnectorKind::Confluence
                | ConnectorKind::Github
                | ConnectorKind::Fireflies
                | ConnectorKind::FirefliesBrowser
                | ConnectorKind::Youtrack => Replies::Never,
            };
            assert_eq!(definition.replies, expected, "{}", definition.kind.as_str());
        }
    }

    /// A record of `kind` that says where its message was said, in every kind's terms at once.
    fn answerable_record(kind: ConnectorKind) -> TimelineRecord {
        TimelineRecord {
            id: Uuid::now_v7(),
            source_kind: kind.as_str().to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: chrono::Utc::now(),
            actor: "Ada <ada@example.com>".to_owned(),
            title: "Launch".to_owned(),
            body: "Ship it?".to_owned(),
            url: None,
            media: None,
            metadata: serde_json::json!({
                "team": "T1",
                "conversation": "C1",
                "ts": "1730000000.000100",
                "conversationId": "19:launch@thread.tacv2",
                "messageId": "1730000000000",
                "isChat": false,
            }),
            action_item_id: None,
            created_by: Uuid::now_v7(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn every_kind_that_sends_writes_a_reply_target_and_no_other_kind_does() {
        for kind in ConnectorKind::ALL {
            let record = answerable_record(kind);
            for place in [ReplyPlace::Thread, ReplyPlace::Conversation] {
                let target = reply_target(kind, &record, place);
                assert_eq!(
                    target.is_ok(),
                    sending().contains(&kind),
                    "{} in {place:?}: {target:?}",
                    kind.as_str()
                );
            }
        }
    }

    #[test]
    fn a_browser_kind_names_the_page_its_browser_opens_at() {
        for definition in DEFINITIONS {
            let reads_through_a_browser = definition.account == AccountKind::Browser;
            assert_eq!(
                definition.start_url.is_some(),
                reads_through_a_browser,
                "{}",
                definition.kind.as_str()
            );
        }
    }

    #[test]
    fn every_account_kind_has_a_definition_and_every_kind_reads_through_one() {
        let accounts: HashSet<AccountKind> = ACCOUNT_DEFINITIONS
            .into_iter()
            .map(|definition| definition.kind)
            .collect();
        assert_eq!(accounts, AccountKind::ALL.into_iter().collect());
        for definition in DEFINITIONS {
            assert!(accounts.contains(&definition.account));
        }
    }

    #[test]
    fn checks_depend_only_on_checks_that_run_before_them() {
        for definition in DEFINITIONS {
            let mut seen: HashSet<&str> = HashSet::new();
            for spec in definition.checks {
                for dependency in spec.depends_on {
                    assert!(
                        seen.contains(dependency),
                        "{}'s {} depends on {}, which does not run before it",
                        definition.kind.as_str(),
                        spec.id,
                        dependency
                    );
                }
                assert!(seen.insert(spec.id), "{} is declared twice", spec.id);
            }
        }
    }

    #[test]
    fn an_account_choice_names_an_existing_account_or_a_new_one() {
        let id = Uuid::now_v7();
        let existing: AccountChoice =
            serde_json::from_value(serde_json::json!({ "id": id })).expect("parses");
        assert!(matches!(existing, AccountChoice::Existing { id: chosen } if chosen == id));

        let new: AccountChoice =
            serde_json::from_value(serde_json::json!({ "new": { "apiKey": "x" } }))
                .expect("parses");
        assert!(matches!(new, AccountChoice::New { .. }));

        serde_json::from_value::<AccountChoice>(serde_json::json!({ "name": "x" }))
            .expect_err("neither");
    }
}
