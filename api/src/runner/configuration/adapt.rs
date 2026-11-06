// Copyright © 2026 Jalapeno Labs

//! Adapts a configuration repository's checkout to the workspace that runs it.
//!
//! A configuration repository is usually a person's own `~/.claude`, written for their machine:
//! hooks that call tools only that machine has, scripts under its home directory, plugins
//! installed from a marketplace into a cache the repository does not carry, MCP servers on its
//! `localhost`, and environment that points Claude Code at a proxy or a credential of its own.
//! A turn in a workspace container cannot use any of that, and some of it would break the turn,
//! such as a credential helper that overrides the one Elysium hands it.
//!
//! After every fetch, [`run`] leaves out what the workspace cannot run and reports each thing it
//! left out as a [`Skipped`], which the configuration's status and every batch show. It edits
//! the checkout's working tree only: the next fetch's `checkout -f` restores the repository's own
//! files, and they are adapted again. What it leaves out:
//!
//! | In                                   | Left out when                                              |
//! |--------------------------------------|------------------------------------------------------------|
//! | `settings.json` `hooks`              | its command is not installed, or a path it names is missing; an HTTP hook on loopback |
//! | `settings.json` `enabledPlugins`     | the plugin is not installed in the checkout                |
//! | `settings.json` `env`                | Elysium sets the key itself (`crate::environment`), or the value is a loopback address |
//! | `settings.json` `apiKeyHelper`       | always: Elysium chooses every turn's credential            |
//! | `.claude.json` `mcpServers`          | its command is not installed, a path it names is missing, or it listens on loopback |
//!
//! `statusLine` is left as it is: turns run headless (`claude -p`), which never draws one.
//!
//! What exists is asked of the workspace itself, in one exec as the agent ([`PROBE_SCRIPT`]),
//! so a command is judged by the `PATH` its hooks would run with. The decisions are pure
//! ([`probes`], then [`adapt`]), so they are tested without a container.

use std::collections::{BTreeSet, HashSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::{Host, Url};

use super::super::WorkspacePath;
use super::super::files::FileError;
use super::super::workspace::{AGENT_USER, SCRIPT_TIMEOUT, WorkspaceContainer};
use crate::environment;

/// Where the checkout lives, relative to the workspace's root.
const CHECKOUT_DIRECTORY: &str = ".claude";

/// The files adapted, relative to the checkout.
const SETTINGS_FILE: &str = "settings.json";
const INSTALLED_PLUGINS_FILE: &str = "plugins/installed_plugins.json";
const USER_STATE_FILE: &str = ".claude.json";

/// The largest file read. A settings file is a few kilobytes; anything near this is not one.
const MAX_FILE_BYTES: u64 = 1_048_576;

/// The longest name a [`Skipped`] keeps, in characters: enough to recognize a hook's command.
const MAX_NAME_CHARACTERS: usize = 200;

/// Each argument is `command:<name>` or `path:<path>`; prints `1` or `0` per argument, in order.
/// A path may start with `~/`, `$HOME/`, or `${HOME}/`, as hooks write them, and is taken from
/// the agent's own home.
const PROBE_SCRIPT: &str = r#"
for probe; do
  value="${probe#*:}"
  case "$value" in
    '~/'*) value="$HOME/${value#'~/'}" ;;
    '$HOME/'*) value="$HOME/${value#'$HOME/'}" ;;
    '${HOME}/'*) value="$HOME/${value#'${HOME}/'}" ;;
  esac
  case "$probe" in
    command:*) command -v -- "$value" >/dev/null 2>&1 && echo 1 || echo 0 ;;
    *) [ -e "$value" ] && echo 1 || echo 0 ;;
  esac
done
"#;

/// Something the checkout held that the workspace cannot run, and so left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub kind: SkippedKind,
    /// What was left out, as the repository names it: a hook's event and its executable or its
    /// address's origin, a plugin, an MCP server, an environment key, or a setting. Never a
    /// hook's arguments or a whole address, which may carry a secret.
    pub name: String,
    pub reason: SkipReason,
    /// The command, path, or address that decided it. Never a value of the environment, which
    /// may be a secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// What kind of thing was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkippedKind {
    Hook,
    Plugin,
    McpServer,
    Environment,
    Setting,
}

/// Why it was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// Its command is not installed in the workspace.
    MissingCommand,
    /// A path it names does not exist in the workspace: one on its owner's machine.
    MissingPath,
    /// It reaches a server on the machine itself, where nothing of its owner's listens.
    Loopback,
    /// The plugin is not installed: plugins come from a marketplace, which the repository does
    /// not carry.
    NotInstalled,
    /// Elysium sets it for every turn: the credential, the runner's wiring, or GitHub's.
    Reserved,
}

/// The files [`adapt`] reads, as text; `None` for one the checkout does not have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Files {
    pub settings: Option<String>,
    pub installed_plugins: Option<String>,
    pub user_state: Option<String>,
}

/// What the workspace is asked: whether a command is installed, or whether a path exists.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Probe {
    Command(String),
    Path(String),
}

impl Probe {
    fn argument(&self) -> String {
        match self {
            Self::Command(name) => format!("command:{name}"),
            Self::Path(path) => format!("path:{path}"),
        }
    }
}

/// The adapted files, each `Some` only when it changed, and everything left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Adapted {
    pub settings: Option<String>,
    pub user_state: Option<String>,
    pub skipped: Vec<Skipped>,
}

/// Why a checkout could not be adapted.
#[derive(Debug, thiserror::Error)]
pub enum AdaptError {
    #[error("{file} in the configuration repository is not a JSON object: {message}")]
    NotAnObject { file: &'static str, message: String },
}

/// Adapts the checkout in `workspace`'s configuration directory and reports what it left out.
///
/// # Errors
/// A file that cannot be read or written, a settings file that is not JSON, or a probe that
/// does not answer.
pub(super) async fn run(workspace: &WorkspaceContainer) -> anyhow::Result<Vec<Skipped>> {
    let root = WorkspacePath::new(CHECKOUT_DIRECTORY).expect("a fixed relative path");
    let files = Files {
        settings: read(workspace, &root, SETTINGS_FILE).await?,
        installed_plugins: read(workspace, &root, INSTALLED_PLUGINS_FILE).await?,
        user_state: read(workspace, &root, USER_STATE_FILE).await?,
    };
    let asked: Vec<Probe> = probes(&files)?.into_iter().collect();
    let found = probe(workspace, &asked).await?;
    let adapted = adapt(&files, &found)?;
    if let Some(settings) = adapted.settings {
        write(workspace, &root, SETTINGS_FILE, settings).await?;
    }
    if let Some(user_state) = adapted.user_state {
        write(workspace, &root, USER_STATE_FILE, user_state).await?;
    }
    Ok(adapted.skipped)
}

/// Everything [`adapt`] needs the workspace to answer for `files`.
///
/// # Errors
/// [`AdaptError`] for a settings file that is not a JSON object.
pub fn probes(files: &Files) -> Result<BTreeSet<Probe>, AdaptError> {
    let mut asked = BTreeSet::new();
    walk(files, &mut |probe: &Probe| {
        asked.insert(probe.clone());
        true
    })?;
    Ok(asked)
}

/// `files` without what the workspace cannot run, given the probes `found` true.
///
/// # Errors
/// [`AdaptError`] for a settings file that is not a JSON object.
pub fn adapt(files: &Files, found: &HashSet<Probe>) -> Result<Adapted, AdaptError> {
    walk(files, &mut |probe: &Probe| found.contains(probe))
}

/// Walks every file once, asking `exists` about each command and path, and leaves out what does
/// not exist. [`probes`] walks with an `exists` that records the question and answers yes, so it
/// asks everything; [`adapt`]'s walk stops asking about a hook once one answer leaves it out, so
/// every question it asks was asked first.
fn walk(files: &Files, exists: &mut dyn FnMut(&Probe) -> bool) -> Result<Adapted, AdaptError> {
    let mut walker = Walker {
        exists,
        skipped: Vec::new(),
    };
    let installed = installed_plugins(files.installed_plugins.as_deref());

    let settings = match files.settings.as_deref() {
        Some(text) => {
            let mut settings = object(SETTINGS_FILE, text)?;
            let changed = [
                walker.hooks(&mut settings),
                walker.plugins(&mut settings, &installed),
                walker.environment(&mut settings),
                walker.credential_helper(&mut settings),
            ]
            .contains(&true);
            changed.then(|| pretty(settings))
        }
        None => None,
    };
    let user_state = match files.user_state.as_deref() {
        Some(text) => {
            let mut user_state = object(USER_STATE_FILE, text)?;
            walker
                .mcp_servers(&mut user_state)
                .then(|| pretty(user_state))
        }
        None => None,
    };
    Ok(Adapted {
        settings,
        user_state,
        skipped: walker.skipped,
    })
}

struct Walker<'a> {
    exists: &'a mut dyn FnMut(&Probe) -> bool,
    skipped: Vec<Skipped>,
}

impl Walker<'_> {
    /// Leaves out every hook that cannot run, then every matcher and event left with none.
    fn hooks(&mut self, settings: &mut Map<String, Value>) -> bool {
        let Some(Value::Object(events)) = settings.get_mut("hooks") else {
            return false;
        };
        let mut changed = false;
        for (event, groups) in events.iter_mut() {
            let Value::Array(groups) = groups else {
                continue;
            };
            for group in groups.iter_mut() {
                let Some(Value::Array(hooks)) = group.get_mut("hooks") else {
                    continue;
                };
                let before = hooks.len();
                hooks.retain(|hook| self.keeps_hook(event, hook));
                changed |= hooks.len() != before;
            }
            groups.retain(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_none_or(|hooks| !hooks.is_empty())
            });
        }
        events.retain(|_event, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
        if events.is_empty() {
            settings.remove("hooks");
        }
        changed
    }

    fn keeps_hook(&mut self, event: &str, hook: &Value) -> bool {
        let refusal = match hook.get("type").and_then(Value::as_str) {
            Some("command") => hook
                .get("command")
                .and_then(Value::as_str)
                .and_then(|command| {
                    let arguments = hook.get("args").and_then(Value::as_array);
                    let words = words(command, arguments.map(Vec::as_slice));
                    let executable = words.first()?.clone();
                    self.unrunnable(&words)
                        .map(|refusal| (format!("{event}: {executable}"), refusal))
                }),
            Some("http") => hook.get("url").and_then(Value::as_str).and_then(|address| {
                loopback_origin(address)
                    .map(|origin| (format!("{event}: {origin}"), (SkipReason::Loopback, origin)))
            }),
            _other => None,
        };
        let Some((name, (reason, detail))) = refusal else {
            return true;
        };
        self.skip(SkippedKind::Hook, &name, reason, Some(detail));
        false
    }

    /// Leaves out every enabled plugin the checkout has not installed.
    fn plugins(
        &mut self,
        settings: &mut Map<String, Value>,
        installed: &[(String, String)],
    ) -> bool {
        let Some(Value::Object(enabled)) = settings.get_mut("enabledPlugins") else {
            return false;
        };
        let mut missing = Vec::new();
        for (plugin, is_enabled) in enabled.iter() {
            if *is_enabled != Value::Bool(true) {
                continue;
            }
            // Every install path is asked, never stopping at the first, so the probing walk
            // asks each one the adapting walk may read.
            let answers: Vec<bool> = installed
                .iter()
                .filter(|(id, _path)| id == plugin)
                .map(|(_id, path)| (self.exists)(&Probe::Path(path.clone())))
                .collect();
            let is_installed = answers.contains(&true);
            if !is_installed {
                missing.push(plugin.clone());
            }
        }
        for plugin in &missing {
            enabled.remove(plugin);
            self.skip(SkippedKind::Plugin, plugin, SkipReason::NotInstalled, None);
        }
        !missing.is_empty()
    }

    /// Leaves out environment Elysium sets itself, and any that points at loopback.
    fn environment(&mut self, settings: &mut Map<String, Value>) -> bool {
        let Some(Value::Object(variables)) = settings.get_mut("env") else {
            return false;
        };
        let mut left_out = Vec::new();
        for (key, value) in variables.iter() {
            if environment::key_refusal(key).is_some() {
                left_out.push((key.clone(), SkipReason::Reserved, None));
                continue;
            }
            if let Some(origin) = value.as_str().and_then(loopback_origin) {
                left_out.push((key.clone(), SkipReason::Loopback, Some(origin)));
            }
        }
        for (key, reason, detail) in &left_out {
            variables.remove(key);
            self.skip(SkippedKind::Environment, key, *reason, detail.clone());
        }
        !left_out.is_empty()
    }

    /// Leaves out a credential helper: Elysium hands every turn its credential, and fails over
    /// between them, which a helper would override.
    fn credential_helper(&mut self, settings: &mut Map<String, Value>) -> bool {
        if settings.remove("apiKeyHelper").is_none() {
            return false;
        }
        self.skip(
            SkippedKind::Setting,
            "apiKeyHelper",
            SkipReason::Reserved,
            None,
        );
        true
    }

    /// Leaves out every MCP server of the user's own that cannot run here.
    fn mcp_servers(&mut self, user_state: &mut Map<String, Value>) -> bool {
        let Some(Value::Object(servers)) = user_state.get_mut("mcpServers") else {
            return false;
        };
        let mut left_out = Vec::new();
        for (name, server) in servers.iter() {
            let refusal = if let Some(address) = server.get("url").and_then(Value::as_str) {
                loopback_origin(address).map(|origin| (SkipReason::Loopback, origin))
            } else if let Some(command) = server.get("command").and_then(Value::as_str) {
                // A server's command and its arguments are always given apart.
                let arguments = server
                    .get("args")
                    .and_then(Value::as_array)
                    .map_or(&[][..], Vec::as_slice);
                self.unrunnable(&words(command, Some(arguments)))
            } else {
                None
            };
            if let Some(refusal) = refusal {
                left_out.push((name.clone(), refusal));
            }
        }
        for (name, (reason, detail)) in &left_out {
            servers.remove(name);
            self.skip(SkippedKind::McpServer, name, *reason, Some(detail.clone()));
        }
        !left_out.is_empty()
    }

    /// Why a command of `words` ([`words`]) cannot run here, with what decided it: its
    /// executable is not installed, or a path among its arguments does not exist.
    fn unrunnable(&mut self, words: &[String]) -> Option<(SkipReason, String)> {
        let (executable, rest) = words.split_first()?;
        let named = if executable.contains('/') {
            Probe::Path(executable.clone())
        } else {
            Probe::Command(executable.clone())
        };
        if !(self.exists)(&named) {
            return Some((SkipReason::MissingCommand, executable.clone()));
        }
        rest.iter()
            .filter(|word| is_checkable_path(word))
            .find(|word| !(self.exists)(&Probe::Path((*word).clone())))
            .map(|word| (SkipReason::MissingPath, word.clone()))
    }

    fn skip(&mut self, kind: SkippedKind, name: &str, reason: SkipReason, detail: Option<String>) {
        self.skipped.push(Skipped {
            kind,
            name: name.chars().take(MAX_NAME_CHARACTERS).collect(),
            reason,
            detail,
        });
    }
}

/// Every installed plugin the checkout records for the user, each with where it is installed.
/// A plugin installed for one project of its owner's is installed for no project here. A file
/// that does not read installs nothing, which leaves every enabled plugin out, as reported.
fn installed_plugins(text: Option<&str>) -> Vec<(String, String)> {
    let Some(Value::Object(plugins)) = text
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|file| file.get("plugins").cloned())
    else {
        return Vec::new();
    };
    let mut installed = Vec::new();
    for (id, entries) in plugins {
        for entry in entries.as_array().into_iter().flatten() {
            if entry.get("scope").and_then(Value::as_str) != Some("user") {
                continue;
            }
            if let Some(path) = entry.get("installPath").and_then(Value::as_str) {
                installed.push((id.clone(), path.to_owned()));
            }
        }
    }
    installed
}

/// `address`'s origin when it names this machine itself, as a server on its owner's machine
/// does. Only the origin is kept, since the rest of an address may carry a secret.
fn loopback_origin(address: &str) -> Option<String> {
    let url = Url::parse(address).ok()?;
    let is_loopback = match url.host()? {
        Host::Domain(domain) => {
            let domain = domain.to_ascii_lowercase();
            domain == "localhost" || domain.ends_with(".localhost")
        }
        Host::Ipv4(address) => address.is_loopback() || address == Ipv4Addr::UNSPECIFIED,
        Host::Ipv6(address) => address.is_loopback() || address == Ipv6Addr::UNSPECIFIED,
    };
    is_loopback.then(|| url.origin().ascii_serialization())
}

/// Whether `word` is a shell's `NAME=value`, which sets a variable before the command.
fn is_assignment(word: &str) -> bool {
    word.split_once('=')
        .is_some_and(|(name, _value)| environment::is_well_formed(name))
}

/// Whether `word` is a whole path whose existence means something: absolute, or under the home
/// directory, with no other variable or glob in it.
fn is_checkable_path(word: &str) -> bool {
    let rest = ["/", "~/", "$HOME/", "${HOME}/"]
        .iter()
        .find_map(|prefix| word.strip_prefix(prefix));
    rest.is_some_and(|rest| !rest.contains(['$', '*', '?', '[', '`']))
}

/// A command as its executable, then its arguments. With `arguments` the command is the
/// executable; without, it is a shell command line, whose leading `NAME=value` assignments are
/// not its executable.
fn words(command: &str, arguments: Option<&[Value]>) -> Vec<String> {
    match arguments {
        Some(arguments) => std::iter::once(command.to_owned())
            .chain(
                arguments
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            )
            .collect(),
        None => shlex::split(command)
            .unwrap_or_default()
            .into_iter()
            .skip_while(|word| is_assignment(word))
            .collect(),
    }
}

fn object(file: &'static str, text: &str) -> Result<Map<String, Value>, AdaptError> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(object)) => Ok(object),
        Ok(_other) => Err(AdaptError::NotAnObject {
            file,
            message: "it holds another kind of value".to_owned(),
        }),
        Err(error) => Err(AdaptError::NotAnObject {
            file,
            message: error.to_string(),
        }),
    }
}

fn pretty(object: Map<String, Value>) -> String {
    let mut text = serde_json::to_string_pretty(&Value::Object(object))
        .expect("a JSON value always serializes");
    text.push('\n');
    text
}

/// Asks the workspace about every probe in one exec.
async fn probe(workspace: &WorkspaceContainer, asked: &[Probe]) -> anyhow::Result<HashSet<Probe>> {
    if asked.is_empty() {
        return Ok(HashSet::new());
    }
    let arguments: Vec<String> = asked.iter().map(Probe::argument).collect();
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let output = workspace
        .run_script(
            AGENT_USER,
            PROBE_SCRIPT,
            &borrowed,
            Vec::new(),
            SCRIPT_TIMEOUT,
        )
        .await?;
    let answers: Vec<&str> = output.output.lines().collect();
    if output.exit_code != Some(0) || answers.len() != asked.len() {
        anyhow::bail!(
            "could not ask the workspace what the configuration needs: {}",
            output.output.trim()
        );
    }
    Ok(asked
        .iter()
        .zip(answers)
        .filter(|(_probe, answer)| *answer == "1")
        .map(|(probe, _answer)| probe.clone())
        .collect())
}

/// `name` under `root` as text, or `None` when the checkout does not have it.
async fn read(
    workspace: &WorkspaceContainer,
    root: &WorkspacePath,
    name: &str,
) -> anyhow::Result<Option<String>> {
    let file = match workspace.read_file(root, name).await {
        Ok(file) => file,
        Err(FileError::NotFound(_path)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if file.content_length > MAX_FILE_BYTES {
        anyhow::bail!("{name} in the configuration repository is larger than 1 MiB");
    }
    let mut bytes = Vec::new();
    let mut body = file.body;
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(Some(String::from_utf8(bytes).map_err(|_not_utf8| {
        anyhow::anyhow!("{name} in the configuration repository is not UTF-8 text")
    })?))
}

async fn write(
    workspace: &WorkspaceContainer,
    root: &WorkspacePath,
    name: &str,
    text: String,
) -> anyhow::Result<()> {
    let body = stream::once(async move { Ok::<_, std::io::Error>(Bytes::from(text)) });
    workspace.write_file(root, name, body).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// This machine's own `~/.claude/settings.json`, in its parts that matter here.
    fn owners_settings() -> Value {
        json!({
            "env": {
                "CLAUDE_CODE_MAX_RETRIES": "1000",
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:8787",
                "SECRET_PROXY_URL": "http://localhost:4444/v1?key=secret-value",
            },
            "apiKeyHelper": "~/.claude/oauth-api-key/print-key.sh",
            "hooks": {
                "SessionStart": [{
                    "matcher": "startup|resume|clear",
                    "hooks": [
                        { "type": "command", "command": "node", "args": ["-e", "import('x')"] },
                        { "type": "command", "command": "paplay ~/.claude/sounds/start.ogg" },
                    ],
                }],
                "Stop": [{
                    "hooks": [
                        { "type": "command", "command": "/home/alex/bin/notify --quiet" },
                    ],
                }],
                "PreToolUse": [{
                    "hooks": [
                        { "type": "command", "command": "FORCE=1 bash ~/.claude/hooks/guard.sh" },
                        { "type": "http", "url": "http://localhost:9000/hook?token=secret-value" },
                    ],
                }],
            },
            "enabledPlugins": {
                "slack@claude-plugins-official": true,
                "kept@local": true,
                "off@claude-plugins-official": false,
            },
            "statusLine": { "type": "command", "command": "node ~/.claude/statusline.mjs" },
            "model": "opus[1m]",
        })
    }

    fn owners_files() -> Files {
        Files {
            settings: Some(owners_settings().to_string()),
            installed_plugins: Some(
                json!({
                    "version": 2,
                    "plugins": {
                        "slack@claude-plugins-official": [{
                            "scope": "user",
                            "installPath": "/home/alex/.claude/plugins/cache/slack/1.4.1",
                        }],
                        "kept@local": [{
                            "scope": "user",
                            "installPath": "/workspace/.claude/plugins/local/kept",
                        }],
                    },
                })
                .to_string(),
            ),
            user_state: Some(
                json!({
                    "mcpServers": {
                        "tollbooth": { "type": "http", "url": "http://localhost:7000/mcp" },
                        "atelier": { "command": "/home/alex/atelier/bin/manager", "args": [] },
                        "docs": { "command": "npx", "args": ["-y", "docs-mcp"] },
                        "remote": { "type": "http", "url": "https://mcp.example.com/mcp" },
                    },
                    "numStartups": 12,
                })
                .to_string(),
            ),
        }
    }

    /// What a worker's container has: Node, bash, and npx, the checkout's own hook script and
    /// plugin, and nothing of the owner's machine.
    fn workspace_has() -> HashSet<Probe> {
        HashSet::from([
            Probe::Command("node".to_owned()),
            Probe::Command("bash".to_owned()),
            Probe::Command("npx".to_owned()),
            Probe::Path("~/.claude/hooks/guard.sh".to_owned()),
            Probe::Path("/workspace/.claude/plugins/local/kept".to_owned()),
        ])
    }

    fn adapted() -> Adapted {
        adapt(&owners_files(), &workspace_has()).expect("adapted")
    }

    fn reasons(adapted: &Adapted) -> Vec<(SkippedKind, &str, SkipReason)> {
        adapted
            .skipped
            .iter()
            .map(|skipped| (skipped.kind, skipped.name.as_str(), skipped.reason))
            .collect()
    }

    #[test]
    fn hooks_another_machine_needs_are_left_out_and_the_rest_kept() {
        let adapted = adapted();
        let settings: Value =
            serde_json::from_str(adapted.settings.as_deref().expect("changed")).expect("json");
        let start = &settings["hooks"]["SessionStart"][0]["hooks"];
        assert_eq!(start.as_array().expect("hooks").len(), 1, "node runs here");
        assert_eq!(start[0]["command"], "node");
        assert!(
            settings["hooks"].get("Stop").is_none(),
            "an event left with no hook goes"
        );
        let guard = &settings["hooks"]["PreToolUse"][0]["hooks"];
        assert_eq!(guard.as_array().expect("hooks").len(), 1);
        assert_eq!(guard[0]["command"], "FORCE=1 bash ~/.claude/hooks/guard.sh");

        let hooks: Vec<_> = reasons(&adapted)
            .into_iter()
            .filter(|(kind, _name, _reason)| *kind == SkippedKind::Hook)
            .collect();
        assert_eq!(
            hooks,
            [
                (
                    SkippedKind::Hook,
                    "PreToolUse: http://localhost:9000",
                    SkipReason::Loopback
                ),
                (
                    SkippedKind::Hook,
                    "SessionStart: paplay",
                    SkipReason::MissingCommand
                ),
                (
                    SkippedKind::Hook,
                    "Stop: /home/alex/bin/notify",
                    SkipReason::MissingCommand
                ),
            ]
        );
    }

    #[test]
    fn a_hook_whose_script_is_missing_is_left_out_with_the_path() {
        let files = Files {
            settings: Some(
                json!({ "hooks": { "Stop": [{ "hooks": [
                    { "type": "command", "command": "bash", "args": ["/home/alex/.local/bin/chime.sh"] },
                ] }] } })
                .to_string(),
            ),
            ..Files::default()
        };
        let adapted = adapt(&files, &workspace_has()).expect("adapted");
        assert_eq!(
            adapted.skipped,
            [Skipped {
                kind: SkippedKind::Hook,
                name: "Stop: bash".to_owned(),
                reason: SkipReason::MissingPath,
                detail: Some("/home/alex/.local/bin/chime.sh".to_owned()),
            }]
        );
        let settings: Value =
            serde_json::from_str(adapted.settings.as_deref().expect("changed")).expect("json");
        assert!(settings.get("hooks").is_none(), "nothing is left of hooks");
    }

    #[test]
    fn only_installed_plugins_stay_enabled() {
        let adapted = adapted();
        let settings: Value =
            serde_json::from_str(adapted.settings.as_deref().expect("changed")).expect("json");
        assert_eq!(
            settings["enabledPlugins"],
            json!({ "kept@local": true, "off@claude-plugins-official": false })
        );
        assert!(reasons(&adapted).contains(&(
            SkippedKind::Plugin,
            "slack@claude-plugins-official",
            SkipReason::NotInstalled
        )));
    }

    #[test]
    fn elysiums_own_environment_and_loopback_addresses_are_left_out_without_their_values() {
        let adapted = adapted();
        let settings: Value =
            serde_json::from_str(adapted.settings.as_deref().expect("changed")).expect("json");
        assert_eq!(
            settings["env"],
            json!({ "CLAUDE_CODE_MAX_RETRIES": "1000" })
        );
        let environment: Vec<&Skipped> = adapted
            .skipped
            .iter()
            .filter(|skipped| skipped.kind == SkippedKind::Environment)
            .collect();
        assert_eq!(environment.len(), 2);
        assert_eq!(environment[0].name, "ANTHROPIC_BASE_URL");
        assert_eq!(environment[0].reason, SkipReason::Reserved);
        assert_eq!(environment[0].detail, None, "a value is never repeated");
        assert_eq!(environment[1].name, "SECRET_PROXY_URL");
        assert_eq!(environment[1].reason, SkipReason::Loopback);
        assert_eq!(
            environment[1].detail.as_deref(),
            Some("http://localhost:4444"),
            "only the origin, never the query that may hold a secret"
        );
    }

    #[test]
    fn a_credential_helper_is_always_left_out_and_the_status_line_kept() {
        let adapted = adapted();
        let settings: Value =
            serde_json::from_str(adapted.settings.as_deref().expect("changed")).expect("json");
        assert!(settings.get("apiKeyHelper").is_none());
        assert!(reasons(&adapted).contains(&(
            SkippedKind::Setting,
            "apiKeyHelper",
            SkipReason::Reserved
        )));
        assert_eq!(settings["statusLine"], owners_settings()["statusLine"]);
        assert_eq!(settings["model"], "opus[1m]");
    }

    #[test]
    fn mcp_servers_on_another_machine_are_left_out() {
        let adapted = adapted();
        let user_state: Value =
            serde_json::from_str(adapted.user_state.as_deref().expect("changed")).expect("json");
        let mut kept: Vec<&String> = user_state["mcpServers"]
            .as_object()
            .expect("servers")
            .keys()
            .collect();
        kept.sort();
        assert_eq!(kept, ["docs", "remote"]);
        assert_eq!(user_state["numStartups"], 12, "the rest of the file stays");
        let servers: Vec<_> = reasons(&adapted)
            .into_iter()
            .filter(|(kind, _name, _reason)| *kind == SkippedKind::McpServer)
            .collect();
        assert_eq!(
            servers,
            [
                (
                    SkippedKind::McpServer,
                    "atelier",
                    SkipReason::MissingCommand
                ),
                (SkippedKind::McpServer, "tollbooth", SkipReason::Loopback),
            ]
        );
    }

    #[test]
    fn a_hooks_name_never_carries_its_arguments_or_its_whole_address() {
        let adapted = adapted();
        for skipped in &adapted.skipped {
            assert!(
                !skipped.name.contains("secret") && !skipped.name.contains(".ogg"),
                "{skipped:?}"
            );
        }
    }

    #[test]
    fn a_plugin_installed_at_its_second_path_stays_enabled() {
        let files = Files {
            settings: Some(json!({ "enabledPlugins": { "a@b": true } }).to_string()),
            installed_plugins: Some(
                json!({ "plugins": { "a@b": [
                    { "scope": "user", "installPath": "/stale" },
                    { "scope": "user", "installPath": "/live" },
                ] } })
                .to_string(),
            ),
            user_state: None,
        };
        let asked = probes(&files).expect("probes");
        assert!(
            asked.contains(&Probe::Path("/live".to_owned())),
            "{asked:?}"
        );
        let found = HashSet::from([Probe::Path("/live".to_owned())]);
        assert_eq!(adapt(&files, &found).expect("adapted"), Adapted::default());
    }

    #[test]
    fn adapting_asks_nothing_the_probes_did_not() {
        let files = owners_files();
        let asked = probes(&files).expect("probes");
        for found in [
            HashSet::new(),
            workspace_has(),
            asked.iter().cloned().collect(),
        ] {
            let mut consulted = BTreeSet::new();
            walk(&files, &mut |probe: &Probe| {
                consulted.insert(probe.clone());
                found.contains(probe)
            })
            .expect("walked");
            assert!(
                consulted.is_subset(&asked),
                "{consulted:?} beyond {asked:?}"
            );
        }
    }

    #[test]
    fn every_command_and_path_a_hook_names_is_asked_once() {
        let files = Files {
            settings: Some(
                json!({ "hooks": {
                    "Stop": [{ "hooks": [
                        { "type": "command", "command": "FORCE=1 paplay ~/.claude/sounds/x.ogg -v" },
                        { "type": "command", "command": "paplay", "args": ["/tmp/a.ogg", "$X/b"] },
                    ] }],
                }, "enabledPlugins": { "a@b": true } })
                .to_string(),
            ),
            installed_plugins: Some(
                json!({ "plugins": { "a@b": [{ "scope": "user", "installPath": "/p/a" }] } })
                    .to_string(),
            ),
            user_state: None,
        };
        assert_eq!(
            probes(&files).expect("probes"),
            BTreeSet::from([
                Probe::Command("paplay".to_owned()),
                Probe::Path("/p/a".to_owned()),
                Probe::Path("/tmp/a.ogg".to_owned()),
                Probe::Path("~/.claude/sounds/x.ogg".to_owned()),
            ]),
            "an assignment is not a command, and a path behind a variable is not asked"
        );
    }

    #[test]
    fn a_checkout_that_runs_here_as_it_is_is_left_unchanged() {
        let files = Files {
            settings: Some(
                json!({ "hooks": { "SessionStart": [{ "hooks": [
                    { "type": "command", "command": "node", "args": ["-e", "1"] },
                ] }] }, "model": "opus" })
                .to_string(),
            ),
            ..Files::default()
        };
        let adapted = adapt(&files, &workspace_has()).expect("adapted");
        assert_eq!(adapted, Adapted::default());
    }

    #[test]
    fn a_settings_file_that_is_not_an_object_is_refused() {
        let files = Files {
            settings: Some("[1, 2]".to_owned()),
            ..Files::default()
        };
        assert!(matches!(
            adapt(&files, &HashSet::new()),
            Err(AdaptError::NotAnObject {
                file: "settings.json",
                ..
            })
        ));
    }

    #[test]
    fn only_whole_paths_without_variables_are_checked() {
        for path in [
            "/etc/hosts",
            "~/.claude/x.sh",
            "$HOME/bin/x",
            "${HOME}/bin/x",
        ] {
            assert!(is_checkable_path(path), "{path} is checked");
        }
        for word in [
            "-e",
            "x.sh",
            "/tmp/*.log",
            "$CLAUDE_PROJECT_DIR/x",
            "~/$USER/x",
        ] {
            assert!(!is_checkable_path(word), "{word} is not");
        }
    }

    #[test]
    fn loopback_means_this_machine_itself() {
        for address in [
            "http://localhost:3000/x",
            "http://127.0.0.1:8080",
            "http://[::1]:9000",
            "http://0.0.0.0:80",
            "http://api.localhost/x",
        ] {
            assert!(loopback_origin(address).is_some(), "{address} is loopback");
        }
        for value in ["https://api.example.com", "1000", "true", "/usr/bin/x"] {
            assert!(loopback_origin(value).is_none(), "{value} is not");
        }
    }
}
