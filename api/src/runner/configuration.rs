// Copyright © 2026 Jalapeno Labs

//! `~/.claude` for every agent, fetched from a configuration repository.
//!
//! Agents read their instructions, skills, and settings from Claude Code's configuration
//! directory, which the workspace image points at `/workspace/.claude` (`CLAUDE_CONFIG_DIR`),
//! on the workspace volume. When the workspace settings name a configuration repository,
//! that directory is a checkout of it: [`PROVISION_SCRIPT`] fetches the repository's default
//! branch and checks it out over the directory with `-f`, which replaces tracked files and
//! leaves untracked ones. Claude's transcripts (`projects/`) are untracked, so every
//! conversation survives a fetch and `--resume` keeps working.
//!
//! An https URL on github.com is fetched with the workspace's default GitHub token, through
//! git's `http.extraHeader` in the environment, so the token is never written to disk or put
//! in the remote's URL. Without a configured repository the directory is left as it is.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
use tracing::{Level, event};
use url::Url;

use super::workspace::{AGENT_USER, SCRIPT_TIMEOUT, WorkspaceContainer};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::{github_credential, user};

/// Claude Code's configuration directory in the workspace. The image sets `CLAUDE_CONFIG_DIR`
/// to it and links the agent's `~/.claude` there.
pub const CLAUDE_CONFIG_DIRECTORY: &str = "/workspace/.claude";

/// `$1` is the configuration directory and `$2` the repository URL.
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

/// A configuration repository and the token that reads it.
#[derive(Debug, Clone)]
pub struct ConfigurationRepository {
    pub url: Url,
    pub token: Option<SecretString>,
}

/// Where the configuration repository is read from.
pub enum ConfigurationSource {
    Database {
        database: Pool,
        cipher: Arc<Cipher>,
    },
    #[cfg(test)]
    None,
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
        }
    }
}

impl ConfigurationSource {
    /// The repository the workspace settings name, with the default GitHub token for a
    /// github.com URL.
    ///
    /// # Errors
    /// Propagates any database error, and refuses a stored URL that no longer parses.
    pub async fn load(&self) -> anyhow::Result<Option<ConfigurationRepository>> {
        let (database, cipher) = match self {
            Self::Database { database, cipher } => (database, cipher),
            #[cfg(test)]
            Self::None => return Ok(None),
        };
        let mut connection = database.get().await?;
        let settings = user::workspace_settings(&mut connection).await?;
        let Some(stored) = settings.agent_configuration_repository else {
            return Ok(None);
        };
        let url = Url::parse(&stored)?;
        let token = if url.host_str() == Some("github.com") {
            match github_credential::find_default(&mut connection).await? {
                Some(credential) => Some(credential.token(cipher)?),
                None => None,
            }
        } else {
            None
        };
        Ok(Some(ConfigurationRepository { url, token }))
    }
}

/// Checks the configuration repository out into [`CLAUDE_CONFIG_DIRECTORY`].
///
/// # Errors
/// Describes what git reported when the fetch or checkout fails.
pub async fn provision(
    workspace: &WorkspaceContainer,
    repository: Option<ConfigurationRepository>,
) -> anyhow::Result<()> {
    let Some(repository) = repository else {
        event!(
            name: "runner.configuration.unset",
            Level::INFO,
            "no configuration repository is set; agents use the workspace's own ~/.claude",
        );
        return Ok(());
    };

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
    event!(
        name: "runner.configuration.fetched",
        Level::INFO,
        url.full = %repository.url,
        vcs.ref.head.revision = %output.output.trim(),
        "fetched the configuration repository",
    );
    Ok(())
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
}
