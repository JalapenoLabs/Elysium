// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/coding-sessions`: open a thread on a satellite and record it.
//!
//! How the thread is opened and recorded is shared with Studio (see `open`). A prompt, when
//! given, is queued as the thread's first turn. A session started from an action item must
//! have one, and its first turn carries the item's context ahead of it (see `first_turn`).
//! If the satellite refuses that turn, the session is discarded whole, so a create either
//! yields a session with its first turn queued or nothing.
//!
//! A session clones any number of repositories up to [`MAX_REPOSITORIES`], each into its own
//! directory under the workspace's `repos/`. The satellite checks that each name is safe but
//! not that the names differ, so two repositories that would share a directory are refused
//! here, while the caller is still listening, rather than failing a clone minutes later.

use std::collections::hash_map::{Entry, HashMap};

use anyhow::Context;
use arsox_sdk::proto::settings::v1::Repo;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use super::github_token::{self, SessionChoice, SessionToken};
use super::open::{self, SessionOpening};
use super::{CodingSessionResponse, ThreadPlan, thread_settings, validate_not_blank};
use super::{first_turn, model_stack};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::github::Repository;
use crate::models::environment_variable;
use crate::models::llm;
use crate::models::{project, satellite, storage_location};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    project_id: Uuid,
    satellite_id: Uuid,
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    title: String,
    /// The repositories the satellite clones into the thread's workspace, in order.
    #[serde(default)]
    #[validate(length(max = MAX_REPOSITORIES), nested)]
    repositories: Vec<RepositoryRequest>,
    /// The GitHub token the agent works with: absent follows the project, `null` asks for
    /// none, and an id names one.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and an id are three distinct requests"
    )]
    github_credential_id: Option<Option<Uuid>>,
    /// The first prompt, queued as the thread's first turn. Large enough for a pasted stack
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
/// The satellite sets no limit of its own. Clones run one after another before the thread is
/// usable, and the first that fails parks the thread, so a long list is slow to start and
/// fragile; sixteen covers a service with its libraries while keeping that bounded.
const MAX_REPOSITORIES: u64 = 16;

/// One repository to clone. `Serialize` because a length error on the list reports it.
#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryRequest {
    /// Git URL the satellite clones from.
    #[validate(length(max = 2048), custom(function = "validate_repository_url"))]
    url: String,
    /// The branch the work starts from; absent uses the remote's default branch.
    #[validate(length(min = 1, max = 255), custom(function = "validate_not_blank"))]
    base_branch: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;
    refuse_shared_directories(&body.repositories)?;

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
    let storage_locations =
        storage_location::list_for_project(&mut connection, Some(project.id)).await?;
    // An item is checked, and its context read, before a thread exists.
    let first_turn = first_turn::build(
        &mut connection,
        &project,
        body.action_item_id,
        body.prompt,
        Utc::now(),
    )
    .await?;

    let variables =
        environment_variable::thread_environment(&mut connection, &state.cipher).await?;
    let github = github_token::open(
        &mut connection,
        &state.cipher,
        &project,
        SessionChoice::from_request(body.github_credential_id),
    )
    .await?;
    drop(connection);

    let repositories = body
        .repositories
        .into_iter()
        .map(|repository| repository_to_clone(repository, github.as_ref()))
        .collect();
    let stack = model_stack::build(
        &model_stack::open_credentials(credentials, &state.cipher),
        Utc::now(),
    );
    let settings = thread_settings(ThreadPlan {
        repositories,
        stack,
        variables: &variables,
        github_token: github.as_ref().map(|github| &github.token),
        has_storage_locations: !storage_locations.is_empty(),
        has_project: true,
        instructions: String::new(),
    });

    let opened = open::open(
        &state,
        SessionOpening {
            created_by: current.id(),
            satellite_id: satellite.id,
            settings,
            title: body.title,
            project_id: Some(project.id),
            github_credential_id: github.map(|github| github.credential_id),
            action_item_id: body.action_item_id,
            studio_item_id: None,
        },
    )
    .await?;
    if let Some(first_turn) = first_turn
        && let Err(turn_error) = opened.handle.start_turn(first_turn).await
    {
        open::discard(&state, &opened).await;
        return Err(turn_error.into());
    }
    open::announce(&state, &opened);

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "session": CodingSessionResponse::new(opened.session, Some(opened.thread)),
        })),
    ))
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

/// The repository the satellite clones into the workspace, with the session's GitHub token
/// as its credential when GitHub accepts one for that URL.
fn repository_to_clone(repository: RepositoryRequest, github: Option<&SessionToken>) -> Repo {
    let clone_url = github_token::clone_url(&repository.url);
    Repo {
        name: repository_directory(&repository.url)
            .expect("validation guarantees a directory name")
            .to_owned(),
        auth: github.and_then(|github| github_token::clone_auth(&clone_url, &github.token)),
        url: clone_url,
        base_branch: repository.base_branch,
        ..Repo::default()
    }
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
    use secrecy::SecretString;

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
    fn body_with(urls: &[&str]) -> RequestBody {
        let repositories: Vec<Value> = urls.iter().map(|url| json!({ "url": url })).collect();
        serde_json::from_value(json!({
            "projectId": Uuid::nil(),
            "satelliteId": Uuid::nil(),
            "title": "A",
            "repositories": repositories,
        }))
        .expect("parses")
    }

    #[test]
    fn bodies_validate_titles_and_repository_urls() {
        let valid: RequestBody = serde_json::from_value(json!({
            "projectId": Uuid::nil(),
            "satelliteId": Uuid::nil(),
            "title": "Fix the login bug",
            "repositories": [
                { "url": "https://github.com/JalapenoLabs/Elysium.git", "baseBranch": "main" },
                { "url": "git@github.com:JalapenoLabs/uikit.git" },
            ],
        }))
        .expect("parses");
        valid.validate().expect("valid body");
        refuse_shared_directories(&valid.repositories).expect("distinct directories");

        let none: RequestBody = serde_json::from_value(
            json!({ "projectId": Uuid::nil(), "satelliteId": Uuid::nil(), "title": "A" }),
        )
        .expect("parses");
        assert!(none.repositories.is_empty());

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

        serde_json::from_value::<RequestBody>(json!({
            "projectId": Uuid::nil(),
            "satelliteId": Uuid::nil(),
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
    fn new_threads_declare_the_ceilings_the_satellite_requires() {
        let settings = thread_settings(ThreadPlan { has_project: true, ..ThreadPlan::default() });
        assert!(settings.idle_ttl.is_some());
        let budget = settings.budget.expect("budget is set");
        assert!(budget.max_tokens_per_turn.is_some());
        assert!(budget.max_cost_per_thread.is_some());
        assert!(budget.max_wall_clock_per_turn.is_some());
        assert!(settings.repos.is_empty());
        assert!(settings.env.is_empty() && settings.github.is_none());
    }

    #[test]
    fn every_thread_declares_the_work_tools_and_storage_only_with_a_location() {
        let names = |settings: &arsox_sdk::proto::settings::v1::ThreadSettings| -> Vec<String> {
            settings
                .relayed_mcp_servers
                .iter()
                .map(|server| server.name.clone())
                .collect()
        };

        let without = thread_settings(ThreadPlan { has_project: true, ..ThreadPlan::default() });
        assert_eq!(names(&without), ["elysium_work"]);

        let with = thread_settings(ThreadPlan {
            has_storage_locations: true,
            has_project: true,
            ..ThreadPlan::default()
        });
        assert_eq!(names(&with), ["elysium_storage", "elysium_work"]);
        let servers = [crate::tools::storage::SERVER, crate::tools::work::SERVER];
        for (declared, server) in with.relayed_mcp_servers.iter().zip(servers) {
            assert_eq!(declared.tools.len(), server.tools.len(), "{}", server.name);
            for tool in &declared.tools {
                let schema: serde_json::Value =
                    serde_json::from_str(&tool.input_schema_json).expect("schemas are JSON");
                assert_eq!(schema["type"], "object", "{}", tool.name);
            }
        }
    }

    #[test]
    fn every_thread_declares_its_own_blender_and_the_mcp_server_that_reaches_it() {
        let settings = thread_settings(ThreadPlan { has_project: true, ..ThreadPlan::default() });
        let services: Vec<&str> = settings
            .services
            .iter()
            .map(|service| service.name.as_str())
            .collect();
        assert_eq!(services, ["blender-bridge", "blender-mcp"]);

        let [server] = settings.mcp_servers.as_slice() else {
            panic!("one MCP server is declared: {:?}", settings.mcp_servers);
        };
        assert_eq!(server.name, "blender");
        let endpoint = server
            .service
            .as_ref()
            .expect("the server targets a service");
        assert_eq!(endpoint.service, "blender-mcp");
    }

    #[test]
    fn workspace_variables_come_before_the_ones_elysium_sets() {
        use crate::models::environment_variable::ThreadVariable;

        let variables = [
            ThreadVariable {
                key: "NPM_TOKEN".to_owned(),
                value: SecretString::from("npm-secret"),
                is_secret: true,
            },
            ThreadVariable {
                key: "NODE_ENV".to_owned(),
                value: SecretString::from("development"),
                is_secret: false,
            },
        ];
        let token = SecretString::from("github_pat_example");
        let settings = thread_settings(ThreadPlan {
            variables: &variables,
            github_token: Some(&token),
            has_project: true,
            ..ThreadPlan::default()
        });

        let keys: Vec<&str> = settings
            .env
            .iter()
            .map(|variable| variable.key.as_str())
            .collect();
        assert_eq!(&keys[..3], ["NPM_TOKEN", "NODE_ENV", "GH_TOKEN"]);
        assert_eq!(settings.env[0].is_secret, Some(true));
        assert_eq!(settings.env[1].is_secret, Some(false));
        assert!(settings.github.is_some());
    }

    #[test]
    fn a_first_prompt_is_optional_and_never_blank() {
        let base = json!({ "projectId": Uuid::nil(), "satelliteId": Uuid::nil(), "title": "A" });

        let none: RequestBody = serde_json::from_value(base.clone()).expect("parses");
        none.validate()
            .expect("a session may start without a prompt");
        assert_eq!(none.prompt, None);
        assert_eq!(none.action_item_id, None);

        let mut blank = base.clone();
        blank["prompt"] = json!("  ");
        let blank: RequestBody = serde_json::from_value(blank).expect("parses");
        let refused = blank.validate().expect_err("a blank prompt");
        assert!(refused.field_errors().contains_key("prompt"));

        let mut from_item = base;
        from_item["prompt"] = json!("Fix it.");
        from_item["actionItemId"] = json!(Uuid::nil());
        let from_item: RequestBody = serde_json::from_value(from_item).expect("parses");
        from_item.validate().expect("valid");
        assert_eq!(from_item.action_item_id, Some(Uuid::nil()));
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
