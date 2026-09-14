// Copyright © 2026 Jalapeno Labs

//! JSON views of Arsox contract types.
//!
//! Satellites speak protobuf, and the generated types carry no `Serialize`. These are
//! Elysium's own shapes for what the frontend shows, converted at the boundary so no
//! route or client ever depends on the wire contract directly. Enums become
//! kebab-case strings; timestamps become UTC `DateTime`s.
//!
//! A thread event whose payload Elysium does not render keeps its wire `type` and
//! carries no `payload`, so a newer satellite's events still reach the client.

use arsox_sdk::proto::common::v1::{Duration, Timestamp};
use arsox_sdk::proto::error::v1::ErrorCode;
use arsox_sdk::proto::event::v1::{Author, AuthorKind, ThreadEvent, thread_event};
use arsox_sdk::proto::thread::v1::ThreadSummary;
use arsox_sdk::proto::turn::v1::TurnStatus;
use arsox_sdk::proto::{interaction, thread};
use chrono::{DateTime, Utc};
use prost_types::value::Kind;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// What the fleet last learned about a satellite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SatelliteStatus {
    pub satellite_id: Uuid,
    pub reachable: bool,
    pub version: Option<String>,
    pub running_threads: Option<u32>,
    pub max_concurrent_threads: Option<u32>,
    /// Why the satellite is unreachable. Never contains the secret.
    pub error: Option<String>,
}

/// A thread's lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreadState {
    /// The satellite reported a state this build does not know, or has not reported yet.
    Unknown,
    Provisioning,
    Idle,
    Running,
    AwaitingInput,
    Watching,
    Paused,
    Expired,
    Destroyed,
}

impl From<i32> for ThreadState {
    fn from(value: i32) -> Self {
        match thread::v1::ThreadState::try_from(value) {
            Ok(thread::v1::ThreadState::Provisioning) => Self::Provisioning,
            Ok(thread::v1::ThreadState::Idle) => Self::Idle,
            Ok(thread::v1::ThreadState::Running) => Self::Running,
            Ok(thread::v1::ThreadState::AwaitingInput) => Self::AwaitingInput,
            Ok(thread::v1::ThreadState::Watching) => Self::Watching,
            Ok(thread::v1::ThreadState::Paused) => Self::Paused,
            Ok(thread::v1::ThreadState::Expired) => Self::Expired,
            Ok(thread::v1::ThreadState::Destroyed) => Self::Destroyed,
            Ok(thread::v1::ThreadState::Unspecified) | Err(_) => Self::Unknown,
        }
    }
}

/// A thread as the satellite last listed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStatus {
    pub state: ThreadState,
    pub queue_depth: u32,
    pub current_turn_id: Option<String>,
    pub latest_sequence: u64,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl From<&ThreadSummary> for ThreadStatus {
    fn from(summary: &ThreadSummary) -> Self {
        Self {
            state: ThreadState::from(summary.state),
            queue_depth: summary.queue_depth,
            current_turn_id: summary.current_turn_id.clone(),
            latest_sequence: summary.latest_sequence,
            last_activity_at: summary.last_activity_at.as_ref().and_then(to_utc),
            expires_at: summary.expires_at.as_ref().and_then(to_utc),
        }
    }
}

impl From<&thread::v1::Thread> for ThreadStatus {
    fn from(thread: &thread::v1::Thread) -> Self {
        Self {
            state: ThreadState::from(thread.state),
            queue_depth: thread.queue_depth,
            current_turn_id: thread.current_turn_id.clone(),
            latest_sequence: thread.latest_sequence,
            last_activity_at: thread.last_activity_at.as_ref().and_then(to_utc),
            expires_at: thread.expires_at.as_ref().and_then(to_utc),
        }
    }
}

/// A turn's lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurnState {
    Unknown,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Watching,
}

impl From<i32> for TurnState {
    fn from(value: i32) -> Self {
        match TurnStatus::try_from(value) {
            Ok(TurnStatus::Queued) => Self::Queued,
            Ok(TurnStatus::Running) => Self::Running,
            Ok(TurnStatus::Completed) => Self::Completed,
            Ok(TurnStatus::Failed) => Self::Failed,
            Ok(TurnStatus::Cancelled) => Self::Cancelled,
            Ok(TurnStatus::Interrupted) => Self::Interrupted,
            Ok(TurnStatus::Watching) => Self::Watching,
            Ok(TurnStatus::Unspecified) | Err(_) => Self::Unknown,
        }
    }
}

/// A turn as the satellite accepted it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnView {
    pub turn_id: String,
    pub status: TurnState,
    pub prompt: String,
    pub queued_at: Option<DateTime<Utc>>,
}

impl From<&arsox_sdk::proto::turn::v1::Turn> for TurnView {
    fn from(turn: &arsox_sdk::proto::turn::v1::Turn) -> Self {
        Self {
            turn_id: turn.turn_id.clone(),
            status: TurnState::from(turn.status),
            prompt: turn.prompt.clone(),
            queued_at: turn.queued_at.as_ref().and_then(to_utc),
        }
    }
}

/// One event from a session's thread.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub session_id: Uuid,
    /// Strictly increasing per thread. Clients merge live and fetched events on it.
    pub sequence: u64,
    pub turn_id: Option<String>,
    pub occurred_at: Option<DateTime<Utc>>,
    /// The satellite's stable wire name, such as `agent.message`.
    #[serde(rename = "type")]
    pub event_type: String,
    pub member_id: Option<String>,
    pub payload: Option<EventPayload>,
}

impl SessionEvent {
    pub fn new(session_id: Uuid, thread_event: ThreadEvent) -> Self {
        Self {
            session_id,
            sequence: thread_event.sequence,
            turn_id: thread_event.turn_id,
            occurred_at: thread_event.occurred_at.as_ref().and_then(to_utc),
            event_type: thread_event.r#type,
            member_id: thread_event.member_id,
            payload: thread_event.payload.and_then(EventPayload::from_wire),
        }
    }
}

/// Who produced an agent event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorView {
    /// `agent`, `commander`, `member`, `subagent`, `planner`, `reviewer`, or `suggestions`.
    pub kind: &'static str,
    pub member_id: Option<String>,
    pub role: Option<String>,
}

impl From<Author> for AuthorView {
    fn from(author: Author) -> Self {
        let kind = match AuthorKind::try_from(author.kind) {
            Ok(AuthorKind::Commander) => "commander",
            Ok(AuthorKind::Member) => "member",
            Ok(AuthorKind::Subagent) => "subagent",
            Ok(AuthorKind::Planner) => "planner",
            Ok(AuthorKind::Reviewer) => "reviewer",
            Ok(AuthorKind::Suggestions) => "suggestions",
            Ok(AuthorKind::Agent | AuthorKind::Unspecified) | Err(_) => "agent",
        };
        Self {
            kind,
            member_id: author.member_id,
            role: author.role,
        }
    }
}

/// A question an agent asked, with its suggested answers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    pub title: String,
    pub detail: Option<String>,
    pub options: Vec<String>,
}

impl From<interaction::v1::Question> for QuestionView {
    fn from(question: interaction::v1::Question) -> Self {
        Self {
            title: question.title,
            detail: question.detail,
            options: question
                .options
                .into_iter()
                .map(|option| option.title)
                .collect(),
        }
    }
}

/// The payloads Elysium renders, tagged by `kind`.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EventPayload {
    AgentMessage {
        author: Option<AuthorView>,
        text: String,
    },
    AgentThinking {
        author: Option<AuthorView>,
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
    PlanProposed {
        body: String,
    },
    QuestionAsked {
        questions: Vec<QuestionView>,
    },
    BudgetWarning {
        percent_used: u32,
    },
    Incident {
        code: &'static str,
        message: String,
        retryable: bool,
    },
}

impl EventPayload {
    /// Converts the payloads Elysium renders; every other arm becomes `None`.
    fn from_wire(payload: thread_event::Payload) -> Option<Self> {
        use thread_event::Payload;

        let converted = match payload {
            Payload::AgentMessage(message) => Self::AgentMessage {
                author: message.author.map(AuthorView::from),
                text: message.text,
            },
            Payload::AgentThinking(thinking) => Self::AgentThinking {
                author: thinking.author.map(AuthorView::from),
                text: thinking.text,
            },
            Payload::ToolStarted(started) => Self::ToolStarted {
                tool_call_id: started.tool_call_id,
                tool_name: started.tool_name,
                input: started.input.map(struct_to_json),
            },
            Payload::ToolCompleted(completed) => Self::ToolCompleted {
                tool_call_id: completed.tool_call_id,
                tool_name: completed.tool_name,
                ok: completed.ok,
                output_preview: completed.output_preview,
                elapsed_milliseconds: completed.elapsed.as_ref().map(to_milliseconds),
            },
            Payload::TurnStarted(started) => Self::TurnStarted {
                prompt: started.turn.map(|turn| turn.prompt).unwrap_or_default(),
            },
            Payload::TurnCompleted(completed) => {
                let result = completed.result.unwrap_or_default();
                Self::TurnCompleted {
                    status: TurnState::from(result.status),
                    summary: result.summary,
                    error: result.error.map(|error| error.message),
                }
            }
            Payload::PlanProposed(proposed) => Self::PlanProposed {
                body: proposed.plan.map(|plan| plan.body).unwrap_or_default(),
            },
            Payload::QuestionAsked(asked) => Self::QuestionAsked {
                questions: asked
                    .question_set
                    .map(|set| set.questions.into_iter().map(QuestionView::from).collect())
                    .unwrap_or_default(),
            },
            Payload::BudgetWarning(warning) => Self::BudgetWarning {
                percent_used: warning.percent_used,
            },
            Payload::Incident(incident) => Self::Incident {
                code: ErrorCode::try_from(incident.code)
                    .map_or("ERROR_CODE_UNSPECIFIED", |code| code.as_str_name()),
                message: incident.message,
                retryable: incident.retryable,
            },
            _ => return None,
        };

        Some(converted)
    }
}

/// A contract timestamp as a UTC instant; `None` when it is out of chrono's range.
fn to_utc(timestamp: &Timestamp) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(timestamp.epoch_seconds, timestamp.nanos)
}

fn to_milliseconds(duration: &Duration) -> i64 {
    duration
        .seconds
        .saturating_mul(1_000)
        .saturating_add(i64::from(duration.nanos / 1_000_000))
}

/// Tool input arrives as a protobuf `Struct`, which is JSON by definition.
fn struct_to_json(input: prost_types::Struct) -> Value {
    Value::Object(
        input
            .fields
            .into_iter()
            .map(|(key, value)| (key, value_to_json(value)))
            .collect(),
    )
}

fn value_to_json(value: prost_types::Value) -> Value {
    match value.kind {
        None | Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::BoolValue(boolean)) => Value::Bool(boolean),
        Some(Kind::NumberValue(number)) => {
            serde_json::Number::from_f64(number).map_or(Value::Null, Value::Number)
        }
        Some(Kind::StringValue(text)) => Value::String(text),
        Some(Kind::StructValue(nested)) => struct_to_json(nested),
        Some(Kind::ListValue(list)) => {
            Value::Array(list.values.into_iter().map(value_to_json).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use arsox_sdk::proto::event::v1::{AgentMessage, ToolStarted};
    use serde_json::json;

    use super::*;

    fn wire_event(payload: Option<thread_event::Payload>, event_type: &str) -> ThreadEvent {
        ThreadEvent {
            sequence: 7,
            thread_id: "thread-1".to_owned(),
            turn_id: Some("turn-1".to_owned()),
            occurred_at: Some(Timestamp {
                epoch_seconds: 1_800_000_000,
                nanos: 0,
                timezone: "America/Denver".to_owned(),
            }),
            r#type: event_type.to_owned(),
            member_id: None,
            payload,
        }
    }

    #[test]
    fn agent_messages_render_with_their_author_and_a_utc_timestamp() {
        let message = thread_event::Payload::AgentMessage(AgentMessage {
            author: Some(Author {
                kind: AuthorKind::Commander.into(),
                member_id: None,
                role: Some("lead".to_owned()),
                parent_member_id: None,
            }),
            text: "Done.".to_owned(),
        });
        let session_id = Uuid::nil();
        let view = SessionEvent::new(session_id, wire_event(Some(message), "agent.message"));

        assert_eq!(
            serde_json::to_value(view).expect("serializes"),
            json!({
                "sessionId": session_id,
                "sequence": 7,
                "turnId": "turn-1",
                "occurredAt": "2027-01-15T08:00:00Z",
                "type": "agent.message",
                "memberId": null,
                "payload": {
                    "kind": "agentMessage",
                    "author": { "kind": "commander", "memberId": null, "role": "lead" },
                    "text": "Done.",
                },
            })
        );
    }

    #[test]
    fn tool_input_structs_become_plain_json() {
        let input = prost_types::Struct {
            fields: [
                (
                    "command".to_owned(),
                    prost_types::Value {
                        kind: Some(Kind::StringValue("ls".to_owned())),
                    },
                ),
                (
                    "timeout".to_owned(),
                    prost_types::Value {
                        kind: Some(Kind::NumberValue(30.0)),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        let started = thread_event::Payload::ToolStarted(ToolStarted {
            author: None,
            tool_call_id: "call-1".to_owned(),
            tool_name: "Bash".to_owned(),
            input: Some(input),
        });
        let view = SessionEvent::new(Uuid::nil(), wire_event(Some(started), "tool.started"));

        let payload = serde_json::to_value(view.payload).expect("serializes");
        assert_eq!(
            payload["input"],
            json!({ "command": "ls", "timeout": 30.0 })
        );
    }

    #[test]
    fn unrendered_and_unknown_payloads_keep_their_wire_type() {
        let view = SessionEvent::new(Uuid::nil(), wire_event(None, "team.member.spawned"));
        assert_eq!(view.event_type, "team.member.spawned");
        assert!(view.payload.is_none());
    }

    #[test]
    fn unknown_enum_values_degrade_instead_of_failing() {
        assert_eq!(ThreadState::from(999), ThreadState::Unknown);
        assert_eq!(TurnState::from(-1), TurnState::Unknown);
        assert_eq!(ThreadState::from(4), ThreadState::AwaitingInput);
    }
}
