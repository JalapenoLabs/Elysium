// Copyright © 2026 Jalapeno Labs

//! Finding the thread a Studio item's next turn runs in, and continuing the item on a new one
//! when its latest thread has ended.
//!
//! Satellites own no data, so an item outlives any thread it ran on. When the latest thread
//! has expired, been destroyed, or its satellite deleted, the next prompt opens a new session
//! on the same item, on the satellite the person chose or else the previous one, and carries
//! the item's past into it:
//!
//! 1. The newest version of every file the item holds is copied from its storage location
//!    back into the new workspace's `artifacts/`.
//! 2. When the new thread runs the same harness family as the item's latest saved harness
//!    session, that session is imported, and the harness resumes its own conversation.
//! 3. Otherwise the first turn carries a [`Brief`]: the first prompt, the feedback so far, and
//!    the thumbnail as an attachment.
//!
//! The session records which happened (`coding_sessions.continuation`), so the conversation
//! view can say so. An item has at most one live session: continuing takes a short Redis lock
//! per item, so two prompts sent at once cannot both open a thread. See `docs/studio.md`,
//! Continuing.

use std::fmt::Write as _;

use anyhow::Context;
use arsox_sdk::client::ThreadHandle;
use arsox_sdk::proto::turn::v1::TurnAttachment;
use futures_util::stream;
use redis::AsyncCommands as _;
use tracing::{Level, event};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::fleet::views::{ThreadState, ThreadStatus};
use crate::models::coding_session::{self, CodingSession, SessionContinuation};
use crate::models::session_transcript::{self, HarnessFamily};
use crate::models::storage_location;
use crate::models::studio_asset::{self, StudioAsset, StudioAssetKind};
use crate::models::studio_feedback;
use crate::models::studio_item::StudioItem;
use crate::routes::v1::coding_sessions::open::{self, OpenedSession, SessionOpening};
use crate::routes::v1::studio_items::thread::studio_thread_settings;
use crate::state::AppState;
use crate::studio::thumbnail::thumbnail_of;

/// How long one item's continue lock lasts. Long enough to open a thread and copy an item's
/// files back into it; short enough that a request that died holding it frees the item soon.
const CONTINUE_LOCK_SECONDS: u64 = 300;

/// The most feedback prompts a brief repeats, the most recent kept. Enough to say where the
/// asset was heading without the brief outgrowing the prompt it introduces.
const BRIEF_FEEDBACK_LIMIT: usize = 20;

/// The thread an item's next turn runs in.
#[derive(Debug)]
pub struct ItemThread {
    pub session: CodingSession,
    pub handle: ThreadHandle,
    pub thread: ThreadStatus,
    /// Set when this request continued the item on a new session.
    pub continued: Option<Continued>,
}

/// How a session just opened carries the item's past.
#[derive(Debug)]
pub enum Continued {
    /// The harness resumes its own conversation; the turn needs nothing more.
    Imported,
    /// The first turn opens with this brief.
    Brief(Brief),
}

/// What a first turn tells an agent that cannot resume the item's conversation.
#[derive(Debug, Clone)]
pub struct Brief {
    /// Placed ahead of the person's prompt.
    pub text: String,
    /// The item's thumbnail, already in the workspace's `artifacts/`, when it has one.
    pub thumbnail: Option<TurnAttachment>,
}

/// The item's live thread, or a new session that continues it.
///
/// `satellite_id` chooses where a continued item runs; it is ignored while the latest thread
/// is live, which is where the item runs.
///
/// # Errors
/// `409` while another request continues the same item or when no satellite is known to run
/// a continued item on, `502` when the latest thread's satellite cannot be reached, and the
/// failures of opening and seeding a thread, which discard it.
pub async fn thread_for_turn(
    state: &AppState,
    item: &StudioItem,
    actor: Uuid,
    satellite_id: Option<Uuid>,
) -> Result<ItemThread, ApiError> {
    let latest = latest_session(state, item.id).await?;
    if let Some(live) = live_thread(state, latest.as_ref()).await? {
        return Ok(live);
    }

    let previous_satellite = latest.and_then(|session| session.satellite_id);
    let satellite_id = satellite_id
        .or(previous_satellite)
        .ok_or(ApiError::Conflict(
            "this item's satellite is gone; choose a satellite to continue it on",
        ))?;

    let lock = ContinueLock::take(state, item.id).await?;
    let continued = continue_on(state, item, actor, satellite_id).await;
    lock.release(state).await;
    continued
}

/// The item's newest session.
async fn latest_session(
    state: &AppState,
    studio_item_id: Uuid,
) -> Result<Option<CodingSession>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // Oldest first, as the conversation reads; the newest is the last.
    let sessions =
        coding_session::list_for_studio_items(&mut connection, &[studio_item_id]).await?;
    Ok(sessions.into_iter().last())
}

/// `session`'s thread when it is still live.
///
/// A thread the satellite reports gone, expired, or destroyed has ended, as has one whose
/// satellite was deleted. An unreachable satellite answers `502` rather than ending the
/// thread: it may come back, and opening a second thread would fork the item.
async fn live_thread(
    state: &AppState,
    session: Option<&CodingSession>,
) -> Result<Option<ItemThread>, ApiError> {
    let Some(session) = session else {
        return Ok(None);
    };
    let Some(satellite_id) = session.satellite_id else {
        return Ok(None);
    };

    let client = state.fleet.client(satellite_id).await?;
    let handle = match client.threads().attach(session.thread_id.as_str()).await {
        Ok(handle) => handle,
        Err(error) if error.is_gone() || error.is_not_found() => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let thread = match handle.get().await {
        Ok(thread) => ThreadStatus::from(&thread),
        Err(error) if error.is_gone() || error.is_not_found() => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if matches!(thread.state, ThreadState::Expired | ThreadState::Destroyed) {
        return Ok(None);
    }
    Ok(Some(ItemThread {
        session: session.clone(),
        handle,
        thread,
        continued: None,
    }))
}

/// Opens a new session on the item and carries its past into it, discarding the session if
/// that fails part way so no half-seeded thread is left behind.
async fn continue_on(
    state: &AppState,
    item: &StudioItem,
    actor: Uuid,
    satellite_id: Uuid,
) -> Result<ItemThread, ApiError> {
    let settings = studio_thread_settings(state, item.project_id).await?;
    let family = HarnessFamily::of(settings.harness());
    let opened = open::open(
        state,
        SessionOpening {
            created_by: actor,
            satellite_id,
            settings,
            title: item.title.clone(),
            project_id: item.project_id,
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: Some(item.id),
        },
    )
    .await?;

    match carry_past(state, item, &opened, family).await {
        Ok((session, continued)) => {
            let opened = OpenedSession { session, ..opened };
            open::announce(state, &opened);
            Ok(ItemThread {
                session: opened.session,
                handle: opened.handle,
                thread: opened.thread,
                continued: Some(continued),
            })
        }
        Err(error) => {
            open::discard(state, &opened).await;
            Err(error)
        }
    }
}

/// Seeds the new workspace with the item's files, then resumes the item's conversation or
/// prepares a brief, and records which on the session.
async fn carry_past(
    state: &AppState,
    item: &StudioItem,
    opened: &OpenedSession,
    family: Option<HarnessFamily>,
) -> Result<(CodingSession, Continued), ApiError> {
    let files = latest_files(state, item.id).await?;
    copy_files_back(state, item, &opened.handle, &files).await?;

    let continued = if import_conversation(state, item.id, &opened.handle, family).await {
        Continued::Imported
    } else {
        Continued::Brief(brief(state, item, &files).await?)
    };
    let kind = match continued {
        Continued::Imported => SessionContinuation::Imported,
        Continued::Brief(_) => SessionContinuation::Brief,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session =
        coding_session::set_continuation(&mut connection, opened.session.id, kind).await?;
    Ok((session, continued))
}

async fn latest_files(
    state: &AppState,
    studio_item_id: Uuid,
) -> Result<Vec<StudioAsset>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    Ok(studio_asset::latest_per_path(&mut connection, studio_item_id).await?)
}

/// Streams each file from the item's storage location into the workspace's `artifacts/`, where
/// the agent left it. Nothing is held in memory.
///
/// # Errors
/// `502` when the provider or the satellite refuses a file: a workspace missing some of the
/// item's files would quietly lose them from the next turn's work.
async fn copy_files_back(
    state: &AppState,
    item: &StudioItem,
    workspace: &ThreadHandle,
    files: &[StudioAsset],
) -> Result<(), ApiError> {
    if files.is_empty() {
        return Ok(());
    }
    let location = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        storage_location::find(&mut connection, item.storage_location_id).await?
    };
    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;

    for file in files {
        let download = state
            .storage
            .download(&location, &access_key, &file.storage_path)
            .await?;
        let content_length = download
            .content_length
            .unwrap_or_else(|| u64::try_from(file.size_bytes).unwrap_or_default());
        workspace
            .write_file(
                &format!("artifacts/{}", file.artifact_path),
                content_length,
                download.body,
            )
            .await?;
    }
    Ok(())
}

/// Imports the item's latest saved harness session into the new thread, when the thread runs
/// the same harness family. Answers whether it did; a failure is logged, and the turn falls
/// back to a brief rather than failing.
async fn import_conversation(
    state: &AppState,
    studio_item_id: Uuid,
    workspace: &ThreadHandle,
    family: Option<HarnessFamily>,
) -> bool {
    let transcript = match state.database.get().await {
        Ok(mut connection) => {
            session_transcript::latest_for_studio_item(&mut connection, studio_item_id).await
        }
        Err(error) => {
            log_import_failure(studio_item_id, &error.to_string());
            return false;
        }
    };
    let transcript = match transcript {
        Ok(Some(transcript)) => transcript,
        Ok(None) => return false,
        Err(error) => {
            log_import_failure(studio_item_id, &error.to_string());
            return false;
        }
    };
    let same_family = transcript
        .harness()
        .is_ok_and(|saved| Some(saved) == family);
    if !same_family {
        return false;
    }

    let archive = match transcript.open(&state.cipher) {
        Ok(archive) => archive,
        Err(error) => {
            log_import_failure(studio_item_id, &error.to_string());
            return false;
        }
    };
    let content_length = archive.len() as u64;
    let body = stream::once(async move { Ok::<_, std::io::Error>(bytes::Bytes::from(archive)) });
    match workspace.import_session(content_length, body).await {
        Ok(_imported) => true,
        Err(error) => {
            log_import_failure(studio_item_id, &error.to_string());
            false
        }
    }
}

fn log_import_failure(studio_item_id: Uuid, message: &str) {
    event!(
        name: "studio.continue.import_failure",
        Level::WARN,
        studio_item.id = %studio_item_id,
        error.message = %message,
        "could not resume an item's conversation; its next turn carries a brief instead",
    );
}

/// The brief a continued item's first turn opens with: what the item was asked for, the
/// feedback it has had, and its thumbnail, which `copy_files_back` already put in the
/// workspace.
async fn brief(
    state: &AppState,
    item: &StudioItem,
    files: &[StudioAsset],
) -> Result<Brief, ApiError> {
    let feedback = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        studio_feedback::list_for_item(&mut connection, item.id).await?
    };
    let prompts: Vec<&str> = feedback
        .iter()
        .map(|feedback| feedback.prompt.as_str())
        .collect();

    let file_references: Vec<&StudioAsset> = files.iter().collect();
    let thumbnail = thumbnail_of(item.thumbnail_asset_id, &file_references)
        .and_then(|id| files.iter().find(|file| file.id == id))
        .filter(|file| file.kind == StudioAssetKind::Image)
        .map(|file| TurnAttachment {
            path: format!("artifacts/{}", file.artifact_path),
            content_type: file.content_type.clone(),
            size_bytes: u64::try_from(file.size_bytes).unwrap_or_default(),
        });

    Ok(Brief {
        text: brief_text(&item.prompt, &prompts, thumbnail.is_some()),
        thumbnail,
    })
}

/// The brief's words. A pure function of the item's history, so its shape is tested on its
/// own.
fn brief_text(first_prompt: &str, feedback: &[&str], has_thumbnail: bool) -> String {
    let mut text = String::from(
        "This item continues work from an earlier conversation that could not be resumed. \
         The files made so far are in artifacts/.\n\nThe item was first asked for:\n",
    );
    text.push_str(first_prompt);
    text.push('\n');

    let skipped = feedback.len().saturating_sub(BRIEF_FEEDBACK_LIMIT);
    if !feedback.is_empty() {
        text.push_str("\nFeedback given since, oldest first");
        if skipped > 0 {
            let _ = write!(text, " (the {skipped} earliest left out)");
        }
        text.push_str(":\n");
        for prompt in &feedback[skipped..] {
            let _ = writeln!(text, "- {prompt}");
        }
    }
    if has_thumbnail {
        text.push_str("\nThe attached image is the item as it stands.\n");
    }
    text.push_str("\nNow:\n");
    text
}

/// One item's continue lock, held in Redis so every API replica sees it.
#[derive(Debug)]
struct ContinueLock {
    key: String,
    /// Proves this request holds the lock, so releasing never frees another's.
    token: String,
}

impl ContinueLock {
    /// Takes the item's lock.
    ///
    /// # Errors
    /// `409` while another request holds it.
    async fn take(state: &AppState, studio_item_id: Uuid) -> Result<Self, ApiError> {
        let lock = Self {
            key: format!("studio:continue:{studio_item_id}"),
            token: Uuid::now_v7().to_string(),
        };
        let mut redis = state.redis.clone();
        let taken: Option<String> = redis::cmd("SET")
            .arg(&lock.key)
            .arg(&lock.token)
            .arg("NX")
            .arg("EX")
            .arg(CONTINUE_LOCK_SECONDS)
            .query_async(&mut redis)
            .await
            .context("Redis refused the continue lock")?;
        if taken.is_none() {
            return Err(ApiError::Conflict(
                "this item is already being continued; send again in a moment",
            ));
        }
        Ok(lock)
    }

    /// Frees the lock if this request still holds it. A failure is logged; the lock expires
    /// on its own.
    async fn release(self, state: &AppState) {
        let mut redis = state.redis.clone();
        let holder: redis::RedisResult<Option<String>> = redis.get(&self.key).await;
        let released = match holder {
            Ok(Some(holder)) if holder == self.token => redis
                .del::<_, ()>(&self.key)
                .await
                .map_err(|error| error.to_string()),
            Ok(_) => Ok(()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(message) = released {
            event!(
                name: "studio.continue.unlock_failure",
                Level::WARN,
                lock.key = %self.key,
                error.message = %message,
                "could not free an item's continue lock; it expires on its own",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_brief_names_the_first_prompt_and_the_feedback_in_order() {
        let text = brief_text("Model a banana", &["Make it yellower", "Add a stem"], true);
        let first = text.find("Model a banana").expect("the first prompt");
        let yellower = text.find("- Make it yellower").expect("the first feedback");
        let stem = text.find("- Add a stem").expect("the second feedback");
        assert!(first < yellower && yellower < stem, "oldest first: {text}");
        assert!(text.contains("attached image"));
        assert!(text.ends_with("Now:\n"), "the person's prompt follows it");
    }

    #[test]
    fn a_long_history_keeps_the_most_recent_feedback() {
        let feedback: Vec<String> = (1..=25).map(|number| format!("change {number}")).collect();
        let prompts: Vec<&str> = feedback.iter().map(String::as_str).collect();
        let text = brief_text("Model a banana", &prompts, false);
        assert!(text.contains("the 5 earliest left out"));
        assert!(!text.contains("- change 5\n"));
        assert!(text.contains("- change 6\n"));
        assert!(text.contains("- change 25\n"));
        assert!(!text.contains("attached image"));
    }

    #[test]
    fn an_item_without_feedback_leaves_that_section_out() {
        let text = brief_text("Model a banana", &[], false);
        assert!(!text.contains("Feedback given since"));
    }
}
