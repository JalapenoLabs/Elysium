// Copyright © 2026 Jalapeno Labs

//! `/api/v1/jira-credentials`: the Jira Cloud credentials Elysium holds, and the issues
//! they reach.
//!
//! Tokens are write-only over HTTP, and every write checks the token with Jira before it is
//! stored. What a credential may touch is chosen from what Jira reports, never typed, and
//! every call is bounded by that choice in `allowlist.rs`.

mod add_comment;
pub mod allowlist;
mod apply_transition;
mod create_issue;
mod create_jira_credential;
mod delete_jira_credential;
mod discover_jira_site;
mod get_issue;
mod get_jira_credential;
mod list_boards;
mod list_jira_credentials;
mod list_projects;
mod list_transitions;
mod search_issues;
mod test_jira_credential;
mod update_issue;
mod update_jira_credential;

use anyhow::Context;
use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::errors::ApiError;
use crate::jira::{Account, Board, Jira, JiraError, Project, Site};
use crate::models::jira_credential::{
    self, Allowed, AllowedBoard, AllowedProject, Allowlist, JiraCredential,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_jira_credentials::handle).post(create_jira_credential::handle),
        )
        .route("/discover", post(discover_jira_site::handle))
        .route(
            "/{id}",
            get(get_jira_credential::handle)
                .patch(update_jira_credential::handle)
                .delete(delete_jira_credential::handle),
        )
        .route("/{id}/test", post(test_jira_credential::handle))
        .route("/{id}/projects", get(list_projects::handle))
        .route("/{id}/boards", get(list_boards::handle))
        .route(
            "/{id}/issues",
            get(search_issues::handle).post(create_issue::handle),
        )
        .route(
            "/{id}/issues/{key}",
            get(get_issue::handle).patch(update_issue::handle),
        )
        .route(
            "/{id}/issues/{key}/transitions",
            get(list_transitions::handle).post(apply_transition::handle),
        )
        .route("/{id}/issues/{key}/comments", post(add_comment::handle))
}

/// Upper bound on a stored token. An Atlassian API token is about 200 characters today;
/// this leaves room for longer ones without letting a row carry an arbitrary blob.
const TOKEN_MAX_BYTES: usize = 512;

/// A credential as clients see it. The token is never included, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraCredentialResponse {
    id: Uuid,
    name: String,
    /// The site's origin, such as `https://acme.atlassian.net`.
    site_url: String,
    /// The account the token belongs to, which signs in with it.
    account_email: String,
    /// Atlassian's id for that account, as Jira reported it.
    account_id: String,
    display_name: String,
    /// `"*"` for every project, or the projects this credential may touch.
    projects: Allowlist<AllowedProject>,
    /// `"*"` for every board, or the boards this credential may touch.
    boards: Allowlist<AllowedBoard>,
    /// When Jira last confirmed the token.
    checked_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl JiraCredentialResponse {
    pub fn new(credential: JiraCredential, allowed: Allowed) -> Self {
        Self {
            id: credential.id,
            name: credential.name,
            site_url: credential.site_url,
            account_email: credential.account_email,
            account_id: credential.account_id,
            display_name: credential.display_name,
            projects: allowed.projects,
            boards: allowed.boards,
            checked_at: credential.checked_at,
            created_at: credential.created_at,
            updated_at: credential.updated_at,
        }
    }
}

/// A token from a request body, checked as it is parsed.
///
/// Validation happens during deserialization rather than through `validator`, because a
/// validator error report would need to serialize the value itself. Surrounding whitespace
/// is dropped, since a token copied out of Atlassian's dialog often brings some and never
/// contains any.
#[derive(Debug)]
pub struct JiraToken(pub SecretString);

impl<'de> Deserialize<'de> for JiraToken {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let token = String::deserialize(deserializer)?;
        let token = token.trim();

        if token.is_empty() {
            return Err(serde::de::Error::custom("token must not be blank"));
        }
        if token.len() > TOKEN_MAX_BYTES {
            return Err(serde::de::Error::custom("token exceeds 512 characters"));
        }

        Ok(Self(SecretString::from(token)))
    }
}

/// What a client picked: everything of a kind, or the entries it named.
///
/// Projects are named by their Jira id or their key, boards by their id. Nothing is taken
/// on trust: a selection is matched against what Jira reports the token can reach, and what
/// is stored is what Jira called it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection<Reference> {
    All,
    Only(Vec<Reference>),
}

impl<'de, Reference: Deserialize<'de>> Deserialize<'de> for Selection<Reference> {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire<Reference> {
            Named(Vec<Reference>),
            Everything(String),
        }

        match Wire::deserialize(deserializer) {
            Ok(Wire::Named(references)) => Ok(Self::Only(references)),
            Ok(Wire::Everything(text)) if text == "*" => Ok(Self::All),
            _ => Err(serde::de::Error::custom(
                "a selection is \"*\" or a list of what Jira reported",
            )),
        }
    }
}

/// Rejects names that are only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}

/// Rejects anything that is not an email address, which is the username half of basic auth.
///
/// Addresses are not parsed further than Jira needs: Jira decides whose account it is.
fn validate_email_address(value: &str) -> Result<(), ValidationError> {
    let address = value.trim();
    let is_address = match address.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty() && domain.contains('.') && !domain.starts_with('.')
        }
        None => false,
    };
    if !is_address || address.chars().any(char::is_whitespace) {
        return Err(ValidationError::new("email")
            .with_message("must be the email address of an Atlassian account".into()));
    }
    Ok(())
}

/// What a token can reach, as Jira reports it right now.
#[derive(Debug)]
pub struct Reachable {
    pub account: Account,
    pub projects: Vec<Project>,
    pub boards: Vec<Board>,
    /// Whether Jira had more projects than one listing reads.
    pub projects_truncated: bool,
    pub boards_truncated: bool,
}

/// Checks a token with Jira and reads everything a selection can be made from.
///
/// This is step one of creating a credential, and it runs again on every write, so a stored
/// selection is never one Jira did not confirm.
///
/// # Errors
/// Propagates whatever Jira answered; see [`JiraError`].
pub async fn reach(jira: &Jira, site: &Site<'_>) -> Result<Reachable, JiraError> {
    let account = jira.verify(site).await?;
    let projects = jira.list_projects(site).await?;
    let boards = jira.list_boards(site).await?;

    Ok(Reachable {
        account,
        projects: projects.items,
        boards: boards.items,
        projects_truncated: projects.truncated,
        boards_truncated: boards.truncated,
    })
}

impl Reachable {
    /// The projects a client picked, as Jira spells them.
    ///
    /// A selection is matched by Jira's own id or by the project key, in any case, since
    /// both are what a picker has to hand. Duplicates collapse, and the result is ordered by
    /// key so a stored selection reads the same everywhere.
    ///
    /// # Errors
    /// Returns [`ApiError::BadRequest`] naming the first pick Jira does not report for this
    /// token, since storing it would promise access the token does not have.
    pub fn choose_projects(
        &self,
        selection: &Selection<String>,
    ) -> Result<Allowlist<AllowedProject>, ApiError> {
        let Selection::Only(references) = selection else {
            return Ok(Allowlist::All);
        };

        let mut chosen: Vec<AllowedProject> = Vec::with_capacity(references.len());
        for reference in references {
            let reference = reference.trim();
            let found = self.projects.iter().find(|project| {
                project.id == reference || project.key.eq_ignore_ascii_case(reference)
            });
            let Some(project) = found else {
                return Err(ApiError::BadRequest(format!(
                    "Jira does not report a project {reference} for this token"
                )));
            };
            if chosen.iter().any(|picked| picked.id == project.id) {
                continue;
            }
            chosen.push(AllowedProject {
                id: project.id.clone(),
                key: project.key.clone(),
                name: project.name.clone(),
            });
        }

        chosen.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        Ok(Allowlist::Only(chosen))
    }

    /// The boards a client picked, as Jira spells them.
    ///
    /// # Errors
    /// As [`Reachable::choose_projects`].
    pub fn choose_boards(
        &self,
        selection: &Selection<i64>,
    ) -> Result<Allowlist<AllowedBoard>, ApiError> {
        let Selection::Only(references) = selection else {
            return Ok(Allowlist::All);
        };

        let mut chosen: Vec<AllowedBoard> = Vec::with_capacity(references.len());
        for &reference in references {
            let Some(board) = self.boards.iter().find(|board| board.id == reference) else {
                return Err(ApiError::BadRequest(format!(
                    "Jira does not report a board {reference} for this token"
                )));
            };
            if chosen.iter().any(|picked| picked.id == board.id) {
                continue;
            }
            chosen.push(AllowedBoard {
                id: board.id,
                name: board.name.clone(),
                project_key: board.project_key.clone(),
            });
        }

        chosen.sort_unstable_by_key(|board| board.id);
        Ok(Allowlist::Only(chosen))
    }
}

/// A stored credential, opened: what it may touch, and the token to call its site with.
///
/// Every route that reaches Jira with a stored credential starts here, so the allowlist and
/// the token are always loaded together.
#[derive(Debug)]
pub struct OpenCredential {
    pub credential: JiraCredential,
    pub allowed: Allowed,
    token: SecretString,
}

impl OpenCredential {
    /// Where this credential's calls go, and who they go as.
    pub fn site(&self) -> Site<'_> {
        Site {
            url: &self.credential.site_url,
            email: &self.credential.account_email,
            token: &self.token,
        }
    }
}

/// Loads one credential, what it may touch, and its decrypted token.
///
/// # Errors
/// Returns [`ApiError::NotFound`] when no credential has that id, and
/// [`ApiError::Internal`] when the stored token cannot be decrypted, which refuses the call
/// rather than sending Jira something meaningless.
pub async fn open(state: &AppState, id: Uuid) -> Result<OpenCredential, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential = jira_credential::find(&mut connection, id).await?;
    let allowed = jira_credential::allowed_of(&mut connection, &credential).await?;
    drop(connection);

    let token = credential
        .token(&state.cipher)
        .context("the stored token cannot be decrypted")?;
    Ok(OpenCredential {
        credential,
        allowed,
        token,
    })
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;
    use serde_json::json;

    use super::*;

    fn reachable() -> Reachable {
        Reachable {
            account: Account {
                account_id: "5b10".to_owned(),
                display_name: "Alex".to_owned(),
                email: None,
            },
            projects: vec![
                Project {
                    id: "10001".to_owned(),
                    key: "OPS".to_owned(),
                    name: "Operations".to_owned(),
                },
                Project {
                    id: "10002".to_owned(),
                    key: "ELY".to_owned(),
                    name: "Elysium".to_owned(),
                },
            ],
            boards: vec![Board {
                id: 12,
                name: "ELY board".to_owned(),
                project_key: Some("ELY".to_owned()),
            }],
            projects_truncated: false,
            boards_truncated: false,
        }
    }

    #[test]
    fn a_selection_is_the_wildcard_or_a_list() {
        let everything: Selection<String> = serde_json::from_value(json!("*")).expect("parses");
        assert_eq!(everything, Selection::All);

        let listed: Selection<String> =
            serde_json::from_value(json!(["ELY", "10001"])).expect("parses");
        assert_eq!(
            listed,
            Selection::Only(vec!["ELY".to_owned(), "10001".to_owned()])
        );

        let boards: Selection<i64> = serde_json::from_value(json!([12, 13])).expect("parses");
        assert_eq!(boards, Selection::Only(vec![12, 13]));

        serde_json::from_value::<Selection<String>>(json!("all")).expect_err("another word");
        serde_json::from_value::<Selection<i64>>(json!(["12"]))
            .expect_err("a board id is a number");
    }

    #[test]
    fn picks_are_matched_by_id_or_key_and_stored_as_jira_spells_them() {
        let reachable = reachable();

        let by_key = reachable
            .choose_projects(&Selection::Only(vec!["ely".to_owned()]))
            .expect("chosen");
        assert_eq!(
            by_key,
            Allowlist::Only(vec![AllowedProject {
                id: "10002".to_owned(),
                key: "ELY".to_owned(),
                name: "Elysium".to_owned(),
            }]),
            "a key in any case picks the project, stored the way Jira spells it"
        );

        let both = reachable
            .choose_projects(&Selection::Only(vec![
                "10001".to_owned(),
                "ELY".to_owned(),
                "10002".to_owned(),
            ]))
            .expect("chosen");
        let Allowlist::Only(projects) = both else {
            panic!("a list stays a list");
        };
        let keys: Vec<&str> = projects
            .iter()
            .map(|project| project.key.as_str())
            .collect();
        assert_eq!(keys, ["ELY", "OPS"], "ordered by key, and named twice once");

        assert_eq!(
            reachable
                .choose_projects(&Selection::All)
                .expect("every project"),
            Allowlist::All
        );
    }

    #[test]
    fn a_pick_jira_does_not_report_is_refused_by_name() {
        let reachable = reachable();

        let refused = reachable
            .choose_projects(&Selection::Only(vec!["SECRET".to_owned()]))
            .unwrap_err();
        let ApiError::BadRequest(message) = refused else {
            panic!("an unknown project is a bad request");
        };
        assert!(message.contains("SECRET"), "{message}");

        let refused = reachable
            .choose_boards(&Selection::Only(vec![99]))
            .unwrap_err();
        let ApiError::BadRequest(message) = refused else {
            panic!("an unknown board is a bad request");
        };
        assert!(message.contains("99"), "{message}");

        assert_eq!(
            reachable
                .choose_boards(&Selection::Only(vec![12, 12]))
                .expect("chosen"),
            Allowlist::Only(vec![AllowedBoard {
                id: 12,
                name: "ELY board".to_owned(),
                project_key: Some("ELY".to_owned()),
            }])
        );
    }

    #[test]
    fn tokens_are_trimmed_bounded_and_never_shown() {
        let parsed: JiraToken = serde_json::from_value(json!("  ATATT-padded  ")).expect("parses");
        assert_eq!(parsed.0.expose_secret(), "ATATT-padded");
        assert!(!format!("{parsed:?}").contains("padded"));

        serde_json::from_value::<JiraToken>(json!("   ")).expect_err("a blank token");
        serde_json::from_value::<JiraToken>(json!("t".repeat(513)))
            .expect_err("an oversized token");
    }

    #[test]
    fn email_addresses_are_checked_before_a_call_is_spent() {
        for address in ["alex@example.com", "alex+jira@sub.example.co.uk"] {
            validate_email_address(address).expect(address);
        }
        for refused in [
            "alex",
            "alex@example",
            "alex@.com",
            "@example.com",
            "a b@c.com",
        ] {
            validate_email_address(refused).expect_err(refused);
        }
    }
}
