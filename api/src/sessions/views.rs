// Copyright © 2026 Jalapeno Labs

//! What clients see of a session: its recorded events, its turns' summaries, and what it is
//! doing now.
//!
//! A session's events are kept as the runner reported them (`crate::runner::RunnerEvent`) and
//! turned into these views when read or published, so a later build renders old events with
//! whatever it knows then. An event Elysium does not render keeps its `type` and carries no
//! `payload`, so clients can still list it.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::models::turn_summary::TurnSummary;
use crate::runner::{RunnerEvent, TurnStatus};

/// One recorded event of a session, as clients render it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub session_id: i64,
    /// Strictly increasing per session; history and live events merge on it.
    pub sequence: u64,
    pub turn_id: Option<Uuid>,
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub payload: Option<EventPayload>,
}

impl SessionEvent {
    /// The view of `event`, the `sequence`-th event of `session_id`.
    pub fn new(
        session_id: i64,
        sequence: u64,
        turn_id: Option<Uuid>,
        occurred_at: DateTime<Utc>,
        event: &RunnerEvent,
    ) -> Self {
        let (event_type, payload) = render(event);
        Self {
            session_id,
            sequence,
            turn_id,
            occurred_at: Some(occurred_at),
            event_type,
            payload,
        }
    }
}

/// A turn's state, as a finished turn reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurnState {
    Queued,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    UsageLimited,
}

impl From<TurnStatus> for TurnState {
    fn from(status: TurnStatus) -> Self {
        match status {
            TurnStatus::Completed => Self::Completed,
            TurnStatus::Failed => Self::Failed,
            TurnStatus::Cancelled => Self::Cancelled,
            TurnStatus::Interrupted => Self::Interrupted,
            TurnStatus::UsageLimited => Self::UsageLimited,
        }
    }
}

/// A turn as it was accepted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnView {
    pub turn_id: Uuid,
    pub status: TurnState,
    pub prompt: String,
    pub queued_at: DateTime<Utc>,
}

/// What a session is doing now. Sessions never end on their own: one is idle until it is
/// given a prompt, and stays until it is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivityState {
    Idle,
    /// Turns are waiting behind another session's, for a free slot.
    Queued,
    Running,
}

/// A finished turn's summary (`super::summary`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSummaryView {
    pub session_id: i64,
    pub turn_id: Uuid,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

impl From<TurnSummary> for TurnSummaryView {
    fn from(row: TurnSummary) -> Self {
        Self {
            session_id: row.session_id,
            turn_id: row.turn_id,
            summary: row.summary,
            created_at: row.created_at,
        }
    }
}

/// A session's current activity, kept in memory and published as it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivity {
    pub state: ActivityState,
    /// Turns submitted and not yet started.
    pub queue_depth: u32,
    pub current_turn_id: Option<Uuid>,
    pub last_activity_at: Option<DateTime<Utc>>,
}

impl Default for SessionActivity {
    fn default() -> Self {
        Self {
            state: ActivityState::Idle,
            queue_depth: 0,
            current_turn_id: None,
            last_activity_at: None,
        }
    }
}

/// The renderable part of an event, tagged by `kind`.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EventPayload {
    AgentMessage {
        text: String,
    },
    AgentThinking {
        text: String,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        input: Option<Value>,
    },
    ToolCompleted {
        tool_call_id: String,
        tool_name: String,
        ok: bool,
        output_preview: Option<String>,
        elapsed_milliseconds: Option<i64>,
    },
    TurnStarted {
        prompt: String,
    },
    TurnCompleted {
        status: TurnState,
        summary: String,
        error: Option<String>,
    },
    Incident {
        code: &'static str,
        message: String,
        retryable: bool,
    },
}

/// The event's wire `type` and, when Elysium renders it, its payload.
fn render(event: &RunnerEvent) -> (&'static str, Option<EventPayload>) {
    match event {
        RunnerEvent::TurnStarted { prompt, .. } => (
            "turn.started",
            Some(EventPayload::TurnStarted {
                prompt: prompt.clone(),
            }),
        ),
        RunnerEvent::AgentMessage { text } => (
            "agent.message",
            Some(EventPayload::AgentMessage { text: text.clone() }),
        ),
        RunnerEvent::AgentThinking { text } => (
            "agent.thinking",
            Some(EventPayload::AgentThinking { text: text.clone() }),
        ),
        RunnerEvent::ToolStarted {
            tool_call_id,
            tool_name,
            input,
        } => (
            "tool.started",
            Some(EventPayload::ToolStarted {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                input: Some(input.clone()),
            }),
        ),
        RunnerEvent::ToolCompleted {
            tool_call_id,
            tool_name,
            ok,
            output_preview,
        } => (
            "tool.completed",
            Some(EventPayload::ToolCompleted {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                ok: *ok,
                output_preview: Some(output_preview.clone()),
                elapsed_milliseconds: None,
            }),
        ),
        RunnerEvent::CredentialRolledOver { reason, .. } => (
            "credential.rolled_over",
            Some(EventPayload::Incident {
                code: "CREDENTIAL_ROLLED_OVER",
                message: reason.clone(),
                retryable: true,
            }),
        ),
        RunnerEvent::Finished(outcome) => (
            "turn.completed",
            Some(EventPayload::TurnCompleted {
                status: TurnState::from(outcome.status),
                summary: outcome.result.clone().unwrap_or_default(),
                error: outcome.error.clone(),
            }),
        ),
        RunnerEvent::RateLimit { .. } => ("rate_limit", None),
        RunnerEvent::Other { .. } => ("other", None),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::runner::TurnOutcome;

    #[test]
    fn a_finished_turn_renders_as_turn_completed() {
        let event = RunnerEvent::Finished(TurnOutcome {
            status: TurnStatus::UsageLimited,
            result: None,
            error: Some("every credential is spent".to_owned()),
            harness_session_id: None,
            cost_usd_micros: None,
            input_tokens: None,
            output_tokens: None,
        });
        let view = SessionEvent::new(12, 7, None, Utc::now(), &event);
        let json = serde_json::to_value(&view).expect("serializes");
        assert_eq!(json["type"], "turn.completed");
        assert_eq!(
            json["payload"],
            json!({ "kind": "turnCompleted", "status": "usage-limited", "summary": "", "error": "every credential is spent" })
        );
    }

    #[test]
    fn bookkeeping_keeps_its_type_without_a_payload() {
        let view = SessionEvent::new(
            12,
            8,
            None,
            Utc::now(),
            &RunnerEvent::Other {
                raw: json!({ "type": "new" }),
            },
        );
        assert_eq!(view.event_type, "other");
        assert!(view.payload.is_none());
    }
}
