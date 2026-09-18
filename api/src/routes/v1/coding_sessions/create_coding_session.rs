// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/coding-sessions`: open a thread on a satellite and record it.
//!
//! The session's number is reserved first, so the thread carries it in its metadata from
//! the moment it exists. A create that fails after the reservation leaves a gap in the
//! numbering, which is harmless.
//!
//! The satellite's idempotency key is a fresh `UUIDv7` per request, never the number: numbers
//! repeat across Elysium installs sharing a satellite and after a database reset, and a
//! repeated key would hand back another session's thread. The key makes the SDK's own
//! retries of this one request safe; a client that posts again opens a second thread.
//!
//! A prompt, when given, is queued as the thread's first turn. A session started from an
//! action item must have one, and its first turn carries the item's context ahead of it (see
//! `first_turn`). The turn is queued only once the session is recorded and its watcher and
//! relay are started, so the tools the turn asks the agent to call have a client answering
//! them as early as Elysium can manage.
//!
//! If recording the row or queuing the first turn fails, the thread is destroyed rather than
//! left running with nothing pointing at it, and a recorded row is removed with it, so a
//! create either yields a session with its first turn queued or nothing.
//!
//! A session clones any number of repositories up to [`MAX_REPOSITORIES`], each into its own
//! directory under the workspace's `repos/`. The satellite checks that each name is safe but
//! not that the names differ, so two repositories that would share a directory are refused
//! here, while the caller is still listening, rather than failing a clone minutes later.

use std::collections::BTreeMap;
use std::collections::hash_map::{Entry, HashMap};

use anyhow::Context;
use arsox_sdk::client::ThreadHandle;
use arsox_sdk::proto::settings::v1::Repo;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::Utc;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use super::github_token::{self, SessionChoice, SessionToken};
use super::{CodingSessionResponse, thread_settings, validate_not_blank};
use super::{first_turn, model_stack};
use crate::crypto::Cipher;
use crate::errors::ApiError;
use crate::fleet::views::ThreadStatus;
use crate::fleet::{MANAGED_METADATA_KEY, SESSION_METADATA_KEY};
use crate::github::Repository;
use crate::models::coding_session::{self, CodingSession, NewCodingSession};
use crate::models::environment_variable;
use crate::models::llm::{self, Llm};
use crate::models::{project, satellite, storage_location};
use crate::realtime::ServerEvent;
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
    let storage_locations = storage_location::list_for_project(&mut connection, project.id).await?;
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

    let stack = model_stack::build(&open_credentials(credentials, &state.cipher), Utc::now());

    let client = state.fleet.client(satellite.id).await?;
    // Reserved once there is a client for the satellite, so a create refused for an
    // inactive or unreachable satellite costs no number.
    let session_id = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        coding_session::reserve_id(&mut connection).await?
    };
    let metadata = BTreeMap::from([
        (MANAGED_METADATA_KEY.to_owned(), "true".to_owned()),
        (SESSION_METADATA_KEY.to_owned(), session_id.to_string()),
    ]);
    let created = client
        .threads()
        .create_with(
            thread_settings(
                repositories,
                stack,
                &variables,
                github.as_ref().map(|github| &github.token),
                !storage_locations.is_empty(),
            ),
            Some(Uuid::now_v7().to_string()),
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
        action_item_id: body.action_item_id,
    };
    let session = record_or_abandon(&state, &created.handle, &new_session).await?;

    let thread = ThreadStatus::from(&created.thread);
    state.fleet.watch_session(session.clone());
    if let Some(first_turn) = first_turn {
        queue_or_discard(&state, &created.handle, &session, first_turn).await?;
    }
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

/// Records the session of a thread just opened, destroying the thread when the row cannot be
/// written, so no thread runs with nothing pointing at it.
///
/// # Errors
/// Propagates the database's refusal.
async fn record_or_abandon(
    state: &AppState,
    handle: &ThreadHandle,
    new_session: &NewCodingSession,
) -> Result<CodingSession, ApiError> {
    let recorded = match state.database.get().await {
        Ok(mut connection) => coding_session::create(&mut connection, new_session)
            .await
            .map_err(ApiError::from),
        Err(pool_error) => Err(anyhow::Error::from(pool_error)
            .context("no database connection available")
            .into()),
    };
    if recorded.is_err() {
        abandon_thread(handle, new_session.satellite_id).await;
    }
    recorded
}

/// Queues a new session's first turn, or, when the satellite refuses it, discards the session
/// whole: its thread, its row, and its watchers, then announces it gone. This handler has not
/// announced the session, but the satellite poll reads the row and may have; a
/// `session.deleted` for a session a client never saw changes nothing.
///
/// The row goes before the watchers, so a poll landing in between cannot record a status
/// for a session nothing would clear.
///
/// # Errors
/// Answers the satellite's refusal of the turn.
async fn queue_or_discard(
    state: &AppState,
    handle: &ThreadHandle,
    session: &CodingSession,
    first_turn: String,
) -> Result<(), ApiError> {
    let Err(turn_error) = handle.start_turn(first_turn).await else {
        return Ok(());
    };
    abandon_thread(handle, session.satellite_id).await;
    match state.database.get().await {
        Ok(mut connection) => {
            if let Err(database_error) = coding_session::delete(&mut connection, session.id).await {
                event!(
                    name: "coding_session.create.orphaned_row",
                    Level::ERROR,
                    session.id = %session.id,
                    error.message = %database_error,
                    "could not remove a session whose first turn was refused; delete it by hand",
                );
            }
        }
        Err(pool_error) => event!(
            name: "coding_session.create.orphaned_row",
            Level::ERROR,
            session.id = %session.id,
            error.message = %pool_error,
            "could not remove a session whose first turn was refused; delete it by hand",
        ),
    }
    state.fleet.forget_session(session.id);
    state
        .events
        .publish(&ServerEvent::SessionDeleted { id: session.id });
    Err(turn_error.into())
}

/// Destroys a thread whose session could not be completed. A thread that will not go is
/// logged and left to expire on its idle TTL.
async fn abandon_thread(handle: &ThreadHandle, satellite_id: Uuid) {
    if let Err(destroy_error) = handle.destroy().await {
        event!(
            name: "coding_session.create.orphaned_thread",
            Level::ERROR,
            satellite.id = %satellite_id,
            thread.id = %handle.id(),
            error.message = %destroy_error,
            "could not complete the session or destroy its thread; the thread expires on its idle TTL",
        );
    }
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
        let settings = thread_settings(Vec::new(), None, &[], None, false);
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

        let without = thread_settings(Vec::new(), None, &[], None, false);
        assert_eq!(names(&without), ["elysium_work"]);

        let with = thread_settings(Vec::new(), None, &[], None, true);
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
        let settings = thread_settings(Vec::new(), None, &variables, Some(&token), false);

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
