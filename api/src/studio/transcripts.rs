// Copyright © 2026 Jalapeno Labs

//! Backing up a Studio thread's harness session after every turn, so a later thread on the
//! item can resume the same conversation (`crate::studio::continuation`).
//!
//! The satellite exports the harness's session (Claude's transcript, Codex's rollout) as one
//! archive, which is kept sealed in `session_transcripts`, the latest per session. The archive
//! is read whole, since it is compressed and sealed as one value, so a read is bounded by
//! [`MAX_EXPORT_BYTES`]. See `docs/studio.md`, Continuing.

use arsox_sdk::client::ThreadHandle;
use futures_util::StreamExt as _;
use tracing::{Level, event};

use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::session_transcript::{self, HarnessFamily};

/// The largest export read. Sealed transcripts are capped at 64 MiB after compression, and a
/// harness session compresses several times over; past this the export is refused before it
/// is held in memory at all.
const MAX_EXPORT_BYTES: u64 = 256 * 1024 * 1024;

/// Exports the thread's harness session and keeps it as the session's latest. Every failure
/// is logged: a missed backup only means a later continue may need a brief.
pub async fn back_up(database: &Pool, cipher: &Cipher, session_id: i64, workspace: &ThreadHandle) {
    match export(workspace).await {
        Ok(Some((family, harness_session_id, archive))) => {
            let stored = match database.get().await {
                Ok(mut connection) => session_transcript::store(
                    &mut connection,
                    cipher,
                    session_id,
                    family,
                    &harness_session_id,
                    &archive,
                )
                .await
                .map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            if let Err(message) = stored {
                log_failure(session_id, &message);
            }
        }
        Ok(None) => {}
        Err(message) => log_failure(session_id, &message),
    }
}

/// The thread's harness session, or `None` when it has none to export yet or runs a harness
/// this build does not know.
async fn export(
    workspace: &ThreadHandle,
) -> Result<Option<(HarnessFamily, String, Vec<u8>)>, String> {
    let export = match workspace.export_session().await {
        Ok(export) => export,
        Err(error) if error.is_not_found() => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let Some(family) = HarnessFamily::of(export.harness) else {
        return Ok(None);
    };
    if export.content_length() > MAX_EXPORT_BYTES {
        return Err(format!(
            "the export is {} bytes, over the {MAX_EXPORT_BYTES} byte limit",
            export.content_length()
        ));
    }

    let harness_session_id = export.harness_session_id.clone();
    let mut archive = Vec::with_capacity(usize::try_from(export.content_length()).unwrap_or(0));
    let mut body = export.into_body();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        if archive.len() as u64 + chunk.len() as u64 > MAX_EXPORT_BYTES {
            return Err(format!(
                "the export passed the {MAX_EXPORT_BYTES} byte limit"
            ));
        }
        archive.extend_from_slice(&chunk);
    }
    Ok(Some((family, harness_session_id, archive)))
}

fn log_failure(session_id: i64, message: &str) {
    event!(
        name: "studio.transcript.backup_failure",
        Level::WARN,
        session.id = session_id,
        error.message = %message,
        "could not back up a Studio thread's harness session",
    );
}
