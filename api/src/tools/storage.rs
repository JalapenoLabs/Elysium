// Copyright © 2026 Jalapeno Labs

//! `elysium_storage`: the agent's tools for files in its project's storage locations.
//!
//! Every call names a location by id and is checked against the database as it is now: a
//! location unlinked from the project or deleted after the session started is refused,
//! whatever the session was told when it started. Paths inside a location are relative
//! to its directory and checked by [`crate::storage::path::RelativePath`]; workspace paths
//! are relative to the session's directory, and a path that leads outside it is refused
//! inside the workspace container (`crate::runner::files`).
//!
//! File bodies stream between the provider and the workspace without being held in memory.

use diesel::result::Error as DieselError;
use futures_util::FutureExt as _;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{CallScope, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments};
use crate::models::storage_location::{self, StorageLocation, StorageLocationKind};
use crate::runner::WorkspacePath;
use crate::runner::workspace::WorkspaceContainer;
use crate::storage::{MAX_UPLOAD_BYTES, Storage, StorageError};

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_storage",
    instructions: "Files in the storage locations Elysium keeps for this session's project, such \
        as Bunny Storage zones and S3 buckets. Call storage_locations first: every other tool \
        names a location by its id. A location's paths are relative to that location's own \
        directory, with names separated by single slashes and no leading or trailing slash. \
        Workspace paths are relative to the workspace root. Downloads and uploads copy one \
        file between the workspace and a location; edit a stored file by downloading it, \
        changing the copy, and uploading it back. Uploads are limited to 5 GiB, and a very \
        large file on a slow link may take longer than a tool call is allowed to run.",
    tools: &TOOLS,
};

const TOOLS: [Tool; 6] = [
    Tool {
        name: "storage_locations",
        description: "Lists the storage locations this session's project can use: each \
            location's id, name, and provider, and whether listings page with a cursor. Also \
            reports the largest file one upload may carry.",
        input_schema: no_arguments_schema,
        run: |context, scope, arguments| run_locations(context, scope, arguments).boxed(),
    },
    Tool {
        name: "storage_list",
        description: "Lists the files and directories directly inside a directory of a storage \
            location, one page at a time. Pass nextCursor back as cursor for the next page; a \
            null nextCursor is the last page. A directory nothing was written to is empty.",
        input_schema: list_schema,
        run: |context, scope, arguments| run_list(context, scope, arguments).boxed(),
    },
    Tool {
        name: "storage_stat",
        description: "Describes one file or directory in a storage location: its kind, a file's \
            size in bytes, and when it last changed if the provider says. Answers an entry of \
            null when nothing is stored at the path.",
        input_schema: path_schema,
        run: |context, scope, arguments| run_stat(context, scope, arguments).boxed(),
    },
    Tool {
        name: "storage_download",
        description: "Copies a file from a storage location into the workspace, replacing any \
            file at workspacePath and creating missing directories on the way. The file streams \
            straight into the workspace.",
        input_schema: download_schema,
        run: |context, scope, arguments| run_download(context, scope, arguments).boxed(),
    },
    Tool {
        name: "storage_upload",
        description: "Copies a workspace file into a storage location, replacing any file at \
            path. Directories along the path need not exist. Files over maxUploadBytes (5 GiB) \
            are refused.",
        input_schema: upload_schema,
        run: |context, scope, arguments| run_upload(context, scope, arguments).boxed(),
    },
    Tool {
        name: "storage_delete",
        description: "Deletes one file from a storage location. Directories are refused. \
            Deleting a file that is not there succeeds.",
        input_schema: path_schema,
        run: |context, scope, arguments| run_delete(context, scope, arguments).boxed(),
    },
];

/// The location id every call but `storage_locations` names, as a schema property.
fn location_id_property() -> Value {
    json!({
        "type": "string",
        "format": "uuid",
        "description": "The location's id, from storage_locations.",
    })
}

/// A path inside a location, as a schema property.
fn path_property(description: &str) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": 1024,
        "description": description,
    })
}

fn no_arguments_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "locationId": location_id_property(),
            "directory": {
                "type": "string",
                "maxLength": 1024,
                "description": "The directory to list, relative to the location's directory. \
                    Omit or pass an empty string for the location's own directory.",
            },
            "cursor": {
                "type": "string",
                "description": "nextCursor from the previous page. Only locations whose \
                    supportsCursor is true take one.",
            },
        },
        "required": ["locationId"],
        "additionalProperties": false,
    })
}

fn path_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "locationId": location_id_property(),
            "path": path_property("The file's path, relative to the location's directory."),
        },
        "required": ["locationId", "path"],
        "additionalProperties": false,
    })
}

/// A workspace file, as a schema property.
fn workspace_path_property(description: &str) -> Value {
    json!({ "type": "string", "minLength": 1, "description": description })
}

fn download_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "locationId": location_id_property(),
            "path": path_property("The stored file's path, relative to the location's directory."),
            "workspacePath": workspace_path_property(
                "Where to write the file, relative to the workspace root.",
            ),
        },
        "required": ["locationId", "path", "workspacePath"],
        "additionalProperties": false,
    })
}

fn upload_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspacePath": workspace_path_property(
                "The file to upload, relative to the workspace root.",
            ),
            "locationId": location_id_property(),
            "path": path_property("Where to store the file, relative to the location's directory."),
            "contentType": {
                "type": "string",
                "minLength": 1,
                "maxLength": 255,
                "description": "The file's media type, such as image/png. Omit to use the type \
                    the workspace reports, or none.",
            },
        },
        "required": ["workspacePath", "locationId", "path"],
        "additionalProperties": false,
    })
}

#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde reads {} only into a braced struct; a unit struct takes null"
)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NoArguments {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListArguments {
    location_id: Uuid,
    #[serde(default)]
    directory: String,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PathArguments {
    location_id: Uuid,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DownloadArguments {
    location_id: Uuid,
    path: String,
    workspace_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UploadArguments {
    workspace_path: String,
    location_id: Uuid,
    path: String,
    content_type: Option<String>,
}

impl From<StorageError> for ToolError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::NotFound => Self::NotFound(
                "no file is stored at that path; call storage_list to see what is".to_owned(),
            ),
            StorageError::Invalid(message) => Self::Invalid(message),
            StorageError::Unauthorized(message) => Self::Provider(format!(
                "the provider refused this location's access key, which only Elysium's operator \
                 can change: {message}"
            )),
            StorageError::Refused(message) => Self::Provider(message),
        }
    }
}

async fn run_locations(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let NoArguments {} = parse_arguments(arguments)?;
    let CallScope::Session(session) = scope;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let locations = storage_location::list_for_project(&mut connection, session.project_id)
        .await
        .map_err(|error| internal(scope, "storage_location.list_for_project", &error))?;

    let locations: Vec<Value> = locations
        .iter()
        .map(|location| {
            json!({
                "id": location.id,
                "name": location.name,
                "provider": location.kind,
                // Bunny lists a whole directory in one page.
                "supportsCursor": location.kind != StorageLocationKind::Bunny,
            })
        })
        .collect();
    Ok(json!({ "locations": locations, "maxUploadBytes": MAX_UPLOAD_BYTES }))
}

async fn run_list(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: ListArguments = parse_arguments(arguments)?;
    let (location, access_key) = open_location(context, scope, arguments.location_id).await?;
    let page = context
        .storage
        .list(
            &location,
            &access_key,
            &arguments.directory,
            arguments.cursor.as_deref(),
        )
        .await?;
    Ok(json!(page))
}

async fn run_stat(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: PathArguments = parse_arguments(arguments)?;
    let (location, access_key) = open_location(context, scope, arguments.location_id).await?;
    let entry = context
        .storage
        .stat(&location, &access_key, &arguments.path)
        .await?;
    Ok(json!({ "entry": entry }))
}

async fn run_delete(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: PathArguments = parse_arguments(arguments)?;
    let (location, access_key) = open_location(context, scope, arguments.location_id).await?;
    context
        .storage
        .delete(&location, &access_key, &arguments.path)
        .await?;
    Ok(json!({ "deleted": arguments.path }))
}

async fn run_download(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: DownloadArguments = parse_arguments(arguments)?;
    let (location, access_key) = open_location(context, scope, arguments.location_id).await?;
    let CallScope::Session(session) = scope;
    copy_to_workspace(
        &context.storage,
        &context.workspace,
        &session.directory,
        &location,
        &access_key,
        &arguments,
    )
    .await
}

async fn run_upload(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: UploadArguments = parse_arguments(arguments)?;
    // The location is checked first, so a call it refuses never opens the workspace file.
    let (location, access_key) = open_location(context, scope, arguments.location_id).await?;
    let CallScope::Session(session) = scope;
    copy_to_location(
        &context.storage,
        &context.workspace,
        &session.directory,
        &location,
        &access_key,
        arguments,
    )
    .await
}

/// Finds a location the session's project may use, as the database says now, with its
/// decrypted access key.
///
/// # Errors
/// Returns [`ToolError::LocationUnavailable`] for a location that does not exist or is not
/// the project's, and [`ToolError::Internal`] when the database or the key fails.
async fn open_location(
    context: &ToolContext,
    scope: &CallScope,
    location_id: Uuid,
) -> Result<(StorageLocation, SecretString), ToolError> {
    let CallScope::Session(session) = scope;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let location =
        storage_location::find_for_project(&mut connection, location_id, session.project_id)
            .await
            .map_err(|error| match error {
                DieselError::NotFound => ToolError::LocationUnavailable(location_id),
                other => internal(scope, "storage_location.find_for_project", &other),
            })?;
    drop(connection);

    let access_key = location
        .access_key(&context.cipher)
        .map_err(|error| internal(scope, "storage_location.access_key", &error))?;
    Ok((location, access_key))
}

/// Streams a stored file into the session's directory.
///
/// # Errors
/// Returns [`ToolError::NotFound`] when no file is stored at the path, [`ToolError::Provider`]
/// when the provider refuses, and [`ToolError::Workspace`] when the path leaves the session's
/// directory or the write fails part way.
async fn copy_to_workspace(
    storage: &Storage,
    workspace: &WorkspaceContainer,
    directory: &WorkspacePath,
    location: &StorageLocation,
    access_key: &SecretString,
    arguments: &DownloadArguments,
) -> Result<Value, ToolError> {
    let download = storage
        .download(location, access_key, &arguments.path)
        .await?;
    let size_bytes = workspace
        .write_file(directory, &arguments.workspace_path, download.body)
        .await
        .map_err(|error| ToolError::Workspace(error.to_string()))?;
    Ok(json!({ "workspacePath": arguments.workspace_path, "sizeBytes": size_bytes }))
}

/// Streams a file from the session's directory into a location. A file over
/// [`MAX_UPLOAD_BYTES`] is refused before a byte moves.
///
/// # Errors
/// Returns [`ToolError::Workspace`] when the file cannot be read or leaves the session's
/// directory, [`ToolError::Invalid`] for a file too large, and [`ToolError::Provider`] when the
/// provider refuses or the body fails part way.
async fn copy_to_location(
    storage: &Storage,
    workspace: &WorkspaceContainer,
    directory: &WorkspacePath,
    location: &StorageLocation,
    access_key: &SecretString,
    arguments: UploadArguments,
) -> Result<Value, ToolError> {
    let file = workspace
        .read_file(directory, &arguments.workspace_path)
        .await
        .map_err(|error| ToolError::Workspace(error.to_string()))?;
    let content_length = file.content_length;
    if content_length > MAX_UPLOAD_BYTES {
        return Err(ToolError::Invalid(format!(
            "{} is {content_length} bytes, over the {MAX_UPLOAD_BYTES} byte (5 GiB) upload limit",
            arguments.workspace_path
        )));
    }
    let entry = storage
        .upload(
            location,
            access_key,
            &arguments.path,
            file.body,
            content_length,
            arguments.content_type.as_deref(),
        )
        .await?;
    Ok(json!({ "entry": entry }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_follow_the_schema_names_and_refuse_anything_else() {
        let listed: ListArguments = parse_arguments(&format!(
            r#"{{"locationId":"{}","cursor":"next"}}"#,
            Uuid::nil()
        ))
        .expect("parses");
        assert_eq!(
            listed.directory, "",
            "the directory defaults to the location's own"
        );
        assert_eq!(listed.cursor.as_deref(), Some("next"));

        let missing =
            parse_arguments::<PathArguments>(&format!(r#"{{"locationId":"{}"}}"#, Uuid::nil()))
                .expect_err("a path is required");
        assert!(matches!(&missing, ToolError::Invalid(message) if message.contains("path")));

        let snake = parse_arguments::<PathArguments>(&format!(
            r#"{{"location_id":"{}","path":"a.txt"}}"#,
            Uuid::nil()
        ))
        .expect_err("names are camelCase, as the schema says");
        assert!(matches!(snake, ToolError::Invalid(_)));

        let not_an_id = parse_arguments::<PathArguments>(r#"{"locationId":"media","path":"a"}"#)
            .expect_err("a location id is a UUID");
        assert!(matches!(not_an_id, ToolError::Invalid(_)));
    }

    #[test]
    fn every_schema_property_is_an_argument_the_tool_reads() {
        let id = Uuid::nil();
        let everything = [
            ("storage_locations", json!({})),
            (
                "storage_list",
                json!({ "locationId": id, "directory": "builds", "cursor": "next" }),
            ),
            ("storage_stat", json!({ "locationId": id, "path": "a.txt" })),
            (
                "storage_download",
                json!({ "locationId": id, "path": "a.txt", "workspacePath": "in/a.txt" }),
            ),
            (
                "storage_upload",
                json!({
                    "workspacePath": "out/a.txt",
                    "locationId": id,
                    "path": "a.txt",
                    "contentType": "text/plain",
                }),
            ),
            (
                "storage_delete",
                json!({ "locationId": id, "path": "a.txt" }),
            ),
        ];
        assert_eq!(everything.len(), TOOLS.len(), "every tool has an example");
        for (name, arguments) in everything {
            let tool = TOOLS
                .iter()
                .find(|tool| tool.name == name)
                .expect("a tool by that name");
            let schema = (tool.input_schema)();
            let properties: Vec<&String> = schema["properties"]
                .as_object()
                .expect("properties")
                .keys()
                .collect();
            let given: Vec<&String> = arguments.as_object().expect("object").keys().collect();
            assert_eq!(
                properties.len(),
                given.len(),
                "{name}: the example sets every property"
            );

            let text = arguments.to_string();
            let parsed = match name {
                "storage_locations" => parse_arguments::<NoArguments>(&text).map(|_parsed| ()),
                "storage_list" => parse_arguments::<ListArguments>(&text).map(|_parsed| ()),
                "storage_download" => parse_arguments::<DownloadArguments>(&text).map(|_parsed| ()),
                "storage_upload" => parse_arguments::<UploadArguments>(&text).map(|_parsed| ()),
                _ => parse_arguments::<PathArguments>(&text).map(|_parsed| ()),
            };
            parsed.unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    #[test]
    fn storage_errors_tell_the_agent_what_to_do_without_internal_detail() {
        let missing = ToolError::from(StorageError::NotFound);
        assert!(missing.to_string().contains("storage_list"));

        let refused = ToolError::from(StorageError::Unauthorized("check the zone's password"));
        assert!(matches!(&refused, ToolError::Provider(_)));
        assert!(refused.to_string().contains("operator"));

        let invalid = ToolError::from(StorageError::Invalid("not plain".to_owned()));
        assert_eq!(invalid, ToolError::Invalid("not plain".to_owned()));
    }
}
