// Copyright © 2026 Jalapeno Labs

//! Signing a Claude account in through the official CLI, in two steps a person drives.
//!
//! The CLI's own login is the only sign-in: Elysium never runs an OAuth flow itself. With no
//! terminal, `claude auth login` prints a manual sign-in link whose redirect is Anthropic's own
//! page (`platform.claude.com/oauth/code/callback`), which shows a code, and then waits at
//! `Paste code here if prompted >` for that code on stdin. A person's browser cannot reach a
//! callback on the host, so that manual flow is the one used (verified on CLI 2.1.292).
//!
//! 1. [`SignIns::start`] runs the CLI in the background in the workspace container, reading a
//!    FIFO, and answers the link it printed.
//! 2. [`SignIns::finish`] writes the code the person pasted into the FIFO, waits for the CLI to
//!    exit, and reads the `.credentials.json` it wrote.
//!
//! Everything runs as root, in a directory only root can read (`/root/.elysium/sign-ins`), with
//! its own throwaway `CLAUDE_CONFIG_DIR`: agent turns run as `agent` and can never see a login in
//! progress or the tokens it produced. The directory is removed once the credentials are read,
//! on cancel, and, for one abandoned in between, by the next sign-in.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::Deserialize;
use tracing::{Level, event};
use uuid::Uuid;

use crate::models::claude_account::Tokens;
use crate::runner::workspace::{WorkspaceContainer, WorkspaceError};

/// Where sign-ins in progress live, inside the workspace container. Under root's home, so no
/// agent can read it.
const SIGN_IN_ROOT: &str = "/root/.elysium/sign-ins";

/// How long a sign-in may wait for its code. Long enough to sign in and pass two-factor
/// authentication; the CLI is killed after it, and the directory removed by the next sign-in.
pub const SIGN_IN_LIFETIME: Duration = Duration::from_mins(15);

/// How many sign-ins may wait at once. Each holds one CLI process in the workspace.
const MAX_PENDING: usize = 8;

/// How long a script may take: starting waits up to 30 seconds for the link, and finishing up
/// to 90 seconds for the CLI's token exchange.
const SCRIPT_TIMEOUT: Duration = Duration::from_mins(2);

/// Starts the CLI reading the FIFO and prints `url=` and the manual sign-in link, or the CLI's
/// own output when it ended without one. `$1` is the sign-in's directory, `$2` the lifetime in
/// seconds.
///
/// `<>` opens the FIFO read-write, so opening it never blocks for want of a writer, and the
/// CLI reads the code as soon as one is written. `setsid` gives the CLI a process group of its
/// own, named by `pid`, so cancelling kills all of it, and lets it outlive this exec.
const START_SCRIPT: &str = r#"
set -euo pipefail
root="$(dirname "$1")"
mkdir -p -m 700 "$root"
# A sign-in abandoned longer than its lifetime is removed; its CLI is long gone.
find "$root" -mindepth 1 -maxdepth 1 -type d -mmin +20 -exec rm -rf {} +
mkdir -m 700 "$1"
mkfifo -m 600 "$1/code"
setsid bash -c '
  echo $$ > "$1/pid"
  CLAUDE_CONFIG_DIR="$1/config" BROWSER=/bin/true timeout "$2" claude auth login <>"$1/code" >"$1/output" 2>&1
  echo $? > "$1/exit"
' elysium-sign-in "$1" "$2" </dev/null >/dev/null 2>&1 &
for _ in $(seq 1 120); do
  url="$(sed -n 's/.*visit: \(https:\/\/[^[:space:]]*\).*/\1/p' "$1/output" 2>/dev/null | head -n 1)"
  if [ -n "$url" ]; then
    echo "url=$url"
    exit 0
  fi
  if [ -f "$1/exit" ]; then
    break
  fi
  sleep 0.25
done
tail -c 600 "$1/output" 2>/dev/null || true
exit 1
"#;

/// Writes the code (from `ELYSIUM_SIGN_IN_CODE`, so it never appears in a process list) to the
/// CLI and prints the credentials it wrote, or its output when it failed. `$1` is the sign-in's
/// directory.
///
/// Writing a FIFO whose reader has gone would block forever, so the write is bounded; the CLI
/// ending first is reported as the sign-in having expired.
const FINISH_SCRIPT: &str = r#"
set -uo pipefail
if [ ! -p "$1/code" ]; then
  echo "no such sign-in"
  exit 3
fi
if [ -f "$1/exit" ]; then
  echo "the sign-in ended before its code arrived"
  exit 4
fi
if ! timeout 5 bash -c 'printf "%s\n" "$ELYSIUM_SIGN_IN_CODE" > "$1/code"' elysium-code "$1"; then
  echo "the sign-in ended before its code arrived"
  exit 4
fi
for _ in $(seq 1 180); do
  [ -f "$1/exit" ] && break
  sleep 0.5
done
if [ ! -f "$1/exit" ]; then
  echo "the CLI did not finish signing in"
  exit 5
fi
if [ "$(cat "$1/exit")" != "0" ]; then
  tail -c 600 "$1/output"
  exit 6
fi
cat "$1/config/.credentials.json"
"#;

/// Kills a sign-in's CLI and removes its directory. `$1` is the sign-in's directory.
const CLEAN_UP_SCRIPT: &str = r#"
if [ -f "$1/pid" ]; then
  kill -- "-$(cat "$1/pid")" 2>/dev/null || true
fi
rm -rf "$1"
"#;

/// The root account, which alone can read [`SIGN_IN_ROOT`].
const ROOT_USER: &str = "root";

/// Why a sign-in could not start or finish.
#[derive(Debug, thiserror::Error)]
pub enum SignInError {
    #[error("no such sign-in is waiting; start again")]
    NotFound,
    #[error("too many sign-ins are waiting; finish or cancel one first")]
    TooMany,
    #[error("the workspace is not available: {0}")]
    Workspace(#[from] WorkspaceError),
    #[error("the workspace is not running yet; the engine starts it, so try again shortly")]
    WorkspaceNotRunning,
    /// The CLI refused, with its own words, such as an invalid code.
    #[error("{0}")]
    Refused(String),
}

/// A sign-in waiting for its code.
#[derive(Debug, Clone)]
pub struct Started {
    pub id: Uuid,
    /// Opens Anthropic's sign-in page; the page it lands on shows the code to paste back.
    pub authorize_url: String,
    pub expires_at: DateTime<Utc>,
}

/// What the CLI stored once the code was accepted.
#[derive(Debug)]
pub struct Finished {
    pub tokens: Tokens,
    pub subscription_type: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    started_by: Uuid,
    started_at: Instant,
}

/// Sign-ins in progress. Cheap to clone; clones share the same pending sign-ins.
///
/// What is pending is kept in memory, in the API process that started it, because the CLI
/// processes it tracks live only as long as the workspace container does. After an API restart,
/// a person simply starts again. It is one of the things that keep the API to one replica:
/// finishing a sign-in must reach the replica that started it.
#[derive(Debug, Clone)]
pub struct SignIns {
    workspace: WorkspaceContainer,
    pending: Arc<Mutex<HashMap<Uuid, Pending>>>,
}

impl SignIns {
    pub fn new(workspace: WorkspaceContainer) -> Self {
        Self {
            workspace,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Starts a sign-in for `person`.
    ///
    /// # Errors
    /// [`SignInError`] when too many are waiting, the workspace is down, or the CLI printed no
    /// sign-in link.
    pub async fn start(&self, person: Uuid) -> Result<Started, SignInError> {
        {
            let mut pending = self.lock();
            pending.retain(|_id, sign_in| sign_in.started_at.elapsed() < SIGN_IN_LIFETIME);
            if pending.len() >= MAX_PENDING {
                return Err(SignInError::TooMany);
            }
        }

        // The engine creates and replaces the workspace; a sign-in only runs in it.
        if !self.workspace.is_running().await? {
            return Err(SignInError::WorkspaceNotRunning);
        }
        let id = Uuid::now_v7();
        let directory = directory(id);
        let lifetime = SIGN_IN_LIFETIME.as_secs().to_string();
        let output = self
            .workspace
            .run_script(
                ROOT_USER,
                START_SCRIPT,
                &[&directory, &lifetime],
                Vec::new(),
                SCRIPT_TIMEOUT,
            )
            .await?;
        let authorize_url = output
            .output
            .lines()
            .find_map(|line| line.strip_prefix("url="))
            .filter(|_url| output.exit_code == Some(0))
            .map(str::to_owned);
        let Some(authorize_url) = authorize_url else {
            self.clean_up(id).await;
            return Err(SignInError::Refused(format!(
                "the Claude CLI did not print a sign-in link: {}",
                output.output.trim()
            )));
        };

        self.lock().insert(
            id,
            Pending {
                started_by: person,
                started_at: Instant::now(),
            },
        );
        event!(
            name: "claude_accounts.sign_in.started",
            Level::INFO,
            sign_in.id = %id,
            user.id = %person,
            "a Claude sign-in is waiting for its code",
        );
        Ok(Started {
            id,
            authorize_url,
            expires_at: Utc::now()
                + chrono::TimeDelta::from_std(SIGN_IN_LIFETIME).unwrap_or_default(),
        })
    }

    /// Hands `code` to `person`'s sign-in `id` and reads what the CLI stored. The sign-in is
    /// over either way: a wrong code means starting again, since the CLI exits on it.
    ///
    /// # Errors
    /// [`SignInError::NotFound`] for a sign-in that is not waiting or is someone else's, and
    /// [`SignInError::Refused`] with the CLI's words when it rejected the code.
    pub async fn finish(
        &self,
        id: Uuid,
        person: Uuid,
        code: &SecretString,
    ) -> Result<Finished, SignInError> {
        self.take(id, person)?;
        let result = self.exchange(id, code).await;
        self.clean_up(id).await;
        result
    }

    /// Ends `person`'s sign-in `id` without finishing it.
    ///
    /// # Errors
    /// [`SignInError::NotFound`] for a sign-in that is not waiting or is someone else's.
    pub async fn cancel(&self, id: Uuid, person: Uuid) -> Result<(), SignInError> {
        self.take(id, person)?;
        self.clean_up(id).await;
        Ok(())
    }

    async fn exchange(&self, id: Uuid, code: &SecretString) -> Result<Finished, SignInError> {
        use secrecy::ExposeSecret;

        let output = self
            .workspace
            .run_script(
                ROOT_USER,
                FINISH_SCRIPT,
                &[&directory(id)],
                vec![format!(
                    "ELYSIUM_SIGN_IN_CODE={}",
                    code.expose_secret().trim()
                )],
                SCRIPT_TIMEOUT,
            )
            .await?;
        if output.exit_code != Some(0) {
            return Err(SignInError::Refused(cli_refusal(&output.output)));
        }
        read_credentials(&output.output)
    }

    /// Removes a pending sign-in, checking it belongs to `person`.
    fn take(&self, id: Uuid, person: Uuid) -> Result<(), SignInError> {
        let mut pending = self.lock();
        match pending.get(&id) {
            Some(sign_in)
                if sign_in.started_by == person
                    && sign_in.started_at.elapsed() < SIGN_IN_LIFETIME =>
            {
                pending.remove(&id);
                Ok(())
            }
            _ => Err(SignInError::NotFound),
        }
    }

    async fn clean_up(&self, id: Uuid) {
        if let Err(error) = self
            .workspace
            .run_script(
                ROOT_USER,
                CLEAN_UP_SCRIPT,
                &[&directory(id)],
                Vec::new(),
                SCRIPT_TIMEOUT,
            )
            .await
        {
            // The next sign-in removes anything left after its lifetime.
            event!(
                name: "claude_accounts.sign_in.clean_up_failed",
                Level::WARN,
                sign_in.id = %id,
                error.message = %error,
                "a finished sign-in's directory could not be removed",
            );
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Pending>> {
        self.pending
            .lock()
            .expect("the sign-in lock is never poisoned")
    }
}

fn directory(id: Uuid) -> String {
    format!("{SIGN_IN_ROOT}/{id}")
}

/// The CLI's own reason, as it printed it: its last line, such as
/// `Login failed: Request failed with status code 400`.
fn cli_refusal(output: &str) -> String {
    let reason = output
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(|line| {
            line.rsplit_once("> ")
                .map_or(line, |(_prompt, reason)| reason)
                .to_owned()
        })
        .unwrap_or_default();
    if reason.contains("status code 400") {
        return "Anthropic did not accept that code. Start again and paste the code the sign-in page shows."
            .to_owned();
    }
    if reason.is_empty() {
        return "the Claude CLI could not finish signing in".to_owned();
    }
    reason
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsFile {
    claude_ai_oauth: OAuthBlock,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthBlock {
    access_token: String,
    refresh_token: String,
    /// Unix milliseconds.
    expires_at: i64,
    /// Unix milliseconds, when the CLI recorded one.
    refresh_token_expires_at: Option<i64>,
    subscription_type: Option<String>,
}

/// The tokens in the `.credentials.json` the CLI wrote, as Claude Code stores them.
fn read_credentials(text: &str) -> Result<Finished, SignInError> {
    let file: CredentialsFile = serde_json::from_str(text.trim()).map_err(|_shape| {
        SignInError::Refused(
            "the Claude CLI signed in but stored no credentials Elysium can read".to_owned(),
        )
    })?;
    let block = file.claude_ai_oauth;
    let instant = |milliseconds: i64| DateTime::from_timestamp_millis(milliseconds);
    let access_token_expires_at = instant(block.expires_at).ok_or_else(|| {
        SignInError::Refused("the Claude CLI stored an unreadable expiry".to_owned())
    })?;
    Ok(Finished {
        tokens: Tokens {
            access_token: SecretString::from(block.access_token),
            access_token_expires_at,
            refresh_token: SecretString::from(block.refresh_token),
            refresh_token_expires_at: block.refresh_token_expires_at.and_then(instant),
        },
        subscription_type: block.subscription_type,
    })
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn credentials_are_read_as_claude_code_stores_them() {
        let finished = read_credentials(
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-a","refreshToken":"sk-ant-ort01-r","expiresAt":1793678400000,"scopes":["user:inference"],"subscriptionType":"max"},"mcpOAuth":{}}"#,
        )
        .expect("readable");

        assert_eq!(
            finished.tokens.access_token.expose_secret(),
            "sk-ant-oat01-a"
        );
        assert_eq!(
            finished.tokens.refresh_token.expose_secret(),
            "sk-ant-ort01-r"
        );
        assert_eq!(
            finished.tokens.access_token_expires_at.timestamp_millis(),
            1_793_678_400_000
        );
        assert_eq!(finished.tokens.refresh_token_expires_at, None);
        assert_eq!(finished.subscription_type.as_deref(), Some("max"));
    }

    #[test]
    fn a_file_without_tokens_is_refused_without_echoing_it() {
        let error = read_credentials(r#"{"claudeAiOauth":{"accessToken":"secret-only"}}"#)
            .expect_err("incomplete");
        assert!(!error.to_string().contains("secret-only"));
    }

    #[test]
    fn the_clis_refusal_is_read_from_its_last_line() {
        let output = "Opening browser to sign in…\nIf the browser didn't open, visit: https://claude.com/x\nPaste code here if prompted > Login failed: Request failed with status code 400";
        assert!(cli_refusal(output).contains("did not accept that code"));
        assert_eq!(
            cli_refusal("Paste code here if prompted > Login failed: network down"),
            "Login failed: network down"
        );
        assert_eq!(
            cli_refusal("the sign-in ended before its code arrived\n"),
            "the sign-in ended before its code arrived"
        );
    }
}
