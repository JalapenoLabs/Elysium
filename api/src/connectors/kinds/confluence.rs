// Copyright © 2026 Jalapeno Labs

//! Confluence: pages and comments on a Confluence Cloud site, read through an Atlassian account
//! a Jira connector already uses. See `crate::confluence` and `docs/connectors.md`, Confluence.
//!
//! An Atlassian account is made with its Jira connector (`super::jira`), so a Confluence
//! connector always reads through one that already exists, and the token is never given twice.
//! Its content is the spaces it reads, in `connectors.content` as `{"spaces": "*"}` or the spaces
//! picked, each with the id, key, and name Confluence reported when it was picked. The key is
//! what its searches name it by.

use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{
    AccountChoice, AccountChoices, ContentField, ContentFieldType, Definition, Source,
    parse_content,
};
use crate::auth::CurrentUser;
use crate::confluence::{
    ChangeQuery, Confluence, ConfluenceError, ContentType, Listing, Space, SpaceScope,
};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::{AccountKind, ConnectorKind, Scope};
use crate::errors::ApiError;
use crate::models::connector::{self, AccountRef, Connector, NewConnector};
use crate::routes::v1::jira_credentials::{OpenCredential, Selection, open};
use crate::state::AppState;

/// How far back a sample looks: far enough that a site in use has something to show.
const SAMPLE_MINUTES: i64 = 90 * 24 * 60;

/// How many pages or comments a sample reads: enough to prove the search works.
const SAMPLE_SIZE: usize = 5;

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Confluence,
    scope: Scope::Workspace,
    account: AccountKind::Atlassian,
    accounts: AccountChoices::Existing,
    start_url: None,
    content: &[ContentField {
        id: "spaces",
        field: ContentFieldType::Selection,
        required: true,
    }],
    checks: &[
        CheckSpec {
            id: "sign_in",
            required: true,
            depends_on: &[],
        },
        CheckSpec {
            id: "list_spaces",
            required: true,
            depends_on: &["sign_in"],
        },
        CheckSpec {
            id: "sample_pages",
            required: true,
            depends_on: &["list_spaces"],
        },
        CheckSpec {
            id: "sample_page_comments",
            required: false,
            depends_on: &["list_spaces"],
        },
    ],
};

/// The spaces a Confluence connector reads, as the client sends them: `"*"` or the ids or keys
/// of the spaces picked.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Content {
    spaces: Selection<String>,
}

/// The spaces a selection names, as the listing reports them, or the picks the listing does
/// not hold.
fn resolve(selection: &Selection<String>, listing: &[Space]) -> Result<SpaceScope, Vec<String>> {
    let Selection::Only(references) = selection else {
        return Ok(SpaceScope::All);
    };
    let mut chosen: Vec<Space> = Vec::with_capacity(references.len());
    let mut missing: Vec<String> = Vec::new();
    for reference in references {
        let reference = reference.trim();
        let found = listing
            .iter()
            .find(|space| space.id == reference || space.key.eq_ignore_ascii_case(reference));
        match found {
            Some(space) => chosen.push(space.clone()),
            None => missing.push(reference.to_owned()),
        }
    }
    if missing.is_empty() {
        return Ok(SpaceScope::Only(chosen));
    }
    Err(missing)
}

/// A stored scope as the selection it was picked from, so checks resolve it again against what
/// the token reaches now.
fn as_selection(scope: &SpaceScope) -> Selection<String> {
    match scope {
        SpaceScope::All => Selection::All,
        SpaceScope::Only(spaces) => {
            Selection::Only(spaces.iter().map(|space| space.id.clone()).collect())
        }
    }
}

/// The spaces a stored connector reads.
///
/// # Errors
/// [`ApiError::Internal`] for content that is not a scope, which only a hand-edited row holds.
fn stored_scope(stored: &Connector) -> Result<SpaceScope, ApiError> {
    serde_json::from_value(stored.content["spaces"].clone()).map_err(|error| {
        ApiError::Internal(anyhow::anyhow!(
            "a Confluence connector's spaces cannot be read: {error}"
        ))
    })
}

/// The Atlassian account a new connector names, which must be one that exists.
///
/// # Errors
/// [`ApiError::BadRequest`] for a new account: one is made with its Jira connector.
fn existing_account(account: &AccountChoice) -> Result<Uuid, ApiError> {
    match account {
        AccountChoice::Existing { id } => Ok(*id),
        AccountChoice::New { .. } => Err(ApiError::BadRequest(
            "Confluence reads through an Atlassian account you already have; add its site as a \
             Jira connector first"
                .to_owned(),
        )),
    }
}

/// The error a request answers when Confluence refused or could not be reached.
fn api_error(error: ConfluenceError) -> ApiError {
    match error {
        ConfluenceError::Unauthorized(message) => ApiError::BadRequest(message.to_owned()),
        ConfluenceError::Invalid(message) => ApiError::BadRequest(message),
        ConfluenceError::Refused(message) => ApiError::BadGateway(message),
    }
}

/// The spaces `opened`'s token reaches now.
///
/// # Errors
/// As [`api_error`].
async fn list_spaces(
    confluence: &Confluence,
    opened: &OpenCredential,
) -> Result<Listing<Space>, ApiError> {
    confluence
        .list_spaces(&opened.site())
        .await
        .map_err(api_error)
}

/// The scope `selection` resolves to through `opened`, refusing a pick the token no longer
/// reaches.
///
/// # Errors
/// [`ApiError::BadRequest`] naming each such pick, and whatever Confluence answered.
async fn chosen_scope(
    confluence: &Confluence,
    opened: &OpenCredential,
    selection: &Selection<String>,
) -> Result<SpaceScope, ApiError> {
    let listing = list_spaces(confluence, opened).await?;
    resolve(selection, &listing.items).map_err(|missing| {
        ApiError::BadRequest(format!(
            "the token does not reach the space {}",
            missing.join(", ")
        ))
    })
}

pub async fn prepare_checks(
    state: &AppState,
    source: Source<'_>,
) -> Result<ConfluenceChecks, ApiError> {
    let (account_id, selection) = match source {
        Source::Stored(stored) => {
            let id = stored.jira_credential_id.ok_or_else(|| {
                ApiError::BadRequest(
                    "a Confluence connector reads through an Atlassian account".to_owned(),
                )
            })?;
            (id, as_selection(&stored_scope(stored)?))
        }
        Source::Draft { account, content } => {
            let content: Content = parse_content(ConnectorKind::Confluence, content)?;
            (existing_account(&account)?, content.spaces)
        }
    };
    Ok(ConfluenceChecks {
        confluence: state.connectors.confluence.confluence().clone(),
        opened: open(state, account_id).await?,
        selection,
        scope: None,
    })
}

/// Confluence's checks against one Atlassian account and selection.
#[derive(Debug)]
pub struct ConfluenceChecks {
    confluence: Confluence,
    opened: OpenCredential,
    selection: Selection<String>,
    /// The spaces the selection resolves to, once `list_spaces` passes.
    scope: Option<SpaceScope>,
}

impl Checks for ConfluenceChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "sign_in" => match self.confluence.verify(&self.opened.site()).await {
                Ok(_account) => Outcome::passed(),
                Err(error) => Outcome::failed(error.to_string()),
            },
            "list_spaces" => self.list_spaces().await,
            "sample_pages" => self.sample(ContentType::Page).await,
            "sample_page_comments" => self.sample(ContentType::Comment).await,
            unknown => Outcome::failed(format!("Confluence has no check {unknown}")),
        }
    }
}

impl ConfluenceChecks {
    /// The token reaches every space the selection names. Counts the spaces it reads.
    async fn list_spaces(&mut self) -> Outcome {
        let listing = match self.confluence.list_spaces(&self.opened.site()).await {
            Ok(listing) => listing,
            Err(error) => return Outcome::failed(error.to_string()),
        };
        match resolve(&self.selection, &listing.items) {
            Ok(scope) => {
                let count = match &scope {
                    SpaceScope::All => listing.items.len(),
                    SpaceScope::Only(spaces) => spaces.len(),
                };
                self.scope = Some(scope);
                Outcome::counted(count)
            }
            Err(missing) => Outcome::failed(format!(
                "the token no longer reaches the space {}",
                missing.join(", ")
            )),
        }
    }

    /// A bounded search of the chosen spaces answers, for one type of content.
    async fn sample(&self, content_type: ContentType) -> Outcome {
        let Some(scope) = &self.scope else {
            return Outcome::failed_because("spaces_unresolved");
        };
        if scope.reads_nothing() {
            return Outcome::skipped("no_spaces_chosen");
        }
        let query = ChangeQuery {
            scope,
            types: &[content_type],
            since_minutes: SAMPLE_MINUTES,
            max_items: SAMPLE_SIZE,
        };
        match self.confluence.changes(&self.opened.site(), &query).await {
            Ok(changes) => Outcome::counted(changes.items.len()),
            Err(error) => Outcome::failed(error.to_string()),
        }
    }
}

/// Creates a Confluence connector reading through an existing Atlassian account.
///
/// # Errors
/// [`ApiError::BadRequest`] for a new account or a space the token does not reach,
/// [`ApiError::NotFound`] for an account that does not exist, and [`ApiError::Conflict`] when
/// the account already feeds a Confluence connector.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
    content: Value,
) -> Result<Connector, ApiError> {
    let account_id = existing_account(&account)?;
    let content: Content = parse_content(ConnectorKind::Confluence, content)?;
    let opened = open(state, account_id).await?;
    let scope = chosen_scope(
        state.connectors.confluence.confluence(),
        &opened,
        &content.spaces,
    )
    .await?;

    let mut new_connector = NewConnector::new(
        ConnectorKind::Confluence.as_str(),
        AccountRef::Atlassian(account_id),
        current.id(),
    );
    new_connector.content = json!({ "spaces": scope });
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    connector::create(&mut connection, new_connector)
        .await
        .map_err(|error| match error {
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            ) => ApiError::Conflict("this Atlassian account already feeds a Confluence connector"),
            other => other.into(),
        })
}

/// What the spaces can be picked from, as the account's token reaches them now.
///
/// # Errors
/// As [`create`], for the account.
pub async fn options(state: &AppState, account: AccountChoice) -> Result<Value, ApiError> {
    let opened = open(state, existing_account(&account)?).await?;
    let listing = list_spaces(state.connectors.confluence.confluence(), &opened).await?;
    let spaces: Vec<Value> = listing
        .items
        .iter()
        .map(|space| json!({ "id": space.id, "label": space.name, "detail": space.key }))
        .collect();
    let mut fields = Map::new();
    fields.insert(
        "spaces".to_owned(),
        json!({ "options": spaces, "truncated": listing.truncated }),
    );
    Ok(Value::Object(fields))
}

/// Replaces the spaces, checked against what the token reaches first.
///
/// # Errors
/// As [`create`], for the spaces.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::Confluence, content)?;
    let account_id = stored.jira_credential_id.ok_or_else(|| {
        ApiError::BadRequest("a Confluence connector reads through an Atlassian account".to_owned())
    })?;
    let opened = open(state, account_id).await?;
    let scope = chosen_scope(
        state.connectors.confluence.confluence(),
        &opened,
        &content.spaces,
    )
    .await?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let updated = connector::set_content(
        &mut connection,
        stored.id,
        json!({ "spaces": scope }),
        stored.storage_location_id,
    )
    .await?;
    state.connectors.activity.changed(stored.id);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(id: &str, key: &str) -> Space {
        Space {
            id: id.to_owned(),
            key: key.to_owned(),
            name: format!("Space {key}"),
        }
    }

    #[test]
    fn a_pick_resolves_by_id_or_key_to_the_spaces_listed() {
        let listing = [space("1", "ENG"), space("2", "OPS")];
        let picked = Selection::Only(vec!["1".to_owned(), "ops".to_owned()]);
        assert_eq!(
            resolve(&picked, &listing),
            Ok(SpaceScope::Only(vec![space("1", "ENG"), space("2", "OPS")]))
        );
        assert_eq!(resolve(&Selection::All, &listing), Ok(SpaceScope::All));
    }

    #[test]
    fn a_pick_the_listing_no_longer_holds_is_named() {
        let listing = [space("1", "ENG")];
        let picked = Selection::Only(vec!["ENG".to_owned(), "GONE".to_owned()]);
        assert_eq!(resolve(&picked, &listing), Err(vec!["GONE".to_owned()]));
    }

    #[test]
    fn a_stored_scope_round_trips_through_its_selection() {
        let listing = [space("1", "ENG"), space("2", "OPS")];
        let stored = SpaceScope::Only(vec![space("2", "OPS")]);
        assert_eq!(resolve(&as_selection(&stored), &listing), Ok(stored));
    }

    #[test]
    fn a_new_account_is_refused_for_confluence() {
        let refused = existing_account(&AccountChoice::New { new: json!({}) });
        assert!(matches!(refused, Err(ApiError::BadRequest(_))));
        let id = Uuid::now_v7();
        assert!(
            matches!(existing_account(&AccountChoice::Existing { id }), Ok(chosen) if chosen == id)
        );
    }
}
