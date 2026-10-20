// Copyright © 2026 Jalapeno Labs

//! How a session's agent gets its GitHub token.
//!
//! A session starts with one token, chosen at three levels: the workspace default, the
//! project's choice, and the session's own choice when it is created. The token reaches
//! the agent as `GH_TOKEN`, which `gh` reads on its own. Plain `git` does not, so every turn
//! also carries git config through the environment (`GIT_CONFIG_COUNT`, git 2.31 and later)
//! that makes `gh` git's credential helper for github.com. Nothing is written into the
//! workspace. The session's repositories are cloned with the same environment.
//!
//! An agent can read every variable in its environment. Elysium scrubs the token from what it
//! records and shows of a turn (`crate::sessions`), but the agent holds the token, which is
//! why a session's token should be scoped to what the session needs.

use anyhow::Context;
use diesel::result::Error as DieselError;
use diesel_async::AsyncPgConnection;
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::errors::ApiError;
use crate::github::Repository;
use crate::models::github_credential;
use crate::models::project::Project;
use crate::runner::EnvironmentEntry;

/// The git config every turn carries, in order. The empty helper first clears any helper
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

/// Resolves which token a new session starts with, or `None` for no token, and checks it
/// opens. Each turn opens it again ([`open_for_session`]).
///
/// # Errors
/// Answers `400` when the session names a token that does not exist. Unlike a model
/// credential, a token that cannot be decrypted is not skipped: the agent would start
/// without the access it was promised, so the session is refused.
pub async fn resolve(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    project: &Project,
    choice: SessionChoice,
) -> Result<Option<Uuid>, ApiError> {
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
    credential
        .token(cipher)
        .context("the stored GitHub token cannot be decrypted")?;
    Ok(Some(credential_id))
}

/// Opens the token a session was started with, or `None` for a session without one or whose
/// token has since been deleted.
///
/// # Errors
/// Propagates any database error, and refuses a token that cannot be decrypted: the agent
/// would work without the access it was promised.
pub async fn open_for_session(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    credential_id: Option<Uuid>,
) -> Result<Option<SecretString>, ApiError> {
    let Some(credential_id) = credential_id else {
        return Ok(None);
    };
    let credential = match github_credential::find(connection, credential_id).await {
        Ok(credential) => credential,
        Err(DieselError::NotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let token = credential
        .token(cipher)
        .context("the stored GitHub token cannot be decrypted")?;
    Ok(Some(token))
}

/// The variables that give the agent the token: `GH_TOKEN` for `gh`, and the git config
/// that routes `git` through `gh`.
pub fn environment(token: &SecretString) -> Vec<EnvironmentEntry> {
    let mut environment = vec![EnvironmentEntry {
        key: "GH_TOKEN".to_owned(),
        value: SecretString::from(token.expose_secret().to_owned()),
    }];
    environment.push(plain("GIT_CONFIG_COUNT", &GIT_CONFIG.len().to_string()));
    for (index, (key, value)) in GIT_CONFIG.iter().enumerate() {
        environment.push(plain(&format!("GIT_CONFIG_KEY_{index}"), key));
        environment.push(plain(&format!("GIT_CONFIG_VALUE_{index}"), value));
    }
    environment
}

/// The URL a repository is cloned from.
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

fn plain(key: &str, value: &str) -> EnvironmentEntry {
    EnvironmentEntry {
        key: key.to_owned(),
        value: SecretString::from(value.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::models::project::{GithubAccess, ProjectCoverFit};
    use crate::test_support::TEST_PERSON_ID;

    fn project(github_access: GithubAccess, github_credential_id: Option<Uuid>) -> Project {
        Project {
            created_by: TEST_PERSON_ID,
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
        let environment = environment(&SecretString::from("github_pat_example"));
        let pairs: Vec<(&str, &str)> = environment
            .iter()
            .map(|entry| (entry.key.as_str(), entry.value.expose_secret()))
            .collect();

        assert_eq!(
            pairs,
            vec![
                ("GH_TOKEN", "github_pat_example"),
                ("GIT_CONFIG_COUNT", "2"),
                ("GIT_CONFIG_KEY_0", "credential.https://github.com.helper"),
                ("GIT_CONFIG_VALUE_0", ""),
                ("GIT_CONFIG_KEY_1", "credential.https://github.com.helper"),
                ("GIT_CONFIG_VALUE_1", "!gh auth git-credential"),
            ]
        );
    }

    /// Regression: a session for `git@github.com:JalapenoLabs/Elysium.git` failed to
    /// clone, because SSH was tried with no key.
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
    }
}
