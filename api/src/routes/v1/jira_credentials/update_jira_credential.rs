// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/jira-credentials/{id}`: rename a credential, give it a new token, move it
//! to another site, or change what it may touch.
//!
//! A token belongs to one account on one site, and a selection belongs to one site, so this
//! route's rules are about keeping those three true together:
//!
//! - a different site or account email needs the token that goes with it,
//! - a different site needs its projects and boards again, since the old ones named projects
//!   on the old site,
//! - anything that changes a selection is checked against Jira before it is stored, exactly
//!   as creating one is.
//!
//! A rename alone needs none of that and calls Jira not at all.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{
    JiraCredentialResponse, JiraToken, Selection, open, reach, validate_email_address,
    validate_not_blank,
};
use crate::errors::ApiError;
use crate::jira::{Site, normalize_site_url};
use crate::models::jira_credential::{self, JiraCredentialChanges, VerifiedToken};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Absent fields stay as they are.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: Option<String>,
    site_url: Option<String>,
    #[validate(custom(function = "validate_email_address"))]
    account_email: Option<String>,
    token: Option<JiraToken>,
    projects: Option<Selection<String>>,
    boards: Option<Selection<i64>>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    if body.name.is_none()
        && body.site_url.is_none()
        && body.account_email.is_none()
        && body.token.is_none()
        && body.projects.is_none()
        && body.boards.is_none()
    {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }

    let stored = open(&state, id).await?;
    let site_url = match &body.site_url {
        Some(site_url) => normalize_site_url(site_url)
            .map_err(|advice| ApiError::BadRequest(advice.to_owned()))?,
        None => stored.credential.site_url.clone(),
    };
    let account_email = body.account_email.as_ref().map_or_else(
        || stored.credential.account_email.clone(),
        |address| address.trim().to_owned(),
    );

    let moved_site = site_url != stored.credential.site_url;
    let moved_account = account_email != stored.credential.account_email;
    if (moved_site || moved_account) && body.token.is_none() {
        return Err(ApiError::BadRequest(
            "a different site or account email needs the API token that goes with it".to_owned(),
        ));
    }
    if moved_site && (body.projects.is_none() || body.boards.is_none()) {
        return Err(ApiError::BadRequest(
            "a different site needs its projects and boards again, since the stored ones name \
             projects on the old site"
                .to_owned(),
        ));
    }

    // A rename changes nothing Jira knows about, so it spends no call. Anything else is
    // checked, and the picks are matched against what Jira reports for the token that will
    // be stored.
    let checks_with_jira = body.token.is_some() || body.projects.is_some() || body.boards.is_some();
    let mut changes = JiraCredentialChanges {
        name: body.name,
        ..JiraCredentialChanges::default()
    };
    if checks_with_jira {
        let token = body
            .token
            .map_or_else(|| stored.token.clone(), |supplied| supplied.0);
        let reachable = reach(
            &state.jira,
            &Site {
                url: &site_url,
                email: &account_email,
                token: &token,
            },
        )
        .await?;

        if let Some(projects) = &body.projects {
            changes.projects = Some(reachable.choose_projects(projects)?);
        }
        if let Some(boards) = &body.boards {
            changes.boards = Some(reachable.choose_boards(boards)?);
        }
        // The token is re-sealed even when it is the stored one: it was just checked against
        // this site and account, which is exactly what the row records.
        changes.verified = Some(VerifiedToken {
            site_url,
            account_email,
            token,
            account: reachable.account,
        });
    }

    // A body naming only the site or the address it already has leaves nothing to write, so
    // it is refused rather than sent to Diesel as an empty changeset.
    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body changes nothing about this credential".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential = jira_credential::update(&mut connection, &state.cipher, id, &changes).await?;
    let allowed = jira_credential::allowed_of(&mut connection, &credential).await?;
    drop(connection);

    let response = JiraCredentialResponse::new(credential.clone(), allowed.clone());
    state
        .events
        .publish(&ServerEvent::JiraCredentialUpserted(response));

    Ok(Json(
        json!({ "credential": JiraCredentialResponse::new(credential, allowed) }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_is_optional_and_present_ones_are_validated() {
        let empty: RequestBody = serde_json::from_value(json!({})).expect("parses");
        empty
            .validate()
            .expect("an empty body has nothing to validate");

        let blank: RequestBody = serde_json::from_value(json!({ "name": " " })).expect("parses");
        assert!(
            blank
                .validate()
                .expect_err("a blank name")
                .field_errors()
                .contains_key("name")
        );

        let moved: RequestBody = serde_json::from_value(json!({
            "siteUrl": "https://other.atlassian.net",
            "projects": "*",
            "boards": [12],
        }))
        .expect("parses");
        moved.validate().expect("valid");
        assert_eq!(moved.boards, Some(Selection::Only(vec![12])));
    }

    #[test]
    fn a_token_alone_parses_so_the_handler_can_rotate_it() {
        let body: RequestBody =
            serde_json::from_value(json!({ "token": "ATATTrotated" })).expect("parses");

        assert!(body.token.is_some());
        assert!(body.site_url.is_none() && body.projects.is_none());
        assert!(!format!("{body:?}").contains("rotated"));
    }
}
