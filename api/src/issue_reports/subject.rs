// Copyright © 2026 Jalapeno Labs

//! What a report is about, and the [`Problem`] gathered for it.
//!
//! A subject names something Elysium already holds wherever it can: a connector's failed sync,
//! one of its stored checks, a check in a run someone is following. The error and everything
//! around it are then read from Elysium's own records, so a report says what the server saw,
//! with the context that helps (the other checks, recent syncs), and a person can only report
//! what they may see. Any other surface reports through [`Subject::Error`], naming its area and
//! the error it showed.

use std::collections::HashSet;

use diesel_async::AsyncPgConnection;
use serde::Deserialize;
use uuid::Uuid;

use super::draft::Problem;
use crate::connectors::accounts;
use crate::connectors::checks::{CheckProgress, CheckRuns, CheckState};
use crate::connectors::kinds::{self, is_visible};
use crate::connectors::{ConnectorKind, Scope};
use crate::errors::ApiError;
use crate::models::connector::{self, Connector};
use crate::models::connector_check;
use crate::models::connector_run;

/// How many of a connector's newest syncs a report looks through: for the one it names, and for
/// how often the connector has failed lately.
const RECENT_RUNS: i64 = 50;

/// How many recent syncs a report counts failures over.
const FAILURE_WINDOW: usize = 10;

/// How many earlier failures of a different kind a report lists.
const EARLIER_FAILURES_LISTED: usize = 3;

/// The longest error a [`Subject::Error`] may carry. Longer is not an error message.
const CLIENT_ERROR_MAX_CHARACTERS: usize = 4000;

/// The longest check id or area a subject may name.
const IDENTIFIER_MAX_CHARACTERS: usize = 64;

/// What a person is reporting.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Subject {
    /// A connector's failed sync: the run named, or its last error when none is.
    ConnectorSync {
        connector_id: Uuid,
        #[serde(default)]
        run_id: Option<Uuid>,
    },
    /// A check of a stored connector that failed the last time it ran.
    ConnectorCheck {
        connector_id: Uuid,
        check_id: String,
    },
    /// A check that failed in a run the person started, kept for half an hour after it ends:
    /// how a connector not saved yet is reported.
    CheckRun { run_id: Uuid, check_id: String },
    /// Any other error the app showed, named by where it was shown, such as
    /// `action_items.link_write`. The message is the person's to send; it is redacted like any
    /// other.
    Error { area: String, message: String },
}

impl Subject {
    /// Refuses a subject whose identifiers could not have come from the app.
    ///
    /// # Errors
    /// [`ApiError::BadRequest`] naming what is wrong.
    pub fn validate(&self) -> Result<(), ApiError> {
        match self {
            Self::ConnectorSync { .. } => Ok(()),
            Self::ConnectorCheck { check_id, .. } | Self::CheckRun { check_id, .. } => {
                if !is_identifier(check_id, false) {
                    return Err(ApiError::BadRequest(
                        "checkId must be a check's id, such as sign_in".to_owned(),
                    ));
                }
                Ok(())
            }
            Self::Error { area, message } => {
                if !is_identifier(area, true) {
                    return Err(ApiError::BadRequest(
                        "area must be lower-case words joined by dots or underscores, such as \
                         action_items.link_write"
                            .to_owned(),
                    ));
                }
                let length = message.trim().chars().count();
                if length == 0 || length > CLIENT_ERROR_MAX_CHARACTERS {
                    return Err(ApiError::BadRequest(format!(
                        "message must hold between 1 and {CLIENT_ERROR_MAX_CHARACTERS} characters"
                    )));
                }
                Ok(())
            }
        }
    }
}

/// Whether `value` is an id the app writes: lower-case letters, digits, and underscores, and
/// dots and hyphens too when `allow_dots` is set, starting with a letter.
fn is_identifier(value: &str, allow_dots: bool) -> bool {
    let starts_with_letter = value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_lowercase());
    let every_character_allowed = value.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '_'
            || (allow_dots && matches!(character, '.' | '-'))
    });
    starts_with_letter && every_character_allowed && value.len() <= IDENTIFIER_MAX_CHARACTERS
}

/// The problem `subject` names, read from what Elysium holds and what `person` may see.
///
/// # Errors
/// [`ApiError::NotFound`] for a connector or run the person cannot see, and
/// [`ApiError::BadRequest`] for one with nothing to report, such as a check that passed.
pub async fn gather(
    connection: &mut AsyncPgConnection,
    check_runs: &CheckRuns,
    person: Uuid,
    subject: &Subject,
) -> Result<Problem, ApiError> {
    match subject {
        Subject::ConnectorSync {
            connector_id,
            run_id,
        } => {
            let stored = visible(connection, *connector_id, person).await?;
            connector_sync(connection, &stored, *run_id).await
        }
        Subject::ConnectorCheck {
            connector_id,
            check_id,
        } => {
            let stored = visible(connection, *connector_id, person).await?;
            connector_check(connection, &stored, check_id).await
        }
        Subject::CheckRun { run_id, check_id } => {
            let run = check_runs.get(*run_id, person).ok_or(ApiError::NotFound)?;
            let failed = run
                .checks
                .iter()
                .find(|progress| progress.check_id == check_id)
                .filter(|progress| progress.outcome.state == CheckState::Failed)
                .ok_or_else(|| {
                    ApiError::BadRequest("that check did not fail in this run".to_owned())
                })?;
            let setting = if run.connector_id.is_some() {
                "a saved connector"
            } else {
                "a new connector, before it was saved"
            };
            let related = run
                .checks
                .iter()
                .filter(|progress| progress.check_id != check_id)
                .map(describe_progress)
                .collect();
            Ok(check_problem(&CheckFailure {
                kind: run.kind,
                check_id: failed.check_id,
                reason: failed.outcome.reason,
                message: failed.outcome.message.as_deref(),
                setting,
                related,
            }))
        }
        Subject::Error { area, message } => Ok(Problem {
            category: format!("app:{area}"),
            summary: format!("Elysium showed an error in `{area}`."),
            error: message.clone(),
            facts: vec![("Area", area.clone())],
            related: Vec::new(),
        }),
    }
}

/// The connector `id`, when `person` may see it.
async fn visible(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    person: Uuid,
) -> Result<Connector, ApiError> {
    let stored = connector::find(connection, id).await?;
    if !is_visible(&stored, person) {
        return Err(ApiError::NotFound);
    }
    Ok(stored)
}

/// A connector's failed sync, with how often it failed lately and its failing checks.
async fn connector_sync(
    connection: &mut AsyncPgConnection,
    stored: &Connector,
    run_id: Option<Uuid>,
) -> Result<Problem, ApiError> {
    let kind = kinds::kind_of(stored)?;
    let runs = connector_run::recent(connection, &[stored.id], RECENT_RUNS).await?;

    let error = match run_id {
        Some(run_id) => runs
            .iter()
            .find(|run| run.id == run_id)
            .ok_or(ApiError::NotFound)?
            .error
            .clone(),
        // Jira and GitHub are read by the link watcher, which keeps its errors per account.
        None => match stored.last_error.clone() {
            Some(error) => Some(error),
            None => accounts::watcher_passes(connection, std::slice::from_ref(stored))
                .await?
                .remove(&stored.id)
                .and_then(|pass| pass.last_error),
        },
    };
    let error = error.ok_or_else(|| {
        ApiError::BadRequest("that sync did not fail, so there is nothing to report".to_owned())
    })?;

    let window: Vec<&connector_run::ConnectorRun> = runs.iter().take(FAILURE_WINDOW).collect();
    let failures = window.iter().filter(|run| run.outcome == "failed").count();

    let mut related: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::from([error.as_str()]);
    for run in &runs {
        let Some(earlier) = run.error.as_deref() else {
            continue;
        };
        if related.len() == EARLIER_FAILURES_LISTED {
            break;
        }
        if seen.insert(earlier) {
            related.push(format!("Another recent sync failed: {earlier}"));
        }
    }
    for result in connector_check::list_for(connection, &[stored.id]).await? {
        if result.state != CheckState::Passed.as_str() {
            related.push(describe_stored(&result));
        }
    }

    let mut facts = vec![("Connector", kind.as_str().to_owned())];
    facts.append(&mut connector_state(kind, stored));
    facts.push((
        "Recent syncs",
        format!("{failures} of the last {} failed", window.len()),
    ));
    Ok(Problem {
        category: format!("connector:{}:sync", kind.as_str()),
        summary: format!("The `{}` connector's sync failed.", kind.as_str()),
        error,
        facts,
        related,
    })
}

/// A stored connector's check that failed the last time it ran.
async fn connector_check(
    connection: &mut AsyncPgConnection,
    stored: &Connector,
    check_id: &str,
) -> Result<Problem, ApiError> {
    let kind = kinds::kind_of(stored)?;
    let results = connector_check::list_for(connection, &[stored.id]).await?;
    let failed = results
        .iter()
        .find(|result| result.check_id == check_id && result.state == CheckState::Failed.as_str())
        .ok_or_else(|| {
            ApiError::BadRequest("that check did not fail the last time it ran".to_owned())
        })?;
    // The kind's own spelling of the id, so the category is written from Elysium's identifiers
    // alone, whatever the stored row holds.
    let spec = kinds::definition(kind)
        .checks
        .iter()
        .find(|spec| spec.id == check_id)
        .ok_or_else(|| ApiError::BadRequest(format!("{check_id} is not a check of this kind")))?;

    let related = results
        .iter()
        .filter(|result| result.check_id != check_id)
        .map(describe_stored)
        .collect();
    let mut problem = check_problem(&CheckFailure {
        kind,
        check_id: spec.id,
        // Elysium's own word for why, written by the check that failed, exactly as a run has it.
        reason: failed.reason.as_deref(),
        message: failed.message.as_deref(),
        setting: "a saved connector",
        related,
    });
    problem.facts.append(&mut connector_state(kind, stored));
    Ok(problem)
}

/// A failed check, wherever it ran.
struct CheckFailure<'a> {
    kind: ConnectorKind,
    check_id: &'static str,
    reason: Option<&'a str>,
    message: Option<&'a str>,
    /// Where it ran, in words: a saved connector, or one being set up.
    setting: &'static str,
    related: Vec<String>,
}

/// The problem a failed check makes. A check's category is the same whether it failed on a
/// saved connector or one being set up, so both report to one issue.
fn check_problem(failure: &CheckFailure<'_>) -> Problem {
    let kind = failure.kind.as_str();
    let mut category = format!("connector:{kind}:check:{}", failure.check_id);
    if let Some(reason) = failure.reason {
        category = format!("{category}:{reason}");
    }
    let required = kinds::definition(failure.kind)
        .checks
        .iter()
        .find(|spec| spec.id == failure.check_id)
        .is_some_and(|spec| spec.required);

    let mut facts = vec![
        ("Connector", kind.to_owned()),
        (
            "Check",
            format!(
                "{}, {}",
                failure.check_id,
                if required { "required" } else { "optional" }
            ),
        ),
        ("Checked on", failure.setting.to_owned()),
    ];
    if let Some(reason) = failure.reason {
        facts.push(("Reason", reason.to_owned()));
    }
    let error = failure
        .message
        .map(str::to_owned)
        .or_else(|| failure.reason.map(str::to_owned))
        .unwrap_or_else(|| "The check failed without saying why.".to_owned());

    Problem {
        category,
        summary: format!(
            "The `{kind}` connector's `{}` check failed.",
            failure.check_id
        ),
        error,
        facts,
        related: failure.related.clone(),
    }
}

/// How a stored connector stands: whose it is, its sign-in, and whether it is read.
fn connector_state(kind: ConnectorKind, stored: &Connector) -> Vec<(&'static str, String)> {
    let scope = match kinds::definition(kind).scope {
        Scope::Workspace => "workspace",
        Scope::Personal => "personal",
    };
    let reading = if stored.is_active { "active" } else { "paused" };
    vec![
        ("Scope", scope.to_owned()),
        ("Sign-in", stored.sign_in_state.clone()),
        ("Reading", reading.to_owned()),
    ]
}

/// One check in a run, as a line of a report.
fn describe_progress(progress: &CheckProgress) -> String {
    describe_check(
        progress.check_id,
        progress.outcome.state.as_str(),
        progress.outcome.reason,
        progress.outcome.blocked_by,
        progress.outcome.message.as_deref(),
    )
}

/// One stored check result, as a line of a report.
fn describe_stored(result: &connector_check::CheckResult) -> String {
    describe_check(
        &result.check_id,
        &result.state,
        result.reason.as_deref(),
        result.blocked_by.as_deref(),
        result.message.as_deref(),
    )
}

/// `sign_in: failed (fine_grained_scopes): GitHub: Bad credentials`, or
/// `list_projects: skipped, blocked by sign_in`.
fn describe_check(
    check_id: &str,
    state: &str,
    reason: Option<&str>,
    blocked_by: Option<&str>,
    message: Option<&str>,
) -> String {
    let reason = reason
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();
    let blocked_by = blocked_by
        .map(|blocked_by| format!(", blocked by {blocked_by}"))
        .unwrap_or_default();
    // Only a failure's message says what went wrong; a passing check's says what it found.
    let message = message
        .filter(|_message| state == CheckState::Failed.as_str())
        .map(|message| format!(": {message}"))
        .unwrap_or_default();
    format!("{check_id}: {state}{reason}{blocked_by}{message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_are_read_from_the_shape_the_app_sends() {
        let subject: Subject = serde_json::from_value(serde_json::json!({
            "kind": "connectorCheck",
            "connectorId": "0199a3c4-0000-7000-8000-000000000001",
            "checkId": "sign_in",
        }))
        .expect("parses");
        assert!(
            matches!(subject, Subject::ConnectorCheck { ref check_id, .. } if check_id == "sign_in")
        );

        let sync: Subject = serde_json::from_value(serde_json::json!({
            "kind": "connectorSync",
            "connectorId": "0199a3c4-0000-7000-8000-000000000001",
        }))
        .expect("a run is optional");
        assert!(matches!(sync, Subject::ConnectorSync { run_id: None, .. }));

        serde_json::from_value::<Subject>(serde_json::json!({
            "kind": "error",
            "area": "x",
            "message": "y",
            "extra": true,
        }))
        .expect_err("unknown fields are refused");
    }

    #[test]
    fn identifiers_are_checked_before_anything_is_read() {
        let check = |check_id: &str| Subject::CheckRun {
            run_id: Uuid::nil(),
            check_id: check_id.to_owned(),
        };
        check("sign_in").validate().expect("a check id");
        check("Sign In")
            .validate()
            .expect_err("spaces and capitals");
        check("../x").validate().expect_err("a path");

        let error = |area: &str, message: &str| Subject::Error {
            area: area.to_owned(),
            message: message.to_owned(),
        };
        error("action_items.link_write", "Jira refused")
            .validate()
            .expect("an area and a message");
        error("action items", "x").validate().expect_err("a space");
        error("a", "   ").validate().expect_err("a blank message");
        error("a", &"x".repeat(CLIENT_ERROR_MAX_CHARACTERS + 1))
            .validate()
            .expect_err("a message too long");
    }

    #[test]
    fn a_failed_check_reports_to_the_same_category_wherever_it_ran() {
        let failure = |setting: &'static str| CheckFailure {
            kind: ConnectorKind::Github,
            check_id: "sign_in",
            reason: None,
            message: Some("GitHub: Bad credentials"),
            setting,
            related: Vec::new(),
        };
        let saved = check_problem(&failure("a saved connector"));
        let draft = check_problem(&failure("a new connector, before it was saved"));
        assert_eq!(saved.category, "connector:github:check:sign_in");
        assert_eq!(saved.category, draft.category);
        assert_eq!(saved.error, draft.error);
        assert_eq!(
            saved.summary,
            "The `github` connector's `sign_in` check failed."
        );
    }

    #[test]
    fn a_check_without_a_message_reports_its_reason() {
        let problem = check_problem(&CheckFailure {
            kind: ConnectorKind::Github,
            check_id: "read_scopes",
            reason: Some("fine_grained_scopes"),
            message: None,
            setting: "a saved connector",
            related: Vec::new(),
        });
        assert_eq!(problem.error, "fine_grained_scopes");
        assert_eq!(
            problem.category,
            "connector:github:check:read_scopes:fine_grained_scopes"
        );
        assert!(
            problem
                .facts
                .contains(&("Check", "read_scopes, optional".to_owned()))
        );
    }

    #[test]
    fn checks_read_as_one_line_each() {
        assert_eq!(
            describe_check("list_projects", "skipped", None, Some("sign_in"), None),
            "list_projects: skipped, blocked by sign_in"
        );
        assert_eq!(
            describe_check(
                "sign_in",
                "failed",
                Some("bad_token"),
                None,
                Some("Jira: 401")
            ),
            "sign_in: failed (bad_token): Jira: 401"
        );
        assert_eq!(
            describe_check("sample_issues", "passed", None, None, Some("ignored")),
            "sample_issues: passed"
        );
    }
}
