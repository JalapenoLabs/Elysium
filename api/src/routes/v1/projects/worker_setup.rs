// Copyright © 2026 Jalapeno Labs

//! `/api/v1/projects/{id}/worker-setup`: how the project readies its workers' containers, the
//! repositories it declares, each with an optional setup script, and one script for its tools.
//! Anyone who may edit the project reads and replaces it, as `PATCH /api/v1/projects/{id}`
//! allows. See `docs/engine.md`, Workers.
//!
//! `PUT` replaces the whole setup. Each repository follows a coding session's rules (an https,
//! http, ssh, or `git@` URL, SSH on github.com alone), names a directory of its own ignoring
//! case, and is listed once; a project declares at most 16, as a batch clones at most 16. A
//! script that is only whitespace is no script. Each repository's `mergePolicy` says who merges
//! its workers' pull requests: `person` unless asked, and `auto` only on github.com, where
//! Elysium can read and merge them. The change is announced as `projectWorkerSetup.changed`; the
//! scripts apply to the next worker that starts, and who merges to the next look at a pull
//! request.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::github::Repository;
use crate::models::worker_setup::{
    self, Declaration, MergePolicy, ProjectRepository, REPOSITORIES_MAX, SCRIPT_MAX_CHARACTERS,
    WorkerSetup,
};
use crate::realtime::ServerEvent;
use crate::routes::v1::coding_sessions::{
    repository_directory, repository_identity, validate_repository_url,
};
use crate::state::AppState;

/// The longest base branch, matching `project_repositories.base_branch`.
const BRANCH_MAX_CHARACTERS: usize = 255;

/// A project's worker setup as clients see it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSetupResponse {
    tools_setup_script: Option<String>,
    repositories: Vec<RepositoryResponse>,
}

/// One declared repository as clients see it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryResponse {
    id: Uuid,
    url: String,
    base_branch: Option<String>,
    setup_script: Option<String>,
    merge_policy: MergePolicy,
}

impl From<WorkerSetup> for WorkerSetupResponse {
    fn from(setup: WorkerSetup) -> Self {
        Self {
            tools_setup_script: setup.tools_setup_script,
            repositories: setup
                .repositories
                .into_iter()
                .map(RepositoryResponse::from)
                .collect(),
        }
    }
}

impl From<ProjectRepository> for RepositoryResponse {
    fn from(repository: ProjectRepository) -> Self {
        Self {
            id: repository.id,
            url: repository.url,
            base_branch: repository.base_branch,
            setup_script: repository.setup_script,
            merge_policy: repository.merge_policy,
        }
    }
}

/// The whole setup, replacing what the project had.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    tools_setup_script: Option<String>,
    #[serde(default)]
    repositories: Vec<RepositoryRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryRequest {
    url: String,
    #[serde(default)]
    base_branch: Option<String>,
    #[serde(default)]
    setup_script: Option<String>,
    #[serde(default)]
    merge_policy: MergePolicy,
}

pub async fn get(
    State(state): State<AppState>,
    _current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let setup = worker_setup::find(&mut connection, id).await?;
    Ok(Json(
        json!({ "workerSetup": WorkerSetupResponse::from(setup) }),
    ))
}

pub async fn put(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let (tools_setup_script, repositories) = check(body).map_err(ApiError::BadRequest)?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let setup = worker_setup::replace(
        &mut connection,
        id,
        tools_setup_script.as_deref(),
        &repositories,
        current.id(),
    )
    .await?;
    state
        .events
        .publish(&ServerEvent::ProjectWorkerSetupChanged { project_id: id });
    Ok(Json(
        json!({ "workerSetup": WorkerSetupResponse::from(setup) }),
    ))
}

/// The setup `body` asks for, held to every rule before anything is written.
///
/// # Errors
/// Names the first rule `body` breaks.
fn check(body: RequestBody) -> Result<(Option<String>, Vec<Declaration>), String> {
    let tools_setup_script = script(body.tools_setup_script, "toolsSetupScript")?;
    if body.repositories.len() > REPOSITORIES_MAX {
        return Err(format!(
            "a project declares at most {REPOSITORIES_MAX} repositories, as a batch clones at most \
             {REPOSITORIES_MAX}"
        ));
    }
    let mut identities: HashMap<String, String> = HashMap::new();
    let mut directories: HashMap<String, String> = HashMap::new();
    let mut declarations = Vec::with_capacity(body.repositories.len());
    for repository in body.repositories {
        let url = repository.url.trim().to_owned();
        if let Err(refusal) = validate_repository_url(&url) {
            let reason = refusal
                .message
                .unwrap_or_else(|| "is not a repository URL".into());
            return Err(format!("repository {url}: {reason}"));
        }
        match identities.entry(repository_identity(&url)) {
            Entry::Occupied(earlier) => {
                return Err(format!(
                    "repositories lists the same repository twice: {} and {url}",
                    earlier.get()
                ));
            }
            Entry::Vacant(vacant) => {
                vacant.insert(url.clone());
            }
        }
        let directory = repository_directory(&url)
            .expect("a valid repository URL names a directory")
            .to_owned();
        match directories.entry(directory.to_lowercase()) {
            Entry::Occupied(earlier) => {
                return Err(format!(
                    "repositories {} and {url} would both clone into {directory}; each needs a \
                     name of its own, ignoring case",
                    earlier.get()
                ));
            }
            Entry::Vacant(vacant) => {
                vacant.insert(url.clone());
            }
        }
        let base_branch = repository
            .base_branch
            .map(|branch| branch.trim().to_owned())
            .filter(|branch| !branch.is_empty());
        if base_branch
            .as_ref()
            .is_some_and(|branch| branch.chars().count() > BRANCH_MAX_CHARACTERS)
        {
            return Err(format!(
                "repository {url}: baseBranch is at most {BRANCH_MAX_CHARACTERS} characters"
            ));
        }
        let setup_script = script(
            repository.setup_script,
            &format!("repository {url}: setupScript"),
        )?;
        if repository.merge_policy == MergePolicy::Auto && Repository::from_url(&url).is_none() {
            return Err(format!(
                "repository {url}: Elysium merges pull requests on github.com alone, so \
                 mergePolicy auto needs a github.com repository"
            ));
        }
        declarations.push(Declaration {
            url,
            base_branch,
            setup_script,
            merge_policy: repository.merge_policy,
        });
    }
    Ok((tools_setup_script, declarations))
}

/// A script as stored: `None` for none or only whitespace, refused past
/// [`SCRIPT_MAX_CHARACTERS`]. Kept as written otherwise, since its whitespace is the author's.
fn script(script: Option<String>, name: &str) -> Result<Option<String>, String> {
    let Some(script) = script.filter(|script| !script.trim().is_empty()) else {
        return Ok(None);
    };
    if script.chars().count() > SCRIPT_MAX_CHARACTERS {
        return Err(format!(
            "{name} is at most {SCRIPT_MAX_CHARACTERS} characters"
        ));
    }
    Ok(Some(script))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(value: Value) -> RequestBody {
        serde_json::from_value(value).expect("a body")
    }

    #[test]
    fn a_setup_is_kept_as_written_and_blank_scripts_are_none() {
        let (tools, repositories) = check(body(json!({
            "toolsSetupScript": "apt-get install -y protobuf-compiler\n",
            "repositories": [
                {
                    "url": " https://github.com/JalapenoLabs/Elysium.git ",
                    "baseBranch": " main ",
                    "setupScript": "cd frontend && yarn install",
                },
                { "url": "git@github.com:JalapenoLabs/uikit.git", "setupScript": "   " },
            ],
        })))
        .expect("accepted");
        assert_eq!(
            tools.as_deref(),
            Some("apt-get install -y protobuf-compiler\n")
        );
        assert_eq!(
            repositories[0].url,
            "https://github.com/JalapenoLabs/Elysium.git"
        );
        assert_eq!(repositories[0].base_branch.as_deref(), Some("main"));
        assert_eq!(repositories[1].setup_script, None);
        // Nobody asked Elysium to merge, so a person does.
        assert_eq!(repositories[0].merge_policy, MergePolicy::Person);
    }

    #[test]
    fn elysium_merges_only_where_it_can_read_pull_requests() {
        let (_tools, repositories) = check(body(json!({ "repositories": [
            { "url": "ssh://git@github.com/JalapenoLabs/Elysium.git", "mergePolicy": "auto" },
            { "url": "https://gitlab.com/o/r.git", "mergePolicy": "person" },
        ] })))
        .expect("accepted");
        assert_eq!(repositories[0].merge_policy, MergePolicy::Auto);
        assert_eq!(repositories[1].merge_policy, MergePolicy::Person);

        let elsewhere = check(body(json!({ "repositories": [
            { "url": "https://gitlab.com/o/r.git", "mergePolicy": "auto" },
        ] })))
        .expect_err("refused");
        assert!(elsewhere.contains("github.com alone"), "{elsewhere}");
    }

    #[test]
    fn the_same_repository_twice_and_shared_directories_are_refused() {
        let twice = check(body(json!({ "repositories": [
            { "url": "https://github.com/JalapenoLabs/Elysium.git" },
            { "url": "git@github.com:jalapenolabs/elysium.git" },
        ] })))
        .expect_err("refused");
        assert!(twice.contains("the same repository twice"), "{twice}");

        let shared = check(body(json!({ "repositories": [
            { "url": "https://github.com/one/api.git" },
            { "url": "https://gitlab.com/two/API.git" },
        ] })))
        .expect_err("refused");
        assert!(shared.contains("would both clone into"), "{shared}");
    }

    #[test]
    fn urls_follow_a_coding_sessions_rules_and_lists_are_bounded() {
        let ssh = check(body(
            json!({ "repositories": [{ "url": "git@gitlab.com:o/r.git" }] }),
        ))
        .expect_err("refused");
        assert!(
            ssh.contains("SSH remotes are supported only on github.com"),
            "{ssh}"
        );

        let many: Vec<Value> = (0..=REPOSITORIES_MAX)
            .map(|index| json!({ "url": format!("https://github.com/o/r{index}.git") }))
            .collect();
        let refused = check(body(json!({ "repositories": many }))).expect_err("refused");
        assert!(refused.contains("at most 16"), "{refused}");
    }
}
