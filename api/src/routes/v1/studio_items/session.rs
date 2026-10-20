// Copyright © 2026 Jalapeno Labs

//! A Studio item's session: one per item, opened with it, with no repositories and no GitHub
//! token. Every turn carries Studio's instructions (`crate::studio::instructions`), and after
//! every turn its deliverables are collected (`crate::studio::artifacts`). See `docs/studio.md`,
//! Sessions.

use anyhow::Context;
use tracing::{Level, event};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::coding_session::{self, CodingSession, NewCodingSession};
use crate::models::studio_item::StudioItem;
use crate::sessions::TurnExtras;
use crate::state::AppState;
use crate::studio::instructions::INSTRUCTIONS;

/// What every Studio turn adds to its prompt.
pub fn turn_extras() -> TurnExtras {
    TurnExtras {
        system_prompt_append: Some(INSTRUCTIONS.to_owned()),
    }
}

/// Records `item`'s session as `actor` and readies its directory.
///
/// # Errors
/// Propagates database failures, and `502` when the workspace is not available, in which case
/// the row is removed again.
pub async fn open(
    state: &AppState,
    item: &StudioItem,
    actor: Uuid,
) -> Result<CodingSession, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = coding_session::create(
        &mut connection,
        &NewCodingSession {
            created_by: actor,
            id: coding_session::reserve_id(&mut connection).await?,
            project_id: item.project_id,
            title: item.title.clone(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: Some(item.id),
        },
    )
    .await?;
    drop(connection);

    if let Err(error) = state.sessions.prepare(&session, &[]).await {
        let removed = match state.database.get().await {
            Ok(mut connection) => coding_session::delete(&mut connection, session.id)
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(message) = removed {
            event!(
                name: "studio.session.orphaned_row",
                Level::WARN,
                session.id = session.id,
                error.message = %message,
                "could not remove a Studio session whose directory was not readied",
            );
        }
        return Err(error);
    }
    Ok(session)
}

/// The session `item`'s next turn runs in: its newest, or a new one when it has none.
///
/// # Errors
/// As [`open`].
pub async fn for_turn(
    state: &AppState,
    item: &StudioItem,
    actor: Uuid,
) -> Result<CodingSession, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // Oldest first, as the conversation reads; the newest is the last.
    let newest = coding_session::list_for_studio_items(&mut connection, &[item.id])
        .await?
        .into_iter()
        .last();
    drop(connection);
    match newest {
        Some(session) => Ok(session),
        None => open(state, item, actor).await,
    }
}
