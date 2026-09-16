// Copyright © 2026 Jalapeno Labs

//! A session's tool relay: answering the tool calls its agent makes to Elysium.
//!
//! A satellite cannot reach Elysium, so the tools Elysium serves ([`crate::tools`]) are
//! declared on the thread as relayed MCP servers, and Elysium opens the relay socket to the
//! satellite itself. Nothing listens for the satellite. The satellite forwards each call
//! down the socket and Elysium answers up it.
//!
//! One relay runs per session on an active satellite, beside the session watcher and under
//! the same cancellation token, so the two start, restart, and stop together. Each call
//! runs in its own task, so a slow download holds up nothing; a cancellation from the
//! satellite aborts that call's task. Calls are never retried, since the agent decides
//! whether to call again.
//!
//! The satellite keeps nothing for a client that is not attached: while this relay is
//! reconnecting, calls fail at once to the agent. Calls in flight when the socket drops are
//! abandoned, because the satellite has already failed them.

use std::collections::HashMap;
use std::time::Instant;

use arsox_sdk::client::{RelayAnswerer, RelayEvent};
use arsox_sdk::proto::relay::v1::{ToolCall, ToolContent, ToolResult, tool_content};
use tokio::task::{AbortHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use super::{Fleet, RECONNECT_BACKOFF_INITIAL, RECONNECT_BACKOFF_MAX};
use crate::models::coding_session::CodingSession;
use crate::tools::{self, CallScope, ToolContext};

/// How one attachment to a thread's relay ended.
enum RelayOutcome {
    Cancelled,
    /// The satellite no longer has the thread, so there is nothing to relay for.
    ThreadGone,
    Interrupted {
        reason: String,
        /// Whether any call arrived on this attachment, which proves the relay worked
        /// and resets the backoff.
        received: bool,
    },
}

/// Keeps a session's relay attached until `cancel` fires or the thread is gone.
pub(super) async fn run_relay(fleet: Fleet, session: CodingSession, cancel: CancellationToken) {
    let mut backoff = RECONNECT_BACKOFF_INITIAL;
    loop {
        match serve(&fleet, &session, &cancel).await {
            RelayOutcome::Cancelled => return,
            RelayOutcome::ThreadGone => {
                event!(
                    name: "coding.relay.thread_gone",
                    Level::DEBUG,
                    session.id = %session.id,
                    "thread no longer exists; stopped relaying its tool calls",
                );
                return;
            }
            RelayOutcome::Interrupted { reason, received } => {
                event!(
                    name: "coding.relay.interrupted",
                    Level::DEBUG,
                    session.id = %session.id,
                    error.message = %reason,
                    "tool relay interrupted; reconnecting",
                );
                if received {
                    backoff = RECONNECT_BACKOFF_INITIAL;
                }
            }
        }

        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(RECONNECT_BACKOFF_MAX);
    }
}

/// Attaches to the thread's relay and answers calls until the socket ends.
async fn serve(fleet: &Fleet, session: &CodingSession, cancel: &CancellationToken) -> RelayOutcome {
    let interrupted = |reason: String| RelayOutcome::Interrupted {
        reason,
        received: false,
    };

    let client = match fleet.client(session.satellite_id).await {
        Ok(client) => client,
        Err(error) => return interrupted(error.to_string()),
    };
    let handle = match client.threads().attach(session.thread_id.as_str()).await {
        Ok(handle) => handle,
        Err(error) if error.is_gone() || error.is_not_found() => return RelayOutcome::ThreadGone,
        Err(error) => return interrupted(error.to_string()),
    };
    let mut relay = match handle.relay().await {
        Ok(relay) => relay,
        Err(error) => return interrupted(error.to_string()),
    };
    let answerer = relay.answerer();
    let scope = CallScope {
        session_id: session.id,
        project_id: session.project_id,
        workspace: handle,
    };

    // Dropping the set, on any return, aborts every call still running: the satellite
    // fails them to the agent once this socket is gone.
    let mut calls = JoinSet::new();
    let mut in_flight: HashMap<String, AbortHandle> = HashMap::new();
    let mut received = false;
    loop {
        tokio::select! {
            () = cancel.cancelled() => return RelayOutcome::Cancelled,
            Some(finished) = calls.join_next() => {
                if let Ok(call_id) = finished {
                    in_flight.remove(&call_id);
                }
            }
            next = relay.next() => match next {
                Some(Ok(RelayEvent::Call(call))) => {
                    received = true;
                    let call_id = call.call_id.clone();
                    let abort = calls.spawn(answer(
                        fleet.inner.tools.clone(),
                        scope.clone(),
                        answerer.clone(),
                        call,
                    ));
                    in_flight.insert(call_id, abort);
                }
                Some(Ok(RelayEvent::Cancelled(call_id))) => {
                    if let Some(abort) = in_flight.remove(&call_id) {
                        abort.abort();
                        event!(
                            name: "coding.relay.call.cancelled",
                            Level::INFO,
                            session.id = %session.id,
                            tool.call.id = %call_id,
                            "the satellite stopped waiting for a tool call; abandoned it",
                        );
                    }
                }
                Some(Err(error)) => {
                    return RelayOutcome::Interrupted { reason: error.to_string(), received };
                }
                None => {
                    return RelayOutcome::Interrupted {
                        reason: "the relay closed".to_owned(),
                        received,
                    };
                }
            },
        }
    }
}

/// Runs one call and sends its result, returning the call's id once answered.
async fn answer(
    tools: ToolContext,
    scope: CallScope,
    answerer: RelayAnswerer,
    call: ToolCall,
) -> String {
    let started = Instant::now();
    let output = tools::dispatch(
        &tools,
        &scope,
        &call.server,
        &call.tool,
        &call.arguments_json,
    )
    .await;

    event!(
        name: "coding.relay.call.completed",
        Level::INFO,
        session.id = %scope.session_id,
        tool.call.id = %call.call_id,
        tool.server = %call.server,
        tool.name = %call.tool,
        tool.is_error = output.is_error,
        duration.ms = started.elapsed().as_millis(),
        "answered a tool call",
    );

    let result = ToolResult {
        call_id: call.call_id.clone(),
        content: vec![ToolContent {
            kind: Some(tool_content::Kind::Text(output.text)),
        }],
        is_error: output.is_error,
    };
    if let Err(error) = answerer.answer(result).await {
        event!(
            name: "coding.relay.call.unanswered",
            Level::WARN,
            session.id = %scope.session_id,
            tool.call.id = %call.call_id,
            error.message = %error,
            "could not send a tool call's result; the satellite fails the call",
        );
    }
    call.call_id
}
