// Copyright © 2026 Jalapeno Labs

//! `~/.claude` for every agent, fetched from a configuration repository.
//!
//! Agents read their instructions, skills, and settings from Claude Code's configuration
//! directory, which the workspace image points at `/workspace/.claude` (`CLAUDE_CONFIG_DIR`),
//! on the workspace volume. When the workspace settings name a configuration repository,
//! that directory is a checkout of it: [`PROVISION_SCRIPT`] fetches the repository's default
//! branch and checks it out over the directory with `-f`, which replaces tracked files and
//! leaves untracked ones. Claude's transcripts (`projects/`) are untracked, so every
//! conversation survives a fetch and `--resume` keeps working. Each checkout is then adapted
//! to the workspace ([`adapt`]), leaving out what only its owner's machine can run.
//!
//! The repository is read with a GitHub token of its own, chosen beside it in the workspace
//! settings, since it may be private to an account the workspace's default token cannot reach.
//! The token goes to git as an `http.extraHeader` in the environment, for github.com alone, so
//! it is never written to disk or put in the remote's URL. With no token the repository is read
//! as anyone may read it.
//!
//! Nothing here decides who may run without a checkout: `crate::agent_configuration` holds
//! workers and Project Managers back until one is fetched, and records how each fetch went.

pub mod adapt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{Level, event};
use url::Url;
use uuid::Uuid;

use self::adapt::Skipped;
use super::workspace::{AGENT_USER, SCRIPT_TIMEOUT, WorkspaceContainer};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::{github_credential, user};

/// Claude Code's configuration directory in the workspace. The image sets `CLAUDE_CONFIG_DIR`
/// to it and links the agent's `~/.claude` there.
pub const CLAUDE_CONFIG_DIRECTORY: &str = "/workspace/.claude";

/// The only host a configuration repository's token is sent to: tokens are GitHub's.
pub const TOKEN_HOST: &str = "github.com";

/// The longest commit subject kept, in characters: a subject is one line, and this is a long one.
const MAX_SUBJECT_CHARACTERS: usize = 200;

/// `$1` is the configuration directory and `$2` the repository URL. Its last line is the commit
/// checked out and its subject.
const PROVISION_SCRIPT: &str = r#"
set -u
mkdir -p "$1" && cd "$1" || exit 70
[ -d .git ] || git init -q || exit 70
git remote remove origin 2>/dev/null
git remote add origin "$2" || exit 70
git fetch -q --depth 1 origin HEAD || exit 71
git checkout -q -f FETCH_HEAD || exit 72
git log -1 --format='%H %s'
"#;

/// `$1` is the configuration directory. Prints the commit checked out there, or nothing.
const HEAD_SCRIPT: &str = r#"
cd "$1" 2>/dev/null && git rev-parse -q --verify HEAD 2>/dev/null
exit 0
"#;

/// A configuration repository and the token that reads it.
#[derive(Debug, Clone)]
pub struct ConfigurationRepository {
    pub url: Url,
    pub token: Option<SecretString>,
}

/// A repository and the token chosen for it, as the settings named them when a fetch began.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub repository: String,
    pub github_credential_id: Option<Uuid>,
}

/// What a fetch checked out: the commit, and what [`adapt`] left out of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkout {
    pub repository: String,
    /// The full 40-character commit id.
    pub commit: String,
    /// The commit's subject line.
    pub subject: String,
    pub fetched_at: DateTime<Utc>,
    pub skipped: Vec<Skipped>,
}

/// The repository the settings name, and it with its token ready to fetch, or why it is not: a
/// stored URL that no longer parses, or a token that cannot be opened, such as one sealed under
/// another encryption key. Either is the fetch's failure, recorded for the repository like git's.
#[derive(Debug)]
pub struct Loaded {
    pub source: Source,
    pub repository: Result<ConfigurationRepository, String>,
}

/// How one fetch of the configuration repository went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// The workspace settings name no repository.
    Unset,
    /// The repository the settings named, and its checkout or why there is none.
    Attempted {
        source: Source,
        outcome: Result<Checkout, String>,
    },
}

/// Where the configuration repository is read from.
pub enum ConfigurationSource {
    Database {
        database: Pool,
        cipher: Arc<Cipher>,
    },
    #[cfg(test)]
    None,
    /// One repository, as a live test names it.
    #[cfg(test)]
    Fixed(ConfigurationRepository),
}

impl std::fmt::Debug for ConfigurationSource {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database { .. } => formatter.write_str("ConfigurationSource::Database"),
            #[cfg(test)]
            Self::None => formatter.write_str("ConfigurationSource::None"),
            #[cfg(test)]
            Self::Fixed(_repository) => formatter.write_str("ConfigurationSource::Fixed"),
        }
    }
}

impl ConfigurationSource {
    /// The repository the workspace settings name, with the token chosen for it.
    ///
    /// # Errors
    /// Propagates a database error reading the settings. Everything about the repository itself
    /// is [`Loaded::repository`]'s.
    pub async fn load(&self) -> anyhow::Result<Option<Loaded>> {
        let (database, cipher) = match self {
            Self::Database { database, cipher } => (database, cipher),
            #[cfg(test)]
            Self::None => return Ok(None),
            #[cfg(test)]
            Self::Fixed(repository) => {
                return Ok(Some(Loaded {
                    source: Source {
                        repository: repository.url.to_string(),
                        github_credential_id: None,
                    },
                    repository: Ok(repository.clone()),
                }));
            }
        };
        let mut connection = database.get().await?;
        let settings = user::workspace_settings(&mut connection).await?;
        let Some(stored) = settings.agent_configuration_repository else {
            return Ok(None);
        };
        let github_credential_id = settings.agent_configuration_github_credential_id;
        let repository = async {
            let url = Url::parse(&stored)?;
            // The settings refuse a query or a fragment now, but a URL saved before they did may
            // carry one, and with it a token, which would follow the URL into every checkout,
            // batch, and error. It is refused here without being repeated.
            if url.query().is_some() || url.fragment().is_some() {
                anyhow::bail!(
                    "the configuration repository's URL carries a query or a fragment, which may \
                     hold a secret; save it again without one under Settings, LLMs, Agent \
                     configuration"
                );
            }
            let token = match github_credential_id {
                // The settings refuse a token for any other host; this keeps it from ever leaving.
                Some(id) if url.host_str() == Some(TOKEN_HOST) => {
                    let credential = github_credential::find(&mut connection, id).await?;
                    Some(credential.token(cipher).map_err(|error| {
                        anyhow::anyhow!(
                            "the configuration repository's token cannot be opened: {error}"
                        )
                    })?)
                }
                _none_or_elsewhere => None,
            };
            anyhow::Ok(ConfigurationRepository { url, token })
        }
        .await
        .map_err(|error| format!("{error:#}"));
        Ok(Some(Loaded {
            source: Source {
                repository: stored,
                github_credential_id,
            },
            repository,
        }))
    }
}

/// Checks `repository` out into [`CLAUDE_CONFIG_DIRECTORY`] and adapts it to the workspace.
///
/// # Errors
/// Describes what git reported when the fetch or checkout fails, or why adapting failed.
pub async fn provision(
    workspace: &WorkspaceContainer,
    repository: &ConfigurationRepository,
) -> anyhow::Result<Checkout> {
    let environment = repository
        .token
        .as_ref()
        .map(|token| git_authorization(&repository.url, token))
        .unwrap_or_default();
    let output = workspace
        .run_script(
            AGENT_USER,
            PROVISION_SCRIPT,
            &[CLAUDE_CONFIG_DIRECTORY, repository.url.as_str()],
            environment,
            SCRIPT_TIMEOUT,
        )
        .await?;
    if output.exit_code != Some(0) {
        anyhow::bail!(
            "could not fetch the configuration repository {}: {}",
            repository.url,
            output.output.trim()
        );
    }
    let (commit, subject) = output
        .output
        .lines()
        .last()
        .and_then(read_commit_line)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "the configuration repository {} was fetched, but git did not name its commit: {}",
                repository.url,
                output.output.trim()
            )
        })?;
    let skipped = adapt::run(workspace).await?;
    event!(
        name: "runner.configuration.fetched",
        Level::INFO,
        url.full = %repository.url,
        vcs.ref.head.revision = %commit,
        configuration.skipped = skipped.len(),
        "fetched the configuration repository",
    );
    Ok(Checkout {
        repository: repository.url.to_string(),
        commit,
        subject,
        fetched_at: Utc::now(),
        skipped,
    })
}

/// The commit checked out in `workspace`'s configuration directory, if any.
///
/// # Errors
/// Propagates Docker's refusal.
pub async fn checked_out(workspace: &WorkspaceContainer) -> anyhow::Result<Option<String>> {
    let output = workspace
        .run_script(
            AGENT_USER,
            HEAD_SCRIPT,
            &[CLAUDE_CONFIG_DIRECTORY],
            Vec::new(),
            SCRIPT_TIMEOUT,
        )
        .await?;
    Ok(output
        .output
        .lines()
        .next()
        .map(str::trim)
        .filter(|commit| is_commit(commit))
        .map(str::to_owned))
}

/// A `git log --format='%H %s'` line as the commit and its subject.
fn read_commit_line(line: &str) -> Option<(String, String)> {
    let (commit, subject) = line.split_once(' ').unwrap_or((line, ""));
    is_commit(commit).then(|| {
        let subject = subject
            .trim()
            .chars()
            .take(MAX_SUBJECT_CHARACTERS)
            .collect();
        (commit.to_owned(), subject)
    })
}

/// Whether `text` is a full commit id, as the status's column holds one.
fn is_commit(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Git configuration, as environment variables, that sends `token` to `url`'s origin alone.
fn git_authorization(url: &Url, token: &SecretString) -> Vec<String> {
    let credentials = STANDARD.encode(format!("x-access-token:{}", token.expose_secret()));
    vec![
        "GIT_CONFIG_COUNT=1".to_owned(),
        format!(
            "GIT_CONFIG_KEY_0=http.{}.extraHeader",
            url.origin().ascii_serialization()
        ),
        format!("GIT_CONFIG_VALUE_0=Authorization: Basic {credentials}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_scoped_to_the_repositorys_origin() {
        let url = Url::parse("https://github.com/someone/dotclaude.git").expect("a url");
        let environment = git_authorization(&url, &SecretString::from("ghp_example"));
        assert_eq!(
            environment[1],
            "GIT_CONFIG_KEY_0=http.https://github.com.extraHeader"
        );
        let expected = STANDARD.encode("x-access-token:ghp_example");
        assert_eq!(
            environment[2],
            format!("GIT_CONFIG_VALUE_0=Authorization: Basic {expected}")
        );
    }

    #[test]
    fn a_commit_line_names_the_commit_and_its_subject() {
        let commit = "611c46a583443c3a8dd4caad4a89dffa964add43";
        assert_eq!(
            read_commit_line(&format!("{commit} Memory: Elysium on main")),
            Some((commit.to_owned(), "Memory: Elysium on main".to_owned()))
        );
        assert_eq!(
            read_commit_line(commit),
            Some((commit.to_owned(), String::new())),
            "a commit with no subject"
        );
        assert_eq!(read_commit_line("fatal: not a git repository"), None);
        assert_eq!(read_commit_line(&commit.to_uppercase()), None);
    }
}
