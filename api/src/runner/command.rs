// Copyright © 2026 Jalapeno Labs

//! The exact process one turn attempt runs in the workspace.
//!
//! An attempt is `bash -c TURN_SCRIPT` with the turn's directory, working directory, and the
//! `claude` argv as positional parameters, so no path is ever interpolated into shell text.
//! [`TURN_SCRIPT`] creates the turn's directory, writes the MCP configuration from an
//! environment variable into a private file there, starts `claude` in its own process group
//! (recording the group's id so [`KILL_SCRIPT`] can stop everything the turn spawned), waits,
//! and removes the directory.
//!
//! # Where each part travels
//!
//! - **The prompt** goes on the exec's standard input ([`AttemptCommand::prompt`]), which
//!   `claude -p` reads to its end before it starts. Linux refuses any one argument or
//!   environment variable longer than 128 KiB, and a meeting's transcript, every capture side by
//!   side, runs to several times that; standard input has no such limit.
//! - **Secrets** travel only in the exec's environment, never in argv, which every process in
//!   the container could read from `ps`.
//! - **The system prompt** stays an argument (`--system-prompt`, `--append-system-prompt`): the
//!   CLI takes it no other way. It holds a caller's standing instructions from the code and, for
//!   a worker, its batch's brief, whose goal is bounded. Anything that grows with what a turn
//!   reads belongs in the prompt.

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};

use super::events::HarnessSessionId;
use super::{EnvironmentEntry, McpServer, TurnMode};
use crate::models::model_setting::Effort;

/// The variable [`TURN_SCRIPT`] reads the MCP configuration from.
const MCP_CONFIG_VARIABLE: &str = "ELYSIUM_MCP_CONFIG";

/// Runs one attempt. `$1` is the turn's directory, `$2` the working directory, and the rest
/// the `claude` argv. The prompt is on standard input.
///
/// `setsid` puts `claude` in a new process group whose id is its pid, so killing `-pid` stops
/// its tools too. `claude` reads the script's standard input, the prompt and then its end, so a
/// headless turn never waits on a terminal. A non-interactive shell hands a command started
/// with `&` `/dev/null` for its standard input, ahead of the command's own redirections, so the
/// script keeps its own as descriptor 3 and gives `claude` that one.
/// The directory goes away with the turn, taking the MCP configuration's tokens with it.
pub const TURN_SCRIPT: &str = r#"
turn_directory="$1"; working_directory="$2"; shift 2
umask 077
mkdir -p "$turn_directory" "$working_directory" || exit 70
cd "$working_directory" || exit 70
if [ -n "${ELYSIUM_MCP_CONFIG:-}" ]; then
  printf '%s' "$ELYSIUM_MCP_CONFIG" > "$turn_directory/mcp.json"
  set -- "$@" --mcp-config "$turn_directory/mcp.json"
fi
unset ELYSIUM_MCP_CONFIG
exec 3<&0
setsid "$@" <&3 3<&- &
pid=$!
exec 3<&-
printf '%s' "$pid" > "$turn_directory/pid"
trap 'kill -TERM -- "-$pid" 2>/dev/null' TERM INT
wait "$pid"
status=$?
rm -rf "$turn_directory"
exit "$status"
"#;

/// Stops a turn's process group. `$1` is the turn's directory. A turn that already ended has
/// no directory, and that is not an error.
pub const KILL_SCRIPT: &str = r#"
pid_file="$1/pid"
[ -f "$pid_file" ] || exit 0
pid=$(cat "$pid_file")
kill -TERM -- "-$pid" 2>/dev/null
sleep 5
kill -KILL -- "-$pid" 2>/dev/null
rm -rf "$1"
exit 0
"#;

/// Stops every turn a previous run of the API left behind, at startup. `$1` is the directory
/// holding turn directories.
pub const REAP_SCRIPT: &str = r#"
for pid_file in "$1"/*/pid; do
  [ -f "$pid_file" ] || continue
  kill -KILL -- "-$(cat "$pid_file")" 2>/dev/null
  rm -rf "$(dirname "$pid_file")"
done
exit 0
"#;

/// The credential an attempt runs with.
#[derive(Debug, Clone)]
pub enum CredentialSecret {
    /// `claude setup-token`'s long-lived subscription token.
    SubscriptionToken(SecretString),
    ApiKey(SecretString),
}

/// Everything one attempt needs, already decided.
#[derive(Debug, Clone)]
pub struct AttemptCommand {
    /// Absolute, under the workspace's turn directory.
    pub turn_directory: String,
    /// Absolute, under the workspace.
    pub working_directory: String,
    /// Written whole to the attempt's standard input, never put in argv. Slash commands such as
    /// a worker's `/goal` are read from there as they are from an argument.
    pub prompt: String,
    pub resume: Option<HarnessSessionId>,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt_append: Option<String>,
    pub mcp_servers: Vec<McpServer>,
    pub credential: Option<CredentialSecret>,
    pub environment: Vec<EnvironmentEntry>,
    pub mode: TurnMode,
}

impl AttemptCommand {
    /// The exec's argv: the turn script, then its positional parameters.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec![
            "bash".to_owned(),
            "-c".to_owned(),
            TURN_SCRIPT.to_owned(),
            // `$0`, which names the script in its own error messages.
            "elysium-turn".to_owned(),
            self.turn_directory.clone(),
            self.working_directory.clone(),
        ];
        argv.extend(self.claude_argv());
        argv
    }

    /// `claude`'s own arguments. `-p` with no prompt after it reads the prompt from standard
    /// input. `--allowedTools` is variadic, so the list is one `--allowedTools=...` argument,
    /// which can never take the argument after it as a tool.
    fn claude_argv(&self) -> Vec<String> {
        let mut argv = vec![
            "claude".to_owned(),
            "-p".to_owned(),
            "--output-format".to_owned(),
            "stream-json".to_owned(),
            // stream-json in print mode emits the full event stream only with --verbose.
            "--verbose".to_owned(),
            "--permission-mode".to_owned(),
            "bypassPermissions".to_owned(),
        ];
        if let Some(session) = &self.resume {
            argv.push("--resume".to_owned());
            argv.push(session.to_string());
        }
        if let Some(model) = &self.model {
            argv.push("--model".to_owned());
            argv.push(model.clone());
        }
        if let Some(effort) = self.effort {
            argv.push("--effort".to_owned());
            argv.push(effort.as_str().to_owned());
        }
        if let Some(tools) = &self.allowed_tools {
            argv.push(format!("--allowedTools={}", tools.join(",")));
        }
        if let Some(append) = &self.system_prompt_append {
            argv.push("--append-system-prompt".to_owned());
            argv.push(append.clone());
        }
        let (system_prompt, keeps_session) = match &self.mode {
            TurnMode::Agent => return argv,
            TurnMode::Text { system_prompt } => (system_prompt, false),
            TurnMode::ResumableText { system_prompt } => (system_prompt, true),
        };
        argv.extend([
            // An empty list removes every built-in tool.
            "--tools=".to_owned(),
            // Only the request's own MCP servers, never ones from settings or plugins.
            "--strict-mcp-config".to_owned(),
            // No user, project, or local settings: the agents' configuration repository must
            // not steer a text turn.
            "--setting-sources=".to_owned(),
        ]);
        if !keeps_session {
            argv.push("--no-session-persistence".to_owned());
        }
        argv.extend(["--system-prompt".to_owned(), system_prompt.clone()]);
        argv
    }

    /// The exec's environment as `KEY=value` strings. The turn's own variables come first,
    /// so the credential and the MCP configuration Elysium sets win over a variable of the
    /// same name.
    pub fn environment(&self) -> Vec<String> {
        let mut environment: Vec<String> = self
            .environment
            .iter()
            .map(|entry| format!("{}={}", entry.key, entry.value.expose_secret()))
            .collect();
        match &self.credential {
            Some(CredentialSecret::SubscriptionToken(token)) => {
                environment.push(format!("CLAUDE_CODE_OAUTH_TOKEN={}", token.expose_secret()));
            }
            Some(CredentialSecret::ApiKey(key)) => {
                environment.push(format!("ANTHROPIC_API_KEY={}", key.expose_secret()));
            }
            None => {}
        }
        if !self.mcp_servers.is_empty() {
            environment.push(format!(
                "{MCP_CONFIG_VARIABLE}={}",
                mcp_config(&self.mcp_servers)
            ));
        }
        environment
    }
}

/// Claude Code's `--mcp-config` document for `servers`.
fn mcp_config(servers: &[McpServer]) -> Value {
    let mut entries = Map::new();
    for server in servers {
        let (name, entry) = match server {
            McpServer::Http { name, url, headers } => {
                let headers: Map<String, Value> = headers
                    .iter()
                    .map(|(header, value)| {
                        (
                            header.clone(),
                            Value::String(value.expose_secret().to_owned()),
                        )
                    })
                    .collect();
                (
                    name,
                    json!({ "type": "http", "url": url.as_str(), "headers": headers }),
                )
            }
            McpServer::Stdio {
                name,
                command,
                arguments,
                environment,
            } => {
                let environment: Map<String, Value> = environment
                    .iter()
                    .map(|entry| {
                        (
                            entry.key.clone(),
                            Value::String(entry.value.expose_secret().to_owned()),
                        )
                    })
                    .collect();
                (
                    name,
                    json!({ "type": "stdio", "command": command, "args": arguments, "env": environment }),
                )
            }
        };
        entries.insert(name.clone(), entry);
    }
    json!({ "mcpServers": entries })
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;

    fn command() -> AttemptCommand {
        AttemptCommand {
            turn_directory: "/workspace/.elysium/turns/t1".to_owned(),
            working_directory: "/workspace/sessions/7".to_owned(),
            prompt: "--allowedTools looks like a flag but is the prompt".to_owned(),
            resume: Some("session-a".parse().expect("an id")),
            model: Some("claude-opus-5-5".to_owned()),
            effort: Some(Effort::Xhigh),
            allowed_tools: Some(vec!["Read".to_owned(), "mcp__elysia".to_owned()]),
            system_prompt_append: None,
            mcp_servers: vec![McpServer::Http {
                name: "elysia".to_owned(),
                url: Url::parse("http://api:8080/api/elysia/mcp").expect("a url"),
                headers: vec![(
                    "Authorization".to_owned(),
                    SecretString::from("Bearer turn-token"),
                )],
            }],
            credential: Some(CredentialSecret::SubscriptionToken(SecretString::from(
                "sk-ant-oat01-x",
            ))),
            environment: vec![EnvironmentEntry {
                key: "CLAUDE_CODE_OAUTH_TOKEN".to_owned(),
                value: SecretString::from("from-a-variable"),
            }],
            mode: TurnMode::Agent,
        }
    }

    #[test]
    fn an_agent_turn_keeps_its_tools_and_settings() {
        let argv = command().argv();
        assert!(!argv.iter().any(|argument| argument.starts_with("--tools")));
        assert!(
            !argv
                .iter()
                .any(|argument| argument.starts_with("--setting-sources"))
        );
        assert!(!argv.contains(&"--no-session-persistence".to_owned()));
    }

    #[test]
    fn a_text_turn_has_no_tools_settings_or_session_and_its_own_system_prompt() {
        let argv = AttemptCommand {
            mode: TurnMode::Text {
                system_prompt: "Summarize the log.".to_owned(),
            },
            ..command()
        }
        .argv();
        for flag in [
            "--tools=",
            "--strict-mcp-config",
            "--setting-sources=",
            "--no-session-persistence",
        ] {
            assert!(argv.contains(&flag.to_owned()), "{flag}");
        }
        let system_prompt = argv
            .iter()
            .position(|argument| argument == "--system-prompt")
            .expect("--system-prompt");
        assert_eq!(argv[system_prompt + 1], "Summarize the log.");
    }

    #[test]
    fn a_resumable_text_turn_keeps_its_session_and_resumes_the_one_named() {
        let argv = AttemptCommand {
            mode: TurnMode::ResumableText {
                system_prompt: "Read the conversation.".to_owned(),
            },
            ..command()
        }
        .argv();
        for flag in ["--tools=", "--strict-mcp-config", "--setting-sources="] {
            assert!(argv.contains(&flag.to_owned()), "{flag}");
        }
        assert!(
            !argv.contains(&"--no-session-persistence".to_owned()),
            "the next turn resumes what this one keeps"
        );
        let resume = argv
            .iter()
            .position(|argument| argument == "--resume")
            .expect("--resume");
        assert_eq!(argv[resume + 1], "session-a");
        let system_prompt = argv
            .iter()
            .position(|argument| argument == "--system-prompt")
            .expect("--system-prompt");
        assert_eq!(argv[system_prompt + 1], "Read the conversation.");
    }

    #[test]
    fn the_model_and_effort_are_passed_when_chosen_and_left_to_the_cli_otherwise() {
        let argv = command().argv();
        let model = argv
            .iter()
            .position(|argument| argument == "--model")
            .expect("--model");
        assert_eq!(argv[model + 1], "claude-opus-5-5");
        let effort = argv
            .iter()
            .position(|argument| argument == "--effort")
            .expect("--effort");
        assert_eq!(argv[effort + 1], "xhigh");

        let unchosen = AttemptCommand {
            model: None,
            effort: None,
            ..command()
        }
        .argv();
        assert!(!unchosen.contains(&"--model".to_owned()));
        assert!(!unchosen.contains(&"--effort".to_owned()));
    }

    #[test]
    fn the_prompt_is_never_an_argument_and_the_tool_list_is_one() {
        let argv = command().argv();
        assert!(
            !argv
                .iter()
                .any(|argument| argument.contains("is the prompt")),
            "the prompt travels on standard input"
        );
        let claude = argv
            .iter()
            .position(|argument| argument == "claude")
            .expect("claude");
        assert_eq!(argv[claude + 1], "-p");
        assert_eq!(
            argv[claude + 2],
            "--output-format",
            "-p reads its prompt from standard input when none follows it"
        );
        assert!(argv.contains(&"--allowedTools=Read,mcp__elysia".to_owned()));
        let resume = argv
            .iter()
            .position(|argument| argument == "--resume")
            .expect("--resume");
        assert_eq!(argv[resume + 1], "session-a");
    }

    /// Longer than Linux allows any one argument (`MAX_ARG_STRLEN`, 128 KiB), with what a shell
    /// would trip on if it were ever interpolated, and characters of several bytes.
    fn prompt_over_the_argument_limit() -> String {
        let line = "[12] [3:04] Ada: \"Ship it\" $HOME `date` --resume ☃ é\n";
        let prompt = line.repeat(300 * 1024 / line.len() + 1);
        assert!(prompt.len() > 128 * 1024);
        prompt
    }

    #[test]
    fn a_long_prompt_leaves_every_argument_short() {
        let attempt = AttemptCommand {
            prompt: prompt_over_the_argument_limit(),
            ..command()
        };
        let longest = attempt.argv().iter().map(String::len).max().unwrap_or(0);
        assert!(
            longest < 128 * 1024,
            "an argument of {longest} bytes would be refused"
        );
    }

    /// The turn script as the workspace runs it, through a real `bash` and `setsid`, with a
    /// stand-in `claude` that keeps what it read on standard input: a prompt over the argument
    /// limit arrives whole and byte for byte. Before prompts travelled on standard input, this
    /// one could not even be started (`E2BIG`).
    #[tokio::test]
    async fn a_prompt_over_the_argument_limit_reaches_the_cli_intact() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::AsyncWriteExt;

        let scratch = tempfile::tempdir().expect("a scratch directory");
        let bin = scratch.path().join("bin");
        std::fs::create_dir(&bin).expect("bin");
        let stand_in = bin.join("claude");
        std::fs::write(&stand_in, "#!/bin/sh\ncat > received\n").expect("the stand-in");
        std::fs::set_permissions(&stand_in, std::fs::Permissions::from_mode(0o755))
            .expect("executable");

        let working_directory = scratch.path().join("session");
        let attempt = AttemptCommand {
            turn_directory: scratch.path().join("turn").display().to_string(),
            working_directory: working_directory.display().to_string(),
            prompt: prompt_over_the_argument_limit(),
            ..command()
        };
        let argv = attempt.argv();
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut process = tokio::process::Command::new(&argv[0])
            .args(&argv[1..])
            .env_clear()
            .env("PATH", path)
            .envs(
                attempt
                    .environment()
                    .iter()
                    .filter_map(|entry| entry.split_once('=')),
            )
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("the turn script starts");
        let mut input = process.stdin.take().expect("standard input");
        input
            .write_all(attempt.prompt.as_bytes())
            .await
            .expect("the prompt is written");
        drop(input);
        let status = process.wait().await.expect("the turn script ends");

        assert!(status.success(), "{status}");
        let received = std::fs::read(working_directory.join("received")).expect("what claude read");
        assert_eq!(received.len(), attempt.prompt.len());
        assert!(
            received == attempt.prompt.as_bytes(),
            "the prompt arrived intact"
        );
        assert!(
            !scratch.path().join("turn").exists(),
            "the turn's directory goes with it"
        );
    }

    #[test]
    fn secrets_stay_out_of_argv() {
        let argv = command().argv().join(" ");
        assert!(!argv.contains("sk-ant-oat01-x"));
        assert!(!argv.contains("turn-token"));
    }

    #[test]
    fn elysiums_credential_wins_over_a_variable_of_the_same_name() {
        let environment = command().environment();
        let tokens: Vec<&String> = environment
            .iter()
            .filter(|entry| entry.starts_with("CLAUDE_CODE_OAUTH_TOKEN="))
            .collect();
        assert_eq!(
            tokens.last().map(|entry| entry.as_str()),
            Some("CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-x"),
            "a later entry overrides an earlier one in an exec environment"
        );
    }

    #[test]
    fn the_mcp_configuration_names_each_server_with_its_transport() {
        let config = mcp_config(&command().mcp_servers);
        assert_eq!(
            config,
            json!({ "mcpServers": { "elysia": {
                "type": "http",
                "url": "http://api:8080/api/elysia/mcp",
                "headers": { "Authorization": "Bearer turn-token" },
            } } })
        );

        let stdio = mcp_config(&[McpServer::Stdio {
            name: "blender".to_owned(),
            command: "elysium-blender-mcp".to_owned(),
            arguments: vec!["--headless".to_owned()],
            environment: Vec::new(),
        }]);
        assert_eq!(
            stdio["mcpServers"]["blender"],
            json!({ "type": "stdio", "command": "elysium-blender-mcp", "args": ["--headless"], "env": {} })
        );
    }

    #[test]
    fn a_fresh_turn_has_no_resume() {
        let mut fresh = command();
        fresh.resume = None;
        assert!(!fresh.argv().contains(&"--resume".to_owned()));
    }
}
