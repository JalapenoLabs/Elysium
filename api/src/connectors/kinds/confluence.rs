// Copyright © 2026 Jalapeno Labs

//! Confluence: pages and comments on a Confluence Cloud site, read through an Atlassian account
//! of its own. See `crate::confluence` and `docs/connectors.md`, Confluence.
//!
//! The account is an Atlassian one, as Jira's is (`jira_credentials`), signed in with the same
//! site, address, and API token fields, and the connector owns it: made with it, checked with
//! Confluence before it is stored, and removed with it. The account allows no Jira project or
//! board, so its token never reaches Jira through Elysium, and no Jira picker offers it.
//! Its content is the spaces it reads, in `connectors.content` as `{"spaces": "*"}` or the spaces
//! picked, each with the id, key, and name Confluence reported when it was picked. The key is
//! what its searches name it by.

use serde::Deserialize;
use serde_json::{Map, Value, json};
use validator::Validate;

use super::jira::{SignedSite, new_site, stored_site};
use super::{
    AccountChoice, ContentField, ContentFieldType, Definition, Provides, Source, parse_content,
    parse_new_account,
};
use crate::auth::CurrentUser;
use crate::confluence::{
    ChangeQuery, Confluence, ConfluenceError, ContentType, Listing, Space, SpaceScope,
};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::{AccountKind, ConnectorKind, Scope};
use crate::errors::ApiError;
use crate::jira::normalize_site_url;
use crate::models::connector::{self, Connector};
use crate::models::jira_credential::{self, Allowed, Allowlist, NewJiraCredential, VerifiedToken};
use crate::routes::v1::jira_credentials::{
    JiraToken, Selection, validate_email_address, validate_not_blank,
};
use crate::state::AppState;

/// How far back a sample looks: far enough that a site in use has something to show.
const SAMPLE_MINUTES: i64 = 90 * 24 * 60;

/// How many pages or comments a sample reads: enough to prove the search works.
const SAMPLE_SIZE: usize = 5;

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Confluence,
    scope: Scope::Workspace,
    account: AccountKind::Atlassian,
    provides: &[Provides::Pages],
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

/// A new Confluence connector's Atlassian account, as the client sends it: the fields Jira's
/// takes.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewAccount {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: String,
    site_url: String,
    #[validate(custom(function = "validate_email_address"))]
    account_email: String,
    token: JiraToken,
}

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

/// The error a request answers when Confluence refused or could not be reached.
fn api_error(error: ConfluenceError) -> ApiError {
    match error {
        ConfluenceError::Unauthorized(message) => ApiError::BadRequest(message.to_owned()),
        ConfluenceError::Invalid(message) => ApiError::BadRequest(message),
        ConfluenceError::Refused(message) => ApiError::BadGateway(message),
    }
}

/// The spaces `site`'s token reaches now.
///
/// # Errors
/// As [`api_error`].
async fn list_spaces(
    confluence: &Confluence,
    site: &SignedSite,
) -> Result<Listing<Space>, ApiError> {
    confluence
        .list_spaces(&site.site())
        .await
        .map_err(api_error)
}

/// The scope `selection` resolves to through `site`, refusing a pick the token no longer
/// reaches.
///
/// # Errors
/// [`ApiError::BadRequest`] naming each such pick, and whatever Confluence answered.
async fn chosen_scope(
    confluence: &Confluence,
    site: &SignedSite,
    selection: &Selection<String>,
) -> Result<SpaceScope, ApiError> {
    let listing = list_spaces(confluence, site).await?;
    resolve(selection, &listing.items).map_err(|missing| {
        ApiError::BadRequest(format!(
            "the token does not reach the space {}",
            missing.join(", ")
        ))
    })
}

/// The site a stored Confluence connector signs in to.
///
/// # Errors
/// [`ApiError::Internal`] for a connector without its account, which the API never stores.
async fn site_of(state: &AppState, stored: &Connector) -> Result<SignedSite, ApiError> {
    let id = stored.jira_credential_id.ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "a Confluence connector without its Atlassian account"
        ))
    })?;
    Ok(stored_site(state, id).await?.0)
}

pub async fn prepare_checks(
    state: &AppState,
    source: Source<'_>,
) -> Result<ConfluenceChecks, ApiError> {
    let (site, selection) = match source {
        Source::Stored(stored) => (
            site_of(state, stored).await?,
            as_selection(&stored_scope(stored)?),
        ),
        Source::Draft { account, content } => {
            let AccountChoice::New { new } = account else {
                return Err(ApiError::BadRequest(
                    "a Confluence connector is given its own Atlassian account".to_owned(),
                ));
            };
            let content: Content = parse_content(ConnectorKind::Confluence, content)?;
            (new_site(new)?, content.spaces)
        }
    };
    Ok(ConfluenceChecks {
        confluence: state.connectors.confluence.confluence().clone(),
        site,
        selection,
        scope: None,
    })
}

/// Confluence's checks against one Atlassian account and selection.
#[derive(Debug)]
pub struct ConfluenceChecks {
    confluence: Confluence,
    site: SignedSite,
    selection: Selection<String>,
    /// The spaces the selection resolves to, once `list_spaces` passes.
    scope: Option<SpaceScope>,
}

impl Checks for ConfluenceChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "sign_in" => match self.confluence.verify(&self.site.site()).await {
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
        let listing = match self.confluence.list_spaces(&self.site.site()).await {
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
        match self.confluence.changes(&self.site.site(), &query).await {
            Ok(changes) => Outcome::counted(changes.items.len()),
            Err(error) => Outcome::failed(error.to_string()),
        }
    }
}

/// Creates a Confluence connector with its new Atlassian account, `new`, once Confluence accepts
/// the token and reports every space picked. The account and the connector are stored together.
///
/// # Errors
/// [`ApiError::Validation`] for fields out of shape, [`ApiError::BadRequest`] for a site that is
/// not Confluence Cloud, a token Confluence refuses, or a space it does not reach, and
/// [`ApiError::Conflict`] for a name another Atlassian account has.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    new: Value,
    content: Value,
) -> Result<Connector, ApiError> {
    let fields: NewAccount = parse_new_account(AccountKind::Atlassian, new)?;
    fields.validate()?;
    let content: Content = parse_content(ConnectorKind::Confluence, content)?;
    let site_url = normalize_site_url(&fields.site_url)
        .map_err(|advice| ApiError::BadRequest(advice.to_owned()))?;
    let site = SignedSite {
        url: site_url,
        email: fields.account_email.trim().to_owned(),
        token: fields.token.0,
    };

    let confluence = state.connectors.confluence.confluence();
    // Stored only once Confluence accepts the token and reports the spaces, so no account
    // promises a reach that was never proven.
    let account = confluence.verify(&site.site()).await.map_err(api_error)?;
    let scope = chosen_scope(confluence, &site, &content.spaces).await?;
    let new_credential = NewJiraCredential {
        name: fields.name.trim().to_owned(),
        verified: VerifiedToken {
            site_url: site.url,
            account_email: site.email,
            token: site.token,
            account,
        },
        // Confluence's token is never Jira's: its account allows no project or board.
        allowed: Allowed {
            projects: Allowlist::Only(Vec::new()),
            boards: Allowlist::Only(Vec::new()),
        },
        created_by: current.id(),
    };

    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let (_credential, created) = jira_credential::create_carrying(
        &mut connection,
        &state.cipher,
        &new_credential,
        ConnectorKind::Confluence,
        json!({ "spaces": scope }),
    )
    .await
    .map_err(|error| match error {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => ApiError::Conflict("another Atlassian account has that name; choose another"),
        other => other.into(),
    })?;
    Ok(created)
}

/// What the spaces can be picked from, as the token reaches them now: a new account's, or the
/// account a stored connector owns.
///
/// # Errors
/// As [`create`], for the account.
pub async fn options(state: &AppState, account: AccountChoice) -> Result<Value, ApiError> {
    let site = match account {
        AccountChoice::New { new } => new_site(new)?,
        AccountChoice::Existing { id } => stored_site(state, id).await?.0,
    };
    let listing = list_spaces(state.connectors.confluence.confluence(), &site).await?;
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
    let site = site_of(state, stored).await?;
    let scope = chosen_scope(
        state.connectors.confluence.confluence(),
        &site,
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

/// Removes the connector with its Atlassian account, which no other connector reads through.
///
/// # Errors
/// [`ApiError::NotFound`] when it was already removed.
pub async fn remove(state: &AppState, stored: &Connector) -> Result<(), ApiError> {
    let id = stored.jira_credential_id.ok_or(ApiError::NotFound)?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    // The connector goes with its account, by its foreign key.
    jira_credential::delete(&mut connection, id).await?;
    Ok(())
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
    fn a_new_account_takes_the_fields_jira_takes_and_nothing_else() {
        let fields = json!({
            "name": "Wiki",
            "siteUrl": "https://acme.atlassian.net",
            "accountEmail": "ada@acme.test",
            "token": "atlassian-token",
        });
        let parsed: NewAccount =
            parse_new_account(AccountKind::Atlassian, fields.clone()).expect("parses");
        parsed.validate().expect("valid");

        let mut with_projects = fields;
        with_projects["projects"] = json!("*");
        parse_new_account::<NewAccount>(AccountKind::Atlassian, with_projects)
            .expect_err("Confluence's account allows no Jira projects");
    }
}
