// Copyright © 2026 Jalapeno Labs

//! Fireflies: a person's meetings, read with their own API key and cut into segments on the
//! timeline. See `docs/meetings.md`.
//!
//! The account is the key, a token account the connector owns: given with the connector,
//! checked with Fireflies before it is stored, and removed with it. The connector's content is
//! the storage location its recordings and clips are kept in.

use diesel_async::AsyncConnection;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    AccountChoice, AccountDefinition, AccountField, AccountFieldType, ContentField,
    ContentFieldType, Definition, Provides, Replies, Source, parse_content, parse_new_account,
};
use crate::auth::CurrentUser;
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::{AccountKind, ConnectorKind, Scope};
use crate::errors::ApiError;
use crate::meetings::WAIT_FOR_APPROVAL_FIELD;
use crate::meetings::fireflies::Fireflies;
use crate::meetings::source::{ListedMeeting, MeetingSource, SourceError};
use crate::models::connector::{self, AccountRef, Connector, NewConnector};
use crate::models::storage_location;
use crate::models::token_account::{self, NewTokenAccount};
use crate::routes::v1::connectors::ApiKey;
use crate::state::AppState;

/// How far back a check looks for a meeting to sample, as far as a new account's first
/// listing reaches.
const SAMPLE_DAYS: i64 = 14;

/// How many meetings the listing check asks for.
const SAMPLE_SIZE: u32 = 5;

/// What a key's account is called when its person names none.
const DEFAULT_NAME: &str = "Fireflies";

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Fireflies,
    scope: Scope::Personal,
    account: AccountKind::Fireflies,
    provides: &[Provides::Meetings],
    start_url: None,
    content: &[
        ContentField {
            id: "storageLocationId",
            field: ContentFieldType::StorageLocation,
            required: true,
        },
        ContentField {
            id: WAIT_FOR_APPROVAL_FIELD,
            field: ContentFieldType::Toggle,
            required: false,
        },
    ],
    checks: &[
        CheckSpec {
            id: "sign_in",
            required: true,
            depends_on: &[],
        },
        CheckSpec {
            id: "list_meetings",
            required: true,
            depends_on: &["sign_in"],
        },
        CheckSpec {
            id: "sample_transcript",
            required: false,
            depends_on: &["list_meetings"],
        },
    ],
    replies: Replies::Never,
};

pub const ACCOUNT: AccountDefinition = AccountDefinition {
    kind: AccountKind::Fireflies,
    fields: &[
        AccountField {
            id: "name",
            field: AccountFieldType::Text,
            required: false,
        },
        AccountField {
            id: "apiKey",
            field: AccountFieldType::Secret,
            required: true,
        },
    ],
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewKey {
    #[serde(default)]
    name: Option<String>,
    api_key: ApiKey,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Content {
    storage_location_id: Uuid,
    /// Off unless its person turns it on; a form saved before the field existed sends none.
    #[serde(default)]
    wait_for_approval: bool,
}

impl Content {
    /// What the connector keeps in `connectors.content`. The storage location is a column of
    /// its own, so a removed location is caught by its foreign key.
    fn stored(&self) -> Value {
        json!({ WAIT_FOR_APPROVAL_FIELD: self.wait_for_approval })
    }
}

/// The key of `current`'s token account `id`.
async fn owned_key(state: &AppState, id: Uuid, owner: Uuid) -> Result<SecretString, ApiError> {
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let account = token_account::find_owned(&mut connection, id, owner).await?;
    if account.kind != AccountKind::Fireflies.as_str() {
        return Err(ApiError::BadRequest(
            "that account's key is not a Fireflies key".to_owned(),
        ));
    }
    Ok(account
        .credential(&state.cipher)
        .map_err(|_open_error| anyhow::anyhow!("the stored key cannot be decrypted"))?)
}

pub async fn prepare_checks(
    state: &AppState,
    source: Source<'_>,
) -> Result<FirefliesChecks, ApiError> {
    let key = match source {
        Source::Stored(stored) => {
            let id = stored.token_account_id.ok_or_else(|| {
                ApiError::BadRequest("a Fireflies connector reads with an API key".to_owned())
            })?;
            owned_key(state, id, stored.created_by).await?
        }
        Source::Draft { account, .. } => {
            let AccountChoice::New { new } = account else {
                return Err(ApiError::BadRequest(
                    "a Fireflies connector is given its own API key".to_owned(),
                ));
            };
            let fields: NewKey = parse_new_account(AccountKind::Fireflies, new)?;
            fields.api_key.0
        }
    };
    Ok(FirefliesChecks {
        fireflies: state.meeting_sources.fireflies().clone(),
        key,
        meetings: Vec::new(),
    })
}

/// Fireflies' checks against one key.
pub struct FirefliesChecks {
    fireflies: Fireflies,
    key: SecretString,
    meetings: Vec<ListedMeeting>,
}

impl std::fmt::Debug for FirefliesChecks {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FirefliesChecks")
            .finish_non_exhaustive()
    }
}

impl Checks for FirefliesChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "sign_in" => match self.fireflies.whoami(&self.key).await {
                Ok(user) => Outcome {
                    message: Some(user.email),
                    ..Outcome::passed()
                },
                Err(error) => Outcome::failed(error.to_string()),
            },
            "list_meetings" => {
                let since = chrono::Utc::now() - chrono::Duration::days(SAMPLE_DAYS);
                match self
                    .fireflies
                    .list(&self.key, Some(since), SAMPLE_SIZE)
                    .await
                {
                    Ok(meetings) => {
                        self.meetings = meetings;
                        Outcome::counted(self.meetings.len())
                    }
                    Err(error) => Outcome::failed(error.to_string()),
                }
            }
            "sample_transcript" => {
                let Some(meeting) = self.meetings.first() else {
                    return Outcome::skipped("no_meetings");
                };
                match self.fireflies.fetch(&self.key, &meeting.external_id).await {
                    Ok(fetched) => Outcome {
                        message: Some(meeting.title.clone()),
                        ..Outcome::counted(fetched.transcript.len())
                    },
                    Err(SourceError::NotFound) => Outcome::skipped("meeting_unavailable"),
                    Err(error) => Outcome::failed(error.to_string()),
                }
            }
            unknown => Outcome::failed(format!("Fireflies has no check {unknown}")),
        }
    }
}

/// Checks `storage_location_id` names a storage location.
async fn ensure_location(state: &AppState, storage_location_id: Uuid) -> Result<(), ApiError> {
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    match storage_location::find(&mut connection, storage_location_id).await {
        Ok(_location) => Ok(()),
        Err(diesel::result::Error::NotFound) => Err(ApiError::BadRequest(
            "storageLocationId names no storage location".to_owned(),
        )),
        Err(other) => Err(other.into()),
    }
}

/// Connects `current`'s meetings with the new key `new`, checked with Fireflies before it is
/// stored, keeping recordings in the content's storage location. The key's account and the
/// connector are stored together.
///
/// # Errors
/// [`ApiError::BadRequest`] for content or a key Fireflies refuses, and
/// [`ApiError::BadGateway`] when Fireflies cannot be reached.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    new: Value,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::Fireflies, content)?;
    ensure_location(state, content.storage_location_id).await?;
    let fields: NewKey = parse_new_account(AccountKind::Fireflies, new)?;
    // A key is stored only once Fireflies accepts it, so no account holds a key nobody tried.
    let user = state
        .meeting_sources
        .fireflies()
        .whoami(&fields.api_key.0)
        .await
        .map_err(|error| match error {
            SourceError::Unauthorized(message) => ApiError::BadRequest(message),
            SourceError::NotFound | SourceError::Refused(_) => {
                ApiError::BadGateway(error.to_string())
            }
        })?;
    let name = fields
        .name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| DEFAULT_NAME.to_owned());
    let new_account = NewTokenAccount::new(
        &state.cipher,
        AccountKind::Fireflies.as_str(),
        &name.chars().take(120).collect::<String>(),
        &fields.api_key.0,
        &user.email,
        current.id(),
    );

    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let created = connection
        .transaction(async |connection| {
            let account = token_account::create(connection, new_account).await?;
            let mut new_connector = NewConnector::new(
                ConnectorKind::Fireflies.as_str(),
                AccountRef::Token(account.id),
                current.id(),
            );
            new_connector.storage_location_id = Some(content.storage_location_id);
            new_connector.content = content.stored();
            connector::create(connection, new_connector).await
        })
        .await?;
    Ok(created)
}

/// Moves where the connector keeps new recordings, and sets whether it waits for its person's
/// approval before acting on meetings. Meetings already kept stay where they are, and a
/// meeting already corrected keeps the decision its correction made.
///
/// # Errors
/// [`ApiError::BadRequest`] for content that names no storage location.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::Fireflies, content)?;
    ensure_location(state, content.storage_location_id).await?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let updated = connector::set_content(
        &mut connection,
        stored.id,
        content.stored(),
        Some(content.storage_location_id),
    )
    .await?;
    state.connectors.activity.changed(stored.id);
    Ok(updated)
}
