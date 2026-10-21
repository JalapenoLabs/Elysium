// Copyright © 2026 Jalapeno Labs

//! `elysium_passwords`: the agent's tools for signing in to things with the logins in the
//! password vault's one collection for agents. See `docs/passwords.md`.
//!
//! The agent never holds the vault: Elysium's own CLI answers each call, and every call reads
//! the chosen collection from the database as it is now, then checks the item against it
//! (`crate::passwords::items::in_collection`), so an item moved out of the collection or a
//! collection changed mid-turn is refused at once. A TOTP seed is never shown; the tool hands
//! out the current code. Every password and code handed out is scrubbed from what turns print
//! (`crate::passwords::Revealed`).
//!
//! Logs name each call by caller, tool, and item id, never by what it returned.

use futures_util::FutureExt as _;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::{CallScope, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments};
use crate::models::password_vault;
use crate::passwords::VaultError;
use crate::passwords::cli::CliError;

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_passwords",
    instructions: "Logins from the password vault that Elysium keeps for agents, for signing in \
        to websites and services on your own. Only the logins in the one collection reserved for \
        agents are reachable. Find a login with passwords_find (by the site's URL, or by words \
        in its name or username), then call passwords_login with its id for the username and \
        password, and passwords_totp for the current one-time code when the site asks for \
        one. Passwords and codes are hidden from everything Elysium records of your turn, but \
        never print, commit, or write them to a file: type them straight into the sign-in \
        form. When no login fits, say so and ask; never guess or reuse another site's \
        password.",
    tools: &TOOLS,
};

const TOOLS: [Tool; 3] = [
    Tool {
        name: "passwords_find",
        description: "Finds logins in the agents' collection by the URL of the site to sign in \
            to, by words in a login's name, username, or URI, or both. Answers each login's id, \
            name, username, URIs, and whether it has a one-time code, never a password. At most \
            25 logins.",
        input_schema: find_schema,
        run: |context, scope, arguments| run_find(context, scope, arguments).boxed(),
    },
    Tool {
        name: "passwords_login",
        description: "The username and password of one login in the agents' collection, by the \
            id passwords_find answered. Type them into the sign-in form; never print them.",
        input_schema: item_schema,
        run: |context, scope, arguments| run_login(context, scope, arguments).boxed(),
    },
    Tool {
        name: "passwords_totp",
        description: "The current one-time code (TOTP) of one login in the agents' collection, \
            for a site that asks for a code after the password. Codes change every 30 seconds, \
            so fetch it right before entering it.",
        input_schema: item_schema,
        run: |context, scope, arguments| run_totp(context, scope, arguments).boxed(),
    },
];

fn find_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "minLength": 1,
                "maxLength": 2048,
                "description": "The address of the site to sign in to, such as \
                    https://example.com/login. Bitwarden matches it against each login's URIs.",
            },
            "search": {
                "type": "string",
                "minLength": 1,
                "maxLength": 200,
                "description": "Words in the login's name, username, or URI.",
            },
        },
        "additionalProperties": false,
    })
}

fn item_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "itemId": {
                "type": "string",
                "format": "uuid",
                "description": "The login's id, from passwords_find.",
            },
        },
        "required": ["itemId"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FindArguments {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    search: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemArguments {
    item_id: Uuid,
}

/// What the agent reads for a vault failure: the CLI's refusal in its words, what to call
/// instead for an item outside the collection, and a fixed message for Elysium's own failures.
fn tool_error(error: VaultError, scope: &CallScope, operation: &'static str) -> ToolError {
    match error {
        VaultError::Cli(CliError::Refused(message)) => ToolError::Provider(message),
        VaultError::NotInCollection(id) => ToolError::NotFound(format!(
            "login {id} is not in the collection agents may read; call passwords_find for the \
             ones that are"
        )),
        VaultError::NotConfigured | VaultError::NotReady | VaultError::NoCollection => {
            ToolError::Unknown(error.to_string())
        }
        VaultError::Cli(CliError::Failed(_)) | VaultError::Internal(_) => {
            internal(scope, operation, &error)
        }
    }
}

/// The collection agents may read, as the database holds it now.
async fn collection(context: &ToolContext, scope: &CallScope) -> Result<Uuid, ToolError> {
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let vault = password_vault::find(&mut connection)
        .await
        .map_err(|error| internal(scope, "password_vault.find", &error))?
        .ok_or_else(|| ToolError::Unknown(VaultError::NotConfigured.to_string()))?;
    vault
        .collection_id
        .ok_or_else(|| ToolError::Unknown(VaultError::NoCollection.to_string()))
}

fn log_call(scope: &CallScope, tool: &'static str, item_id: Option<Uuid>) {
    event!(
        name: "tools.passwords.call",
        Level::INFO,
        tool.caller = scope.label(),
        tool.name = tool,
        item.id = item_id.map(|id| id.to_string()),
        "an agent read the password vault",
    );
}

async fn run_find(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: FindArguments = parse_arguments(arguments)?;
    let url = arguments
        .url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty());
    let search = arguments
        .search
        .as_deref()
        .map(str::trim)
        .filter(|search| !search.is_empty());
    if url.is_none() && search.is_none() {
        return Err(ToolError::Invalid(
            "pass url, search, or both, so the search is about one site".to_owned(),
        ));
    }
    let collection = collection(context, scope).await?;
    log_call(scope, "passwords_find", None);
    let logins = context
        .passwords
        .find(collection, search, url)
        .await
        .map_err(|error| tool_error(error, scope, "passwords.find"))?;
    Ok(json!({ "logins": logins }))
}

async fn run_login(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let ItemArguments { item_id } = parse_arguments(arguments)?;
    let collection = collection(context, scope).await?;
    log_call(scope, "passwords_login", Some(item_id));
    let login = context
        .passwords
        .login(collection, item_id)
        .await
        .map_err(|error| tool_error(error, scope, "passwords.login"))?;
    if login.password.is_none() {
        return Err(ToolError::Invalid(format!(
            "login {item_id} hides its password from the vault's account, so it cannot be used \
             to sign in"
        )));
    }
    Ok(json!({ "login": login }))
}

async fn run_totp(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let ItemArguments { item_id } = parse_arguments(arguments)?;
    let collection = collection(context, scope).await?;
    log_call(scope, "passwords_totp", Some(item_id));
    let code = context
        .passwords
        .totp(collection, item_id)
        .await
        .map_err(|error| tool_error(error, scope, "passwords.totp"))?;
    Ok(json!({ "code": code.expose_secret() }))
}
