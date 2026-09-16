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
//!
//! A thread that declared no relayed servers refuses the socket with `RELAY_NOT_DECLARED`.
//! A thread's settings never change, so that refusal is permanent and ends the relay. It is
//! the cheapest way to learn it: Elysium stores nothing about what a thread declared, and
//! reading the thread's settings first would cost the same one request.

use std::collections::HashMap;
use std::time::Instant;

use arsox_sdk::client::{
    Relay, RelayAnswerer, RelayEvent, Satellite as SatelliteClient, ThreadHandle,
};
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
    /// The thread declared no relayed servers, so its agent has no tool to call here.
    NotDeclared,
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
            RelayOutcome::NotDeclared => {
                event!(
                    name: "coding.relay.not_declared",
                    Level::DEBUG,
                    session.id = %session.id,
                    "thread declared no relayed tools; nothing to relay",
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
    let (handle, mut relay) = match open_relay(&client, &session.thread_id).await {
        Ok(opened) => opened,
        Err(outcome) => return outcome,
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
            Some(finished) = calls.join_next() => match finished {
                Ok(call_id) => {
                    in_flight.remove(&call_id);
                }
                // A cancelled call was already removed when its abort was sent.
                Err(join_error) if join_error.is_cancelled() => {}
                Err(join_error) => {
                    let task_id = join_error.id();
                    in_flight.retain(|_call_id, abort| abort.id() != task_id);
                    event!(
                        name: "coding.relay.call.panicked",
                        Level::ERROR,
                        session.id = %session.id,
                        error.message = %join_error,
                        "a tool call panicked and was never answered; the satellite fails it at \
                         its deadline",
                    );
                }
            },
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

/// Attaches to a thread and opens its relay socket, or says why there is none to open.
async fn open_relay(
    client: &SatelliteClient,
    thread_id: &str,
) -> Result<(ThreadHandle, Relay), RelayOutcome> {
    let interrupted = |reason: String| RelayOutcome::Interrupted {
        reason,
        received: false,
    };

    let handle = match client.threads().attach(thread_id).await {
        Ok(handle) => handle,
        Err(error) if error.is_gone() || error.is_not_found() => {
            return Err(RelayOutcome::ThreadGone);
        }
        Err(error) => return Err(interrupted(error.to_string())),
    };
    match handle.relay().await {
        Ok(relay) => Ok((handle, relay)),
        Err(error) if error.is_relay_not_declared() => Err(RelayOutcome::NotDeclared),
        Err(error) => Err(interrupted(error.to_string())),
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

#[cfg(test)]
mod tests {
    use arsox_sdk::proto::error::v1::{Error as ContractError, ErrorCode};
    use arsox_sdk::proto::satellite::v1::GetVersionResponse;
    use arsox_sdk::proto::thread::v1::{GetThreadResponse, Thread};
    use axum::Router;
    use axum::extract::Path;
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;

    use super::*;

    fn protobuf(status: StatusCode, message: &impl prost::Message) -> Response {
        let headers = [(header::CONTENT_TYPE, "application/protobuf")];
        (status, headers, message.encode_to_vec()).into_response()
    }

    /// A satellite whose one thread declared no relayed servers, answering its relay the
    /// way a satellite does: `409 RELAY_NOT_DECLARED` before the upgrade.
    async fn satellite_without_relayed_servers() -> SatelliteClient {
        let router = Router::new()
            .route(
                "/v1/version",
                get(|| async {
                    let answer = GetVersionResponse {
                        satellite_version: "test".to_owned(),
                        proto_major: 1,
                        proto_minor: 0,
                    };
                    protobuf(StatusCode::OK, &answer)
                }),
            )
            .route(
                "/v1/threads/{id}",
                get(|Path(thread_id): Path<String>| async move {
                    let answer = GetThreadResponse {
                        thread: Some(Thread {
                            thread_id,
                            ..Thread::default()
                        }),
                    };
                    protobuf(StatusCode::OK, &answer)
                }),
            )
            .route(
                "/v1/threads/{id}/relay",
                get(|| async {
                    let refusal = ContractError {
                        code: ErrorCode::RelayNotDeclared.into(),
                        message: "the thread declared no relayed servers".to_owned(),
                        ..ContractError::default()
                    };
                    protobuf(StatusCode::CONFLICT, &refusal)
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, router).await });

        SatelliteClient::connect(format!("http://{address}"), "satellite-secret")
            .await
            .expect("connect")
    }

    /// Regression: a relay for a thread with no relayed servers reconnected forever, since
    /// every refusal read as an interruption to retry.
    #[tokio::test]
    async fn a_thread_without_relayed_servers_ends_its_relay() {
        let client = satellite_without_relayed_servers().await;

        let outcome = open_relay(&client, "thread-1").await;

        assert!(matches!(outcome, Err(RelayOutcome::NotDeclared)));
    }
}
