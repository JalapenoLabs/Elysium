// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/coding-sessions`: open a thread on a satellite and record it.
//!
//! The session id is generated first and sent as the satellite's idempotency key, so
//! the thread and the row share an identity. If recording the row fails, the thread
//! is destroyed rather than left running with nothing pointing at it.

use std::collections::BTreeMap;

use anyhow::Context;
use arsox_sdk::proto::settings::v1::Repo;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::Utc;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use super::github_token::{self, SessionChoice};
use super::model_stack;
use super::{CodingSessionResponse, thread_settings, validate_not_blank};
use crate::crypto::Cipher;
use crate::errors::ApiError;
use crate::fleet::views::ThreadStatus;
use crate::fleet::{MANAGED_METADATA_KEY, SESSION_METADATA_KEY};
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::llm::{self, Llm};
use crate::models::{project, satellite};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    project_id: Uuid,
    satellite_id: Uuid,
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    title: String,
    /// Git URL the satellite clones into the thread's workspace.
    #[validate(length(max = 2048), custom(function = "validate_repository_url"))]
    repository_url: Option<String>,
    #[validate(length(min = 1, max = 255), custom(function = "validate_not_blank"))]
    base_branch: Option<String>,
    /// The GitHub token the agent works with: absent follows the project, `null` asks for
    /// none, and an id names one.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and an id are three distinct requests"
    )]
    github_credential_id: Option<Option<Uuid>>,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // Both parents are looked up before a thread exists: an unknown id answers 404
    // here instead of opening a thread the insert would then refuse.
    let project = project::find(&mut connection, body.project_id).await?;
    let satellite = satellite::find(&mut connection, body.satellite_id).await?;
    let credentials = llm::list(&mut connection).await?;

    let github = github_token::open(
        &mut connection,
        &state.cipher,
        &project,
        SessionChoice::from_request(body.github_credential_id),
    )
    .await?;
    drop(connection);

    let repository = body.repository_url.as_deref().map(|url| Repo {
        name: repository_directory(url)
            .expect("validation guarantees a directory name")
            .to_owned(),
        url: url.to_owned(),
        base_branch: body.base_branch.clone(),
        auth: github
            .as_ref()
            .and_then(|github| github_token::clone_auth(url, &github.token)),
        ..Repo::default()
    });

    let stack = model_stack::build(&open_credentials(credentials, &state.cipher), Utc::now());

    let client = state.fleet.client(satellite.id).await?;
    let session_id = Uuid::now_v7();
    let metadata = BTreeMap::from([
        (MANAGED_METADATA_KEY.to_owned(), "true".to_owned()),
        (SESSION_METADATA_KEY.to_owned(), session_id.to_string()),
    ]);
    let created = client
        .threads()
        .create_with(
            thread_settings(
                repository,
                stack,
                github.as_ref().map(|github| &github.token),
            ),
            Some(session_id.to_string()),
            metadata,
        )
        .await?;

    let new_session = NewCodingSession {
        id: session_id,
        project_id: project.id,
        satellite_id: satellite.id,
        thread_id: created.thread.thread_id.clone(),
        title: body.title,
        github_credential_id: github.map(|github| github.credential_id),
    };
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = match coding_session::create(&mut connection, &new_session).await {
        Ok(session) => session,
        Err(database_error) => {
            if let Err(destroy_error) = created.handle.destroy().await {
                event!(
                    name: "coding_session.create.orphaned_thread",
                    Level::ERROR,
                    satellite.id = %satellite.id,
                    thread.id = %created.thread.thread_id,
                    error.message = %destroy_error,
                    "could not record the session or destroy its thread; the thread expires on its idle TTL",
                );
            }
            return Err(database_error.into());
        }
    };

    let thread = ThreadStatus::from(&created.thread);
    state.fleet.watch_session(session.clone());
    state
        .events
        .publish(&ServerEvent::SessionUpserted(CodingSessionResponse::new(
            session.clone(),
            Some(thread.clone()),
        )));

    Ok((
        StatusCode::CREATED,
        Json(json!({ "session": CodingSessionResponse::new(session, Some(thread)) })),
    ))
}

/// Decrypts the model credentials a thread fails over through.
///
/// Decrypting here rather than in the stack keeps the cipher out of the shaping rules. A
/// credential that cannot be opened is skipped: the rest still run.
fn open_credentials(credentials: Vec<Llm>, cipher: &Cipher) -> Vec<(Llm, SecretString)> {
    let mut opened = Vec::with_capacity(credentials.len());
    for credential in credentials {
        match credential.secret_token(cipher) {
            Ok(token) => opened.push((credential, token)),
            Err(error) => event!(
                name: "coding_session.credential.unreadable",
                Level::ERROR,
                llm.id = %credential.id,
                error.message = %error,
                "a stored credential could not be decrypted and was skipped",
            ),
        }
    }
    opened
}

/// The directory a repository is cloned into: the URL's last path segment without
/// `.git`. The satellite refuses names that could escape the workspace, so only
/// plain names are accepted.
fn repository_directory(url: &str) -> Option<&str> {
    let last_segment = url.trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last_segment.strip_suffix(".git").unwrap_or(last_segment);

    let is_plain = name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'));
    if name.is_empty() || name.starts_with('.') || !is_plain {
        return None;
    }
    Some(name)
}

fn validate_repository_url(url: &str) -> Result<(), ValidationError> {
    let has_git_scheme = ["https://", "http://", "ssh://", "git@"]
        .iter()
        .any(|scheme| url.starts_with(scheme));
    if !has_git_scheme {
        return Err(ValidationError::new("scheme")
            .with_message("must be an https, http, ssh, or git@ repository URL".into()));
    }
    if repository_directory(url).is_none() {
        return Err(ValidationError::new("name")
            .with_message("must end in a repository name such as org/repo.git".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_directories_come_from_the_last_path_segment() {
        assert_eq!(
            repository_directory("https://github.com/JalapenoLabs/Elysium.git"),
            Some("Elysium")
        );
        assert_eq!(
            repository_directory("git@github.com:JalapenoLabs/uikit.git"),
            Some("uikit")
        );
        assert_eq!(
            repository_directory("https://github.com/org/repo/"),
            Some("repo")
        );
        assert_eq!(repository_directory("https://github.com/org/.."), None);
        assert_eq!(
            repository_directory("https://example.com/"),
            Some("example.com")
        );
        assert_eq!(repository_directory("https://example.com/a b"), None);
    }

    #[test]
    fn bodies_validate_titles_and_repository_urls() {
        let valid: RequestBody = serde_json::from_value(json!({
            "projectId": Uuid::nil(),
            "satelliteId": Uuid::nil(),
            "title": "Fix the login bug",
            "repositoryUrl": "https://github.com/JalapenoLabs/Elysium.git",
            "baseBranch": "main",
        }))
        .expect("parses");
        valid.validate().expect("valid body");

        let blank_title: RequestBody = serde_json::from_value(
            json!({ "projectId": Uuid::nil(), "satelliteId": Uuid::nil(), "title": "  " }),
        )
        .expect("parses");
        assert!(
            blank_title
                .validate()
                .expect_err("blank title")
                .field_errors()
                .contains_key("title")
        );

        let bad_repository: RequestBody = serde_json::from_value(json!({
            "projectId": Uuid::nil(),
            "satelliteId": Uuid::nil(),
            "title": "A",
            "repositoryUrl": "file:///etc/passwd",
        }))
        .expect("parses");
        assert!(
            bad_repository
                .validate()
                .expect_err("file URLs are refused")
                .field_errors()
                .contains_key("repository_url")
        );
    }

    #[test]
    fn new_threads_declare_the_ceilings_the_satellite_requires() {
        let settings = thread_settings(None, None, None);
        assert!(settings.idle_ttl.is_some());
        let budget = settings.budget.expect("budget is set");
        assert!(budget.max_tokens_per_turn.is_some());
        assert!(budget.max_cost_per_thread.is_some());
        assert!(budget.max_wall_clock_per_turn.is_some());
        assert!(settings.repos.is_empty());
        assert!(settings.env.is_empty() && settings.github.is_none());
    }

    #[test]
    fn github_credential_ids_distinguish_absent_null_and_an_id() {
        let base = json!({ "projectId": Uuid::nil(), "satelliteId": Uuid::nil(), "title": "A" });

        let absent: RequestBody = serde_json::from_value(base.clone()).expect("parses");
        assert_eq!(absent.github_credential_id, None);

        let mut null = base.clone();
        null["githubCredentialId"] = Value::Null;
        let null: RequestBody = serde_json::from_value(null).expect("parses");
        assert_eq!(null.github_credential_id, Some(None));

        let mut named = base;
        named["githubCredentialId"] = json!(Uuid::nil());
        let named: RequestBody = serde_json::from_value(named).expect("parses");
        assert_eq!(named.github_credential_id, Some(Some(Uuid::nil())));
    }
}
