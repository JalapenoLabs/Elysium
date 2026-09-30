// Copyright © 2026 Jalapeno Labs

//! How a Studio item's threads are opened: Elysium's usual policy, credentials, environment,
//! and Blender, with no repositories, no GitHub token, Studio's instructions, and the export
//! hook. See `docs/studio.md`, Threads.

use anyhow::Context;
use arsox_sdk::proto::settings::v1::ThreadSettings;
use chrono::Utc;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::{environment_variable, llm, storage_location};
use crate::routes::v1::coding_sessions::model_stack;
use crate::routes::v1::coding_sessions::{ThreadPlan, thread_settings};
use crate::state::AppState;
use crate::studio::instructions::INSTRUCTIONS;

/// Settings for a new thread on an item of `project_id`, which may be `None`.
///
/// # Errors
/// Propagates database failures and a variable that cannot be decrypted.
pub async fn studio_thread_settings(
    state: &AppState,
    project_id: Option<Uuid>,
) -> Result<ThreadSettings, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credentials = llm::list(&mut connection).await?;
    let storage_locations = storage_location::list_for_project(&mut connection, project_id).await?;
    let variables =
        environment_variable::thread_environment(&mut connection, &state.cipher).await?;
    drop(connection);

    let stack = model_stack::build(
        &model_stack::open_credentials(credentials, &state.cipher),
        Utc::now(),
    );
    Ok(thread_settings(ThreadPlan {
        stack,
        variables: &variables,
        has_storage_locations: !storage_locations.is_empty(),
        has_project: project_id.is_some(),
        instructions: INSTRUCTIONS.to_owned(),
        ..ThreadPlan::default()
    }))
}
