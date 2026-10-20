// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/coding-sessions`: start a session for a project.
//!
//! The session's number is reserved and its row recorded, then its repositories are cloned
//! into its directory (`crate::sessions`), and a prompt, when given, is queued as its first
//! turn. A session started from an action item must have one, and its first turn carries the
//! item's context ahead of it (see `first_turn`). If cloning or queuing fails, the directory
//! and the row are removed again, so a create either yields a session with its first turn
//! queued or nothing.
//!
//! A session clones any number of repositories up to [`MAX_REPOSITORIES`], each into its own
//! directory under `repos/`. Two repositories that would share a directory are refused here,
//! while the caller is still listening, rather than failing a clone minutes later.

use std::collections::hash_map::{Entry, HashMap};

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use super::github_token::{self, SessionChoice};
use super::{CodingSessionResponse, first_turn, validate_not_blank};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::github::Repository;
use crate::models::coding_session::{self, CodingSession, NewCodingSession};
use crate::models::project;
use crate::realtime::ServerEvent;
use crate::sessions::{RepositoryClone, TurnExtras};
use crate::state::AppState;

/// A coding session to start for a project.
#[derive(Debug, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateCodingSessionRequest {
    /// The project the session works for.
    project_id: Uuid,
    /// What the session is called.
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    title: String,
    /// The repositories cloned into the session's directory, in order.
    #[serde(default)]
    #[validate(length(max = MAX_REPOSITORIES), nested)]
    repositories: Vec<RepositoryRequest>,
    /// The GitHub token the agent works with: absent follows the project, `null` asks for
    /// none, and an id names one.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[schemars(with = "Option<Uuid>")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and an id are three distinct requests"
    )]
    github_credential_id: Option<Option<Uuid>>,
    /// The first prompt, queued as the session's first turn. Large enough for a pasted stack
    /// trace or spec, as a later turn's is.
    #[validate(
        length(min = 1, max = 100_000),
        custom(function = "validate_not_blank")
    )]
    prompt: Option<String>,
    /// The action item the session is started from. Its first turn carries the item's
    /// context ahead of `prompt`, which is then required.
    action_item_id: Option<Uuid>,
}

/// The most repositories one session clones.
///
/// Clones run one after another before the session is usable, and the first that fails
/// refuses the session, so a long list is slow to start and fragile; sixteen covers a service
/// with its libraries while keeping that bounded.
const MAX_REPOSITORIES: u64 = 16;

/// One repository to clone. `Serialize` because a length error on the list reports it.
#[derive(Debug, Deserialize, Serialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryRequest {
    /// Git URL to clone from.
    #[validate(length(max = 2048), custom(function = "validate_repository_url"))]
    url: String,
    /// The branch the work starts from; absent uses the remote's default branch.
    #[validate(length(min = 1, max = 255), custom(function = "validate_not_blank"))]
    base_branch: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<CreateCodingSessionRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    let session = create(&state, current.id(), body).await?;
    Ok((StatusCode::CREATED, Json(json!({ "session": session }))))
}

/// Starts a session as `actor`: records it, clones its repositories, queues its first turn,
/// and announces it. See the module documentation for the order and what is undone on failure.
///
/// # Errors
/// The request's validation errors, `404` for an unknown project or item, `400` for a
/// repository that will not clone, and `502` when the workspace is not available.
pub async fn create(
    state: &AppState,
    actor: Uuid,
    body: CreateCodingSessionRequest,
) -> Result<CodingSessionResponse, ApiError> {
    body.validate()?;
    refuse_shared_directories(&body.repositories)?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let project = project::find(&mut connection, body.project_id).await?;
    let first_turn = first_turn::build(
        &mut connection,
        &project,
        body.action_item_id,
        body.prompt,
        Utc::now(),
    )
    .await?;
    let github = github_token::open(
        &mut connection,
        &state.cipher,
        &project,
        SessionChoice::from_request(body.github_credential_id),
    )
    .await?;
    let new_session = NewCodingSession {
        created_by: actor,
        id: coding_session::reserve_id(&mut connection).await?,
        project_id: Some(project.id),
        title: body.title,
        github_credential_id: github.map(|github| github.credential_id),
        action_item_id: body.action_item_id,
        studio_item_id: None,
    };
    let session = coding_session::create(&mut connection, &new_session).await?;
    drop(connection);

    let clones: Vec<RepositoryClone> = body
        .repositories
        .into_iter()
        .map(repository_to_clone)
        .collect();
    let started = start(state, &session, &clones, first_turn).await;
    if let Err(error) = started {
        discard(state, &session).await;
        return Err(error);
    }

    let response = CodingSessionResponse::new(session.clone(), state.sessions.activity(session.id));
    state
        .events
        .publish(&ServerEvent::SessionUpserted(response.clone()));
    Ok(response)
}

/// Clones the session's repositories, then queues its first turn when it has one.
async fn start(
    state: &AppState,
    session: &CodingSession,
    clones: &[RepositoryClone],
    first_turn: Option<String>,
) -> Result<(), ApiError> {
    state.sessions.prepare(session, clones).await?;
    if let Some(prompt) = first_turn {
        state
            .sessions
            .start_turn(session, prompt, TurnExtras::default())
            .await?;
    }
    Ok(())
}

/// Removes a session that could not be started: its turns, its directory, and its row.
async fn discard(state: &AppState, session: &CodingSession) {
    state.sessions.remove(session.id).await;
    let deleted = match state.database.get().await {
        Ok(mut connection) => coding_session::delete(&mut connection, session.id)
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    if let Err(message) = deleted {
        event!(
            name: "coding_session.create.orphaned_row",
            Level::ERROR,
            session.id = %session.id,
            error.message = %message,
            "could not remove a session that was not started; delete it by hand",
        );
    }
}

/// Refuses a list in which two repositories would clone into the same directory.
///
/// Names are compared ignoring case, so a workspace stays usable on a case-insensitive
/// filesystem and `Api` beside `api` never confuses a reader. The same repository listed
/// twice is named as such, since it is the likelier mistake.
///
/// # Errors
/// Answers `400` naming both URLs.
fn refuse_shared_directories(repositories: &[RepositoryRequest]) -> Result<(), ApiError> {
    let mut claimed: HashMap<String, &str> = HashMap::new();
    for repository in repositories {
        let directory =
            repository_directory(&repository.url).expect("validation guarantees a directory name");
        let earlier = match claimed.entry(directory.to_lowercase()) {
            Entry::Vacant(vacant) => {
                vacant.insert(&repository.url);
                continue;
            }
            Entry::Occupied(occupied) => *occupied.get(),
        };

        if repository_identity(earlier) == repository_identity(&repository.url) {
            return Err(ApiError::BadRequest(format!(
                "repositories lists the same repository twice: {earlier} and {}",
                repository.url
            )));
        }
        return Err(ApiError::BadRequest(format!(
            "repositories {earlier} and {} would both clone into the directory {directory}; \
             each needs its own name, ignoring case",
            repository.url
        )));
    }
    Ok(())
}

/// What makes two URLs the same repository. github.com names are case-insensitive and
/// reachable over HTTPS and SSH alike; any other host is compared as written, without a
/// trailing slash or `.git`.
fn repository_identity(url: &str) -> String {
    if let Some(repository) = Repository::from_url(url) {
        return format!("github.com/{}/{}", repository.owner, repository.name).to_lowercase();
    }
    let trimmed = url.trim_end_matches('/');
    trimmed.strip_suffix(".git").unwrap_or(trimmed).to_owned()
}

/// The repository as the session clones it: a github.com SSH remote over HTTPS.
fn repository_to_clone(repository: RepositoryRequest) -> RepositoryClone {
    RepositoryClone {
        directory: repository_directory(&repository.url)
            .expect("validation guarantees a directory name")
            .to_owned(),
        url: github_token::clone_url(&repository.url),
        base_branch: repository.base_branch,
    }
}

/// The directory a repository is cloned into: the URL's last path segment without
/// `.git`. Only plain names are accepted, so none can escape the session's directory.
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
    // Elysium holds no SSH keys. A github.com SSH remote is cloned over HTTPS instead
    // (`github_token::clone_url`); any other SSH remote could never authenticate.
    let is_ssh = url.starts_with("ssh://") || url.starts_with("git@");
    if is_ssh && Repository::from_url(url).is_none() {
        return Err(ValidationError::new("ssh").with_message(
            "SSH remotes are supported only on github.com; use the https URL".into(),
        ));
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

    /// A request body with these repository URLs, none with a base branch.
    fn body_with(urls: &[&str]) -> CreateCodingSessionRequest {
        let repositories: Vec<Value> = urls.iter().map(|url| json!({ "url": url })).collect();
        serde_json::from_value(json!({
            "projectId": Uuid::nil(),
            "title": "A",
            "repositories": repositories,
        }))
        .expect("parses")
    }

    #[test]
    fn bodies_validate_titles_and_repository_urls() {
        let valid: CreateCodingSessionRequest = serde_json::from_value(json!({
            "projectId": Uuid::nil(),
            "title": "Fix the login bug",
            "repositories": [
                { "url": "https://github.com/JalapenoLabs/Elysium.git", "baseBranch": "main" },
                { "url": "git@github.com:JalapenoLabs/uikit.git" },
            ],
        }))
        .expect("parses");
        valid.validate().expect("valid body");
        refuse_shared_directories(&valid.repositories).expect("distinct directories");

        let none: CreateCodingSessionRequest = serde_json::from_value(
            json!({ "projectId": Uuid::nil(), "title": "A" }),
        )
        .expect("parses");
        assert!(none.repositories.is_empty());

        let blank_title: CreateCodingSessionRequest = serde_json::from_value(
            json!({ "projectId": Uuid::nil(), "title": "  " }),
        )
        .expect("parses");
        assert!(
            blank_title
                .validate()
                .expect_err("blank title")
                .field_errors()
                .contains_key("title")
        );

        body_with(&["file:///etc/passwd"])
            .validate()
            .expect_err("file URLs are refused");
        body_with(&["git@github.com:JalapenoLabs/Elysium.git"])
            .validate()
            .expect("github.com SSH remotes are cloned over HTTPS");
        body_with(&["git@gitlab.com:org/repo.git"])
            .validate()
            .expect_err("no key could authenticate it");

        let too_many: Vec<String> = (0..=MAX_REPOSITORIES)
            .map(|index| format!("https://github.com/JalapenoLabs/repo-{index}"))
            .collect();
        let too_many: Vec<&str> = too_many.iter().map(String::as_str).collect();
        body_with(&too_many)
            .validate()
            .expect_err("a session clones at most sixteen repositories");

        serde_json::from_value::<CreateCodingSessionRequest>(json!({
            "projectId": Uuid::nil(),
            "title": "A",
            "repositoryUrl": "https://github.com/JalapenoLabs/Elysium.git",
        }))
        .expect_err("a single repositoryUrl is no longer accepted");
    }

    #[test]
    fn repositories_that_would_share_a_directory_are_refused_naming_both() {
        let refusal = |urls: &[&str]| match refuse_shared_directories(&body_with(urls).repositories)
        {
            Err(ApiError::BadRequest(message)) => message,
            other => panic!("expected a 400, got {other:?}"),
        };

        let same = refusal(&[
            "https://github.com/JalapenoLabs/Elysium.git",
            "git@github.com:jalapenolabs/elysium",
        ]);
        assert!(same.contains("same repository"), "{same}");
        assert!(
            same.contains("https://github.com/JalapenoLabs/Elysium.git"),
            "{same}"
        );
        assert!(
            same.contains("git@github.com:jalapenolabs/elysium"),
            "{same}"
        );

        let clash = refusal(&[
            "https://github.com/JalapenoLabs/api.git",
            "https://gitlab.com/someone/API",
        ]);
        assert!(clash.contains("directory"), "{clash}");
        assert!(
            clash.contains("https://github.com/JalapenoLabs/api.git"),
            "{clash}"
        );
        assert!(clash.contains("https://gitlab.com/someone/API"), "{clash}");
    }

    #[test]
    fn a_first_prompt_is_optional_and_never_blank() {
        let base = json!({ "projectId": Uuid::nil(), "title": "A" });

        let none: CreateCodingSessionRequest =
            serde_json::from_value(base.clone()).expect("parses");
        none.validate()
            .expect("a session may start without a prompt");
        assert_eq!(none.prompt, None);
        assert_eq!(none.action_item_id, None);

        let mut blank = base.clone();
        blank["prompt"] = json!("  ");
        let blank: CreateCodingSessionRequest = serde_json::from_value(blank).expect("parses");
        let refused = blank.validate().expect_err("a blank prompt");
        assert!(refused.field_errors().contains_key("prompt"));

        let mut from_item = base;
        from_item["prompt"] = json!("Fix it.");
        from_item["actionItemId"] = json!(Uuid::nil());
        let from_item: CreateCodingSessionRequest =
            serde_json::from_value(from_item).expect("parses");
        from_item.validate().expect("valid");
        assert_eq!(from_item.action_item_id, Some(Uuid::nil()));
    }

    #[test]
    fn github_credential_ids_distinguish_absent_null_and_an_id() {
        let base = json!({ "projectId": Uuid::nil(), "title": "A" });

        let absent: CreateCodingSessionRequest =
            serde_json::from_value(base.clone()).expect("parses");
        assert_eq!(absent.github_credential_id, None);

        let mut null = base.clone();
        null["githubCredentialId"] = Value::Null;
        let null: CreateCodingSessionRequest = serde_json::from_value(null).expect("parses");
        assert_eq!(null.github_credential_id, Some(None));

        let mut named = base;
        named["githubCredentialId"] = json!(Uuid::nil());
        let named: CreateCodingSessionRequest = serde_json::from_value(named).expect("parses");
        assert_eq!(named.github_credential_id, Some(Some(Uuid::nil())));
    }
}
