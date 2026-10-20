// Copyright © 2026 Jalapeno Labs

//! The exact process one turn attempt runs in the workspace.
//!
//! An attempt is `bash -c TURN_SCRIPT` with the turn's directory, working directory, and the
//! `claude` argv as positional parameters, so no prompt or path is ever interpolated into
//! shell text. [`TURN_SCRIPT`] creates the turn's directory, writes the MCP configuration from
//! an environment variable into a private file there, starts `claude` in its own process group
//! (recording the group's id so [`KILL_SCRIPT`] can stop everything the turn spawned), waits,
//! and removes the directory. Secrets travel only in the exec's environment, never in argv,
//! which every process in the container could read from `ps`.

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};

use super::events::HarnessSessionId;
use super::{EnvironmentEntry, McpServer};

/// The variable [`TURN_SCRIPT`] reads the MCP configuration from.
const MCP_CONFIG_VARIABLE: &str = "ELYSIUM_MCP_CONFIG";

/// Runs one attempt. `$1` is the turn's directory, `$2` the working directory, and the rest
/// the `claude` argv.
///
/// `setsid` puts `claude` in a new process group whose id is its pid, so killing `-pid` stops
/// its tools too. Standard input is closed: a headless turn must never wait on a terminal.
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
setsid "$@" < /dev/null &
pid=$!
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
    pub prompt: String,
    pub resume: Option<HarnessSessionId>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt_append: Option<String>,
    pub mcp_servers: Vec<McpServer>,
    pub credential: Option<CredentialSecret>,
    pub environment: Vec<EnvironmentEntry>,
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

    /// `claude`'s own arguments. The prompt follows `-p` directly: `--allowedTools` is
    /// variadic and would swallow a prompt placed after it, so it is passed as one
    /// `--allowedTools=...` argument too.
    fn claude_argv(&self) -> Vec<String> {
        let mut argv = vec![
            "claude".to_owned(),
            "-p".to_owned(),
            self.prompt.clone(),
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
        if let Some(tools) = &self.allowed_tools {
            argv.push(format!("--allowedTools={}", tools.join(",")));
        }
        if let Some(append) = &self.system_prompt_append {
            argv.push("--append-system-prompt".to_owned());
            argv.push(append.clone());
        }
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
            model: Some("claude-opus-5".to_owned()),
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
        }
    }

    #[test]
    fn the_prompt_follows_dash_p_and_the_tool_list_is_one_argument() {
        let argv = command().argv();
        let claude = argv
            .iter()
            .position(|argument| argument == "claude")
            .expect("claude");
        assert_eq!(argv[claude + 1], "-p");
        assert_eq!(
            argv[claude + 2],
            "--allowedTools looks like a flag but is the prompt"
        );
        assert!(argv.contains(&"--allowedTools=Read,mcp__elysia".to_owned()));
        let resume = argv
            .iter()
            .position(|argument| argument == "--resume")
            .expect("--resume");
        assert_eq!(argv[resume + 1], "session-a");
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
