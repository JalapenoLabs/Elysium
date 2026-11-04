// Copyright © 2026 Jalapeno Labs

//! Readying a worker's container for its batch, before Claude Code starts.
//!
//! 1. **Tools.** The project's tools script runs as root in the batch's directory, for whatever
//!    its work needs beyond the workspace image.
//! 2. **Repositories.** Each repository the batch names is cloned into `repos/<name>` with
//!    `--recurse-submodules`, through `gh` with the worker's GitHub token. A submodule that does
//!    not clone (one the token cannot read, or that no longer exists) stops setup, and the
//!    batch's result names it.
//! 3. **Their setup.** A repository the project declares with a setup script has it run as the
//!    agent in its directory once it is cloned.
//!
//! Setup is done once per container: a marker on the worker's volume says it finished, so an
//! engine that takes the worker over after a restart starts its next turn rather than cloning
//! again. One stopped in the middle starts over, each clone replacing what a cut-short one left.
//! Every script's output is scrubbed of the worker's secrets before it reaches a log or a result.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

use crate::models::work_batch::Repository;
use crate::models::worker_setup::WorkerSetup;
use crate::routes::v1::coding_sessions::github_token;
use crate::routes::v1::coding_sessions::{repository_directory, repository_identity};
use crate::runner::workspace::{AGENT_USER, ScriptOutput, WorkspaceContainer};
use crate::sessions::scrub::Scrubber;

/// The batch's directory in the worker's workspace, which its turns work in; its repositories
/// are cloned under `repos/`, as a coding session's are.
pub const BATCH_DIRECTORY: &str = "batch";

/// Written once setup finished, so a worker taken over after a restart is not set up twice.
const SETUP_MARKER: &str = "/workspace/.elysium/setup-finished";

/// How long the tools script may take: a toolchain download and install over a slow link.
const TOOLS_TIMEOUT: Duration = Duration::from_mins(30);

/// How long one repository's clone, with its submodules, may take: a large monorepo over a slow
/// link, as a coding session allows for all of its clones together.
const CLONE_TIMEOUT: Duration = Duration::from_mins(30);

/// How long one repository's setup script may take: installing its dependencies and a first
/// build.
const REPOSITORY_SETUP_TIMEOUT: Duration = Duration::from_mins(30);

/// What the clone script prints for each submodule that did not clone: the marker, its path,
/// and its URL, tab separated.
const SUBMODULE_MARKER: &str = "elysium-submodule";

/// The clone script's exit status when the repository cloned but a submodule did not.
const SUBMODULE_FAILED: i64 = 72;

/// Runs the tools script, `$1`, as root in the batch's directory. The directory is created as
/// the agent's first, since the clones that follow run as the agent; what the script itself
/// writes there is root's, which is why it installs tools system-wide rather than into the batch.
const TOOLS_SCRIPT: &str = r#"
set -u
install -d -o agent -g agent -- /workspace/batch && cd -- /workspace/batch || exit 70
bash -c "$1" 2>&1
"#;

/// Clones one repository with its submodules. `$1` is the batch's directory, `$2` the directory
/// name under `repos/`, `$3` the URL, and `$4` the branch, or empty for the remote's default.
///
/// When the repository cloned but a submodule did not, it prints each submodule left without a
/// checkout, which `git submodule status` marks `-`, and exits [`SUBMODULE_FAILED`]. Terminal
/// prompts are off, so a submodule the token cannot read fails rather than waiting for a
/// password nobody types.
const CLONE_SCRIPT: &str = r#"
set -u
export GIT_TERMINAL_PROMPT=0
mkdir -p -- "$1/repos" && cd -- "$1/repos" || exit 70
rm -rf -- "$2"
if [ -n "$4" ]; then
  git clone --quiet --recurse-submodules --branch "$4" -- "$3" "$2" 2>&1
else
  git clone --quiet --recurse-submodules -- "$3" "$2" 2>&1
fi
status=$?
[ "$status" -eq 0 ] && exit 0
[ -d "$2/.git" ] || exit 71
cd -- "$2" || exit 71
git submodule status --recursive 2>/dev/null | while read -r line; do
  case "$line" in
    -*)
      path=$(printf '%s\n' "$line" | awk '{ print $2 }')
      name=$(git config --file .gitmodules --get-regexp '\.path$' 2>/dev/null \
        | awk -v path="$path" '$2 == path { sub(/^submodule\./, "", $1); sub(/\.path$/, "", $1); print $1 }')
      url=$(git config --file .gitmodules --get "submodule.$name.url" 2>/dev/null)
      printf 'elysium-submodule\t%s\t%s\n' "$path" "$url"
      ;;
  esac
done
exit 72
"#;

/// Runs a repository's setup script, `$2`, as the agent in its directory, `$1`.
const REPOSITORY_SETUP_SCRIPT: &str = r#"
cd -- "$1" || exit 70
bash -c "$2" 2>&1
"#;

/// Says whether setup finished: exit 0 when the marker is there.
const SETUP_FINISHED_SCRIPT: &str = r#"test -f "$1""#;

/// Leaves the marker that setup finished.
const MARK_SETUP_SCRIPT: &str = r#"mkdir -p -- "$(dirname -- "$1")" && touch -- "$1""#;

/// Setup, decided before anything runs: the tools script, then each repository in the batch's
/// order with what readies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub tools_script: Option<String>,
    pub repositories: Vec<PlannedRepository>,
}

/// One repository to clone and ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRepository {
    /// As the batch names it, for what a person reads.
    pub url: String,
    /// What git clones: a github.com SSH remote over HTTPS, since Elysium holds no SSH keys.
    pub clone_url: String,
    /// Its directory under `repos/`.
    pub directory: String,
    /// The batch's base branch, or else the project's, or else the remote's default.
    pub base_branch: Option<String>,
    /// The project's setup script for it, when it declares one.
    pub setup_script: Option<String>,
}

/// The plan for a batch that names `repositories`, by what its project declares in `setup`. A
/// repository the project does not declare is cloned with no setup script.
///
/// # Errors
/// Names a repository that has no directory name, or two that would clone into the same one,
/// ignoring case: rules every batch is already held to when it is submitted, checked again
/// against a stored row.
pub fn plan(repositories: &[Repository], setup: &WorkerSetup) -> Result<Plan, String> {
    let declared: HashMap<String, &crate::models::worker_setup::ProjectRepository> = setup
        .repositories
        .iter()
        .map(|repository| (repository_identity(&repository.url), repository))
        .collect();
    let mut directories: HashMap<String, &str> = HashMap::new();
    let mut planned = Vec::with_capacity(repositories.len());
    for repository in repositories {
        let Some(directory) = repository_directory(&repository.url) else {
            return Err(format!(
                "repository {} names no directory to clone into",
                repository.url
            ));
        };
        match directories.entry(directory.to_lowercase()) {
            Entry::Occupied(earlier) => {
                return Err(format!(
                    "repositories {} and {} would both clone into {directory}",
                    earlier.get(),
                    repository.url
                ));
            }
            Entry::Vacant(vacant) => {
                vacant.insert(&repository.url);
            }
        }
        let declaration = declared.get(&repository_identity(&repository.url));
        planned.push(PlannedRepository {
            url: repository.url.clone(),
            clone_url: github_token::clone_url(&repository.url),
            directory: directory.to_owned(),
            base_branch: repository
                .base_branch
                .clone()
                .or_else(|| declaration.and_then(|declared| declared.base_branch.clone())),
            setup_script: declaration.and_then(|declared| declared.setup_script.clone()),
        });
    }
    Ok(Plan {
        tools_script: setup.tools_setup_script.clone(),
        repositories: planned,
    })
}

/// Why setup stopped, in words the batch's result carries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SetupFailure(pub String);

/// What setup did, one line per step, for the worker's log.
pub type Progress = Vec<String>;

/// Whether `workspace` finished setup already.
///
/// # Errors
/// Describes Docker's refusal.
pub async fn is_finished(workspace: &WorkspaceContainer) -> anyhow::Result<bool> {
    let checked = workspace
        .run_script(
            AGENT_USER,
            SETUP_FINISHED_SCRIPT,
            &[SETUP_MARKER],
            Vec::new(),
            Duration::from_secs(30),
        )
        .await?;
    Ok(checked.exit_code == Some(0))
}

/// Runs `plan` in `workspace` with `environment` (the worker's variables and its GitHub token),
/// writing what each step did to `progress`, scrubbed by `scrubber`, then marks setup finished.
///
/// # Errors
/// [`SetupFailure`] naming the step that failed and what it printed, scrubbed.
pub async fn run(
    workspace: &WorkspaceContainer,
    plan: &Plan,
    environment: &[String],
    scrubber: &Scrubber,
    progress: &mut Progress,
) -> Result<(), SetupFailure> {
    if let Some(script) = &plan.tools_script {
        let output = workspace
            .run_script(
                "root",
                TOOLS_SCRIPT,
                &[script],
                environment.to_vec(),
                TOOLS_TIMEOUT,
            )
            .await
            .map_err(|error| SetupFailure(format!("the tools script could not run: {error}")))?;
        check("the project's tools script", &output, scrubber, progress)?;
    }

    let batch_directory = format!("/workspace/{BATCH_DIRECTORY}");
    for repository in &plan.repositories {
        let output = workspace
            .run_script(
                AGENT_USER,
                CLONE_SCRIPT,
                &[
                    &batch_directory,
                    &repository.directory,
                    &repository.clone_url,
                    repository.base_branch.as_deref().unwrap_or(""),
                ],
                environment.to_vec(),
                CLONE_TIMEOUT,
            )
            .await
            .map_err(|error| {
                SetupFailure(format!("{} could not be cloned: {error}", repository.url))
            })?;
        if output.exit_code == Some(SUBMODULE_FAILED) {
            return Err(SetupFailure(submodule_failure(
                &repository.url,
                &scrubber.scrub_text(&output.output),
            )));
        }
        check(
            &format!("cloning {}", repository.url),
            &output,
            scrubber,
            progress,
        )?;

        if let Some(script) = &repository.setup_script {
            let directory = format!("{batch_directory}/repos/{}", repository.directory);
            let output = workspace
                .run_script(
                    AGENT_USER,
                    REPOSITORY_SETUP_SCRIPT,
                    &[&directory, script],
                    environment.to_vec(),
                    REPOSITORY_SETUP_TIMEOUT,
                )
                .await
                .map_err(|error| {
                    SetupFailure(format!(
                        "the setup script of {} could not run: {error}",
                        repository.url
                    ))
                })?;
            check(
                &format!("the setup script of {}", repository.url),
                &output,
                scrubber,
                progress,
            )?;
        }
    }

    let marked = workspace
        .run_script(
            AGENT_USER,
            MARK_SETUP_SCRIPT,
            &[SETUP_MARKER],
            Vec::new(),
            Duration::from_secs(30),
        )
        .await
        .map_err(|error| SetupFailure(format!("setup could not be marked finished: {error}")))?;
    if marked.exit_code != Some(0) {
        return Err(SetupFailure(format!(
            "setup could not be marked finished: {}",
            marked.output.trim()
        )));
    }
    Ok(())
}

/// Logs a step that succeeded; answers the failure of one that did not.
fn check(
    step: &str,
    output: &ScriptOutput,
    scrubber: &Scrubber,
    progress: &mut Progress,
) -> Result<(), SetupFailure> {
    let printed = scrubber.scrub_text(output.output.trim());
    if output.exit_code == Some(0) {
        progress.push(format!("Setup: {step} finished."));
        return Ok(());
    }
    let status = output.exit_code.map_or_else(
        || "no exit status".to_owned(),
        |code| format!("exit status {code}"),
    );
    Err(SetupFailure(format!("{step} failed ({status}): {printed}")))
}

/// The failure of `url`'s clone whose submodules did not clone, naming each from the clone
/// script's `printed` lines, with what git said.
fn submodule_failure(url: &str, printed: &str) -> String {
    let mut named = Vec::new();
    let mut said = Vec::new();
    for line in printed.lines() {
        let mut fields = line.split('\t');
        if fields.next() == Some(SUBMODULE_MARKER) {
            let path = fields.next().unwrap_or_default();
            match fields
                .next()
                .filter(|submodule_url| !submodule_url.is_empty())
            {
                Some(submodule_url) => named.push(format!("{path} ({submodule_url})")),
                None => named.push(path.to_owned()),
            }
            continue;
        }
        said.push(line);
    }
    let which = if named.is_empty() {
        "a submodule".to_owned()
    } else {
        format!("submodule {}", named.join(", "))
    };
    format!(
        "{url} cloned, but {which} could not be cloned: the worker's GitHub token cannot read it, \
         or it no longer exists. Git said: {}",
        said.join("\n").trim()
    )
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::models::worker_setup::ProjectRepository;

    fn declared(
        url: &str,
        base_branch: Option<&str>,
        setup_script: Option<&str>,
    ) -> ProjectRepository {
        ProjectRepository {
            id: Uuid::now_v7(),
            project_id: Uuid::nil(),
            position: 0,
            url: url.to_owned(),
            base_branch: base_branch.map(str::to_owned),
            setup_script: setup_script.map(str::to_owned),
            created_by: Uuid::nil(),
            created_at: Utc::now(),
        }
    }

    fn named(url: &str, base_branch: Option<&str>) -> Repository {
        Repository {
            url: url.to_owned(),
            base_branch: base_branch.map(str::to_owned),
        }
    }

    #[test]
    fn a_declared_repository_brings_its_setup_and_branch_and_others_clone_bare() {
        let setup = WorkerSetup {
            tools_setup_script: Some("apt-get install -y ripgrep".to_owned()),
            repositories: vec![declared(
                "https://github.com/JalapenoLabs/Elysium.git",
                Some("develop"),
                Some("yarn install"),
            )],
        };
        let plan = plan(
            &[
                // Named over SSH and in another case: the same repository.
                named("git@github.com:jalapenolabs/elysium.git", None),
                named("https://github.com/JalapenoLabs/uikit", Some("next")),
            ],
            &setup,
        )
        .expect("a plan");
        assert_eq!(
            plan.tools_script.as_deref(),
            Some("apt-get install -y ripgrep")
        );
        let [elysium, uikit] = plan.repositories.as_slice() else {
            panic!("two repositories");
        };
        assert_eq!(elysium.directory, "elysium");
        assert_eq!(
            elysium.clone_url, "https://github.com/jalapenolabs/elysium.git",
            "SSH remotes clone over HTTPS"
        );
        assert_eq!(elysium.base_branch.as_deref(), Some("develop"));
        assert_eq!(elysium.setup_script.as_deref(), Some("yarn install"));
        assert_eq!(uikit.base_branch.as_deref(), Some("next"));
        assert_eq!(
            uikit.setup_script, None,
            "undeclared, so nothing readies it"
        );
    }

    #[test]
    fn the_batchs_branch_wins_over_the_projects() {
        let setup = WorkerSetup {
            tools_setup_script: None,
            repositories: vec![declared("https://github.com/o/a.git", Some("main"), None)],
        };
        let plan =
            plan(&[named("https://github.com/o/a.git", Some("fix"))], &setup).expect("a plan");
        assert_eq!(plan.repositories[0].base_branch.as_deref(), Some("fix"));
    }

    #[test]
    fn two_repositories_sharing_a_directory_are_refused() {
        let setup = WorkerSetup {
            tools_setup_script: None,
            repositories: Vec::new(),
        };
        let refused = plan(
            &[
                named("https://github.com/one/api.git", None),
                named("https://github.com/two/API.git", None),
            ],
            &setup,
        )
        .expect_err("refused");
        assert!(refused.contains("both clone into"), "{refused}");
    }

    #[test]
    fn a_submodule_that_did_not_clone_is_named_with_its_url() {
        let printed = "fatal: repository 'https://github.com/o/private.git/' not found\n\
            fatal: clone of 'https://github.com/o/private.git' into submodule path \
            '/workspace/batch/repos/app/vendor/private' failed\n\
            elysium-submodule\tvendor/private\thttps://github.com/o/private.git\n";
        let message = submodule_failure("https://github.com/o/app.git", printed);
        assert!(
            message.contains("submodule vendor/private (https://github.com/o/private.git)"),
            "{message}"
        );
        assert!(
            message.contains("not found"),
            "keeps what git said: {message}"
        );
        assert!(!message.contains(SUBMODULE_MARKER), "{message}");
    }
}
