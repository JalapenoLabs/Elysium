// Copyright © 2026 Jalapeno Labs

//! How a thread's agent gets its GitHub token.
//!
//! A session starts with one token, chosen at three levels: the workspace default, the
//! project's choice, and the session's own choice when it is created. The token reaches
//! the agent as `GH_TOKEN`, which `gh` reads on its own. Plain `git` does not, so the
//! thread also declares git config through the environment (`GIT_CONFIG_COUNT`, git 2.31
//! and later) that makes `gh` git's credential helper for github.com. Nothing is written
//! into the workspace, and no satellite change is needed.
//!
//! The same token is lent to the satellite's own clone of the session's repository, which
//! runs before the agent does and does not see the agent's environment.
//!
//! An agent can read every variable in its environment. The satellite scrubs `GH_TOKEN`
//! from what it reports because the variable is declared secret, but the agent holds the
//! token, which is why a session's token should be scoped to what the session needs.

use anyhow::Context;
use arsox_sdk::proto::common::v1::Secret;
use arsox_sdk::proto::settings::v1::{EnvVar, GitAuth, GithubIntegration, git_auth};
use diesel::result::Error as DieselError;
use diesel_async::AsyncPgConnection;
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::errors::ApiError;
use crate::github::Repository;
use crate::models::github_credential;
use crate::models::project::Project;

/// The git config the thread declares, in order. The empty helper first clears any helper
/// configured elsewhere for github.com, as `gh auth setup-git` does, so git asks only `gh`.
const GIT_CONFIG: [(&str, &str); 2] = [
    ("credential.https://github.com.helper", ""),
    (
        "credential.https://github.com.helper",
        "!gh auth git-credential",
    ),
];

/// Which token a new session asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionChoice {
    /// Whatever the project picks, which may be the workspace default or no token.
    Project,
    /// No token, whatever the project picks.
    None,
    /// This token.
    Credential(Uuid),
}

impl SessionChoice {
    /// Reads a request's `githubCredentialId`: absent follows the project, `null` asks for
    /// no token, and an id names one.
    #[expect(
        clippy::option_option,
        reason = "absent, null, and an id are three distinct requests"
    )]
    pub const fn from_request(requested: Option<Option<Uuid>>) -> Self {
        match requested {
            None => Self::Project,
            Some(None) => Self::None,
            Some(Some(id)) => Self::Credential(id),
        }
    }

    /// The token the session starts with, given its project and the workspace default.
    pub const fn resolve(self, project: &Project, workspace_default: Option<Uuid>) -> Option<Uuid> {
        match self {
            Self::Project => project.github_credential(workspace_default),
            Self::None => None,
            Self::Credential(id) => Some(id),
        }
    }
}

/// The token a session starts with, opened and ready to hand to its thread.
#[derive(Debug)]
pub struct SessionToken {
    pub credential_id: Uuid,
    pub token: SecretString,
}

/// Resolves which token a new session starts with and decrypts it, or `None` for no token.
///
/// # Errors
/// Answers `400` when the session names a token that does not exist. Unlike a model
/// credential, a token that cannot be decrypted is not skipped: the agent would start
/// without the access it was promised, so the session is refused.
pub async fn open(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    project: &Project,
    choice: SessionChoice,
) -> Result<Option<SessionToken>, ApiError> {
    let workspace_default = github_credential::find_default(connection)
        .await?
        .map(|credential| credential.id);
    let Some(credential_id) = choice.resolve(project, workspace_default) else {
        return Ok(None);
    };

    let credential = github_credential::find(connection, credential_id)
        .await
        .map_err(|error| match error {
            DieselError::NotFound => ApiError::BadRequest(
                "githubCredentialId names a token that does not exist".to_owned(),
            ),
            other => other.into(),
        })?;
    let token = credential
        .token(cipher)
        .context("the stored GitHub token cannot be decrypted")?;
    Ok(Some(SessionToken {
        credential_id,
        token,
    }))
}

/// The variables that give the agent the token: `GH_TOKEN` for `gh`, and the git config
/// that routes `git` through `gh`.
pub fn thread_environment(token: &SecretString) -> Vec<EnvVar> {
    let mut environment = vec![EnvVar {
        key: "GH_TOKEN".to_owned(),
        value: Some(secret(token.expose_secret())),
        is_secret: Some(true),
    }];

    environment.push(plain_variable(
        "GIT_CONFIG_COUNT",
        &GIT_CONFIG.len().to_string(),
    ));
    for (index, (key, value)) in GIT_CONFIG.iter().enumerate() {
        environment.push(plain_variable(&format!("GIT_CONFIG_KEY_{index}"), key));
        environment.push(plain_variable(&format!("GIT_CONFIG_VALUE_{index}"), value));
    }
    environment
}

/// The token as the thread's GitHub integration, which the satellite scrubs from its
/// reports and which its planned `gh` broker will use.
pub fn integration(token: &SecretString) -> GithubIntegration {
    GithubIntegration {
        token: Some(secret(token.expose_secret())),
    }
}

/// The URL the satellite clones a repository from.
///
/// Elysium holds no SSH keys, so a github.com SSH remote could never clone. It is cloned
/// over HTTPS instead, where the session's token authenticates it, or nothing does for a
/// public repository. The workspace's `origin` is then HTTPS too, which is the remote the
/// `gh` credential helper serves. Any other URL is cloned as written.
pub fn clone_url(repository_url: &str) -> String {
    let is_github_ssh = !repository_url.starts_with("https://");
    match Repository::from_url(repository_url) {
        Some(repository) if is_github_ssh => {
            format!(
                "https://github.com/{}/{}.git",
                repository.owner, repository.name
            )
        }
        _ => repository_url.to_owned(),
    }
}

/// The token lent to the satellite's clone, for a repository on github.com over HTTPS.
pub fn clone_auth(repository_url: &str, token: &SecretString) -> Option<GitAuth> {
    let is_https = repository_url.starts_with("https://");
    if !is_https || Repository::from_url(repository_url).is_none() {
        return None;
    }
    Some(GitAuth {
        credential: Some(git_auth::Credential::PersonalAccessToken(secret(
            token.expose_secret(),
        ))),
    })
}

/// A variable the satellite may show as it is. Declared explicitly: an unset `is_secret`
/// means secret, and scrubbing `!gh auth git-credential` from a log would only confuse.
fn plain_variable(key: &str, value: &str) -> EnvVar {
    EnvVar {
        key: key.to_owned(),
        value: Some(secret(value)),
        is_secret: Some(false),
    }
}

fn secret(value: &str) -> Secret {
    Secret {
        value: Some(value.to_owned()),
        display: None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::models::project::{GithubAccess, ProjectCoverFit};

    fn project(github_access: GithubAccess, github_credential_id: Option<Uuid>) -> Project {
        Project {
            id: Uuid::nil(),
            name: "Elysium".to_owned(),
            description: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            cover_image_updated_at: None,
            cover_fit: ProjectCoverFit::Fit,
            github_access,
            github_credential_id,
        }
    }

    #[test]
    fn sessions_follow_the_project_which_follows_the_workspace_default() {
        let workspace_default = Some(Uuid::from_u128(1));
        let projects_own = Uuid::from_u128(2);
        let sessions_own = Uuid::from_u128(3);

        let follows_default = project(GithubAccess::Default, None);
        let opted_out = project(GithubAccess::None, None);
        let specific = project(GithubAccess::Specific, Some(projects_own));
        let token_deleted = project(GithubAccess::Specific, None);

        let project_choice = SessionChoice::from_request(None);
        assert_eq!(
            project_choice.resolve(&follows_default, workspace_default),
            workspace_default
        );
        assert_eq!(project_choice.resolve(&opted_out, workspace_default), None);
        assert_eq!(
            project_choice.resolve(&specific, workspace_default),
            Some(projects_own)
        );
        assert_eq!(
            project_choice.resolve(&token_deleted, workspace_default),
            workspace_default,
            "a project whose token was deleted follows the default"
        );
        assert_eq!(project_choice.resolve(&follows_default, None), None);

        let no_token = SessionChoice::from_request(Some(None));
        assert_eq!(no_token.resolve(&specific, workspace_default), None);

        let override_token = SessionChoice::from_request(Some(Some(sessions_own)));
        assert_eq!(
            override_token.resolve(&opted_out, workspace_default),
            Some(sessions_own)
        );
    }

    #[test]
    fn the_environment_hands_gh_the_token_and_routes_git_through_gh() {
        let environment = thread_environment(&SecretString::from("github_pat_example"));
        let pairs: Vec<(&str, Option<&str>, Option<bool>)> = environment
            .iter()
            .map(|variable| {
                (
                    variable.key.as_str(),
                    variable
                        .value
                        .as_ref()
                        .and_then(|value| value.value.as_deref()),
                    variable.is_secret,
                )
            })
            .collect();

        assert_eq!(
            pairs,
            vec![
                ("GH_TOKEN", Some("github_pat_example"), Some(true)),
                ("GIT_CONFIG_COUNT", Some("2"), Some(false)),
                (
                    "GIT_CONFIG_KEY_0",
                    Some("credential.https://github.com.helper"),
                    Some(false)
                ),
                ("GIT_CONFIG_VALUE_0", Some(""), Some(false)),
                (
                    "GIT_CONFIG_KEY_1",
                    Some("credential.https://github.com.helper"),
                    Some(false)
                ),
                (
                    "GIT_CONFIG_VALUE_1",
                    Some("!gh auth git-credential"),
                    Some(false)
                ),
            ]
        );
    }

    #[test]
    fn clones_borrow_the_token_only_for_github_over_https() {
        let token = SecretString::from("ghp_example");

        assert!(clone_auth("https://github.com/JalapenoLabs/Elysium.git", &token).is_some());
        assert!(clone_auth("git@github.com:JalapenoLabs/Elysium.git", &token).is_none());
        assert!(clone_auth("https://gitlab.com/JalapenoLabs/Elysium.git", &token).is_none());
    }

    /// Regression: a session for `git@github.com:JalapenoLabs/Elysium.git` failed to
    /// provision with `REPO_CLONE_FAILED`, because the satellite tried SSH with no key.
    #[test]
    fn github_ssh_remotes_are_cloned_over_https() {
        let expected = "https://github.com/JalapenoLabs/Elysium.git";
        assert_eq!(
            clone_url("git@github.com:JalapenoLabs/Elysium.git"),
            expected
        );
        assert_eq!(
            clone_url("ssh://git@github.com/JalapenoLabs/Elysium"),
            expected
        );
        assert_eq!(
            clone_url("https://github.com/JalapenoLabs/Elysium"),
            "https://github.com/JalapenoLabs/Elysium"
        );
        assert_eq!(
            clone_url("https://gitlab.com/org/repo.git"),
            "https://gitlab.com/org/repo.git"
        );

        let token = SecretString::from("ghp_example");
        let converted = clone_url("git@github.com:JalapenoLabs/Elysium.git");
        assert!(clone_auth(&converted, &token).is_some());
    }
}
