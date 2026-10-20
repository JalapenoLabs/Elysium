// Copyright © 2026 Jalapeno Labs

//! What a turn reports, and how Claude Code's `stream-json` output becomes it.
//!
//! `claude -p --output-format stream-json --verbose` prints one JSON object per line, tagged by
//! `type`. The ones that matter:
//!
//! | `type`             | Carries                                                       |
//! |--------------------|---------------------------------------------------------------|
//! | `system` / `init`  | The harness session id and model                              |
//! | `assistant`        | Content blocks: `text`, `thinking`, `tool_use`                |
//! | `user`             | `tool_result` blocks answering earlier `tool_use` ids         |
//! | `rate_limit_event` | The subscription's limit status, `rejected` once it is spent  |
//! | `result`           | The final text, `is_error`, cost, and token usage             |
//!
//! [`StreamParser`] turns each line into zero or more [`RunnerEvent`]s and remembers what the
//! turn's outcome needs (the session id, the result line, a rejected limit). Lines it does not
//! understand become [`RunnerEvent::Other`] rather than being dropped, because the schema
//! moves between Claude Code releases and a later reader may know what to do with them.
//! Claude's other `system` subtypes are progress telemetry and are dropped.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Longest tool output kept on a [`RunnerEvent::ToolCompleted`]. The conversation shows a
/// preview; a full file read or build log would bloat every stored event.
const OUTPUT_PREVIEW_CHARACTERS: usize = 2_000;

/// Claude Code's own id for a conversation, which `--resume` continues.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HarnessSessionId(String);

impl HarnessSessionId {
    /// The id as Claude Code prints it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HarnessSessionId {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The id was blank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a harness session id cannot be blank")]
pub struct BlankHarnessSessionId;

impl FromStr for HarnessSessionId {
    type Err = BlankHarnessSessionId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(BlankHarnessSessionId);
        }
        Ok(Self(trimmed.to_owned()))
    }
}

/// One thing a turn did, in the order it happened. Every turn's events end with exactly one
/// [`RunnerEvent::Finished`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RunnerEvent {
    /// The turn left the queue and its process started.
    TurnStarted {
        turn_id: Uuid,
        prompt: String,
    },
    AgentMessage {
        text: String,
    },
    AgentThinking {
        text: String,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        input: Value,
    },
    ToolCompleted {
        tool_call_id: String,
        tool_name: String,
        ok: bool,
        output_preview: String,
    },
    /// Claude's `rate_limit_event`, kept verbatim.
    RateLimit {
        info: Value,
    },
    /// A spent or rejected credential handed the turn to the next one.
    CredentialRolledOver {
        from: Uuid,
        to: Uuid,
        reason: String,
    },
    /// A line the parser does not understand, kept as it arrived.
    Other {
        raw: Value,
    },
    Finished(TurnOutcome),
}

/// How a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurnStatus {
    Completed,
    Failed,
    Cancelled,
    /// Stopped from outside: a heartbeat timeout, a dead process, or Elysium shutting down.
    Interrupted,
    /// Every credential was spent.
    UsageLimited,
}

/// What a finished turn reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnOutcome {
    pub status: TurnStatus,
    /// Claude's final text.
    pub result: Option<String>,
    pub error: Option<String>,
    /// Pass back as the next turn's `resume` to continue the conversation.
    pub harness_session_id: Option<HarnessSessionId>,
    /// Millionths of a US dollar, summed over every attempt. Never a float.
    pub cost_usd_micros: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

impl TurnOutcome {
    /// An outcome for a turn that ended without a result line of its own.
    pub fn without_result(status: TurnStatus, error: impl Into<String>) -> Self {
        Self {
            status,
            result: None,
            error: Some(error.into()),
            harness_session_id: None,
            cost_usd_micros: None,
            input_tokens: None,
            output_tokens: None,
        }
    }
}

/// Why one run of `claude` ended without doing its work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptFailure {
    /// The credential's quota or credits are spent; the next one may serve.
    SpentCredential(String),
    /// The credential was refused outright; the next one may serve.
    RejectedCredential(String),
    /// Anything else. Another credential would fail the same way.
    Other(String),
}

impl AttemptFailure {
    /// Whether the next credential should take the turn over.
    pub const fn rolls_over(&self) -> bool {
        matches!(self, Self::SpentCredential(_) | Self::RejectedCredential(_))
    }

    pub fn reason(&self) -> &str {
        match self {
            Self::SpentCredential(reason)
            | Self::RejectedCredential(reason)
            | Self::Other(reason) => reason,
        }
    }
}

/// What one run of `claude` reported, before failover decides what the turn does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptReport {
    pub harness_session_id: Option<HarnessSessionId>,
    pub result: Option<String>,
    pub failure: Option<AttemptFailure>,
    pub cost_usd_micros: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

/// Phrases Claude Code's result text uses when a subscription or account has no quota left.
/// Matched lowercase; the wording has changed between releases, so several are kept.
const SPENT_PHRASES: &[&str] = &[
    "usage limit",
    "hit your limit",
    "limit reached",
    "credit balance is too low",
    "out of extra usage",
];

/// Phrases for a credential that is wrong rather than spent.
const REJECTED_PHRASES: &[&str] = &[
    "invalid api key",
    "invalid bearer token",
    "oauth token has expired",
    "authentication_error",
    "not logged in",
    "please run /login",
];

/// Reads one turn attempt's output line by line.
#[derive(Debug, Default)]
pub struct StreamParser {
    /// Tool names by call id, so a result can name the tool it answers.
    tool_names: HashMap<String, String>,
    harness_session_id: Option<HarnessSessionId>,
    result: Option<ResultLine>,
    limit_rejected: bool,
}

#[derive(Debug, Clone)]
struct ResultLine {
    text: Option<String>,
    is_error: bool,
    cost_usd_micros: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
}

impl StreamParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses one line of output. Blank lines yield nothing; anything that is not JSON is
    /// kept as [`RunnerEvent::Other`] holding the text.
    pub fn parse_line(&mut self, line: &str) -> Vec<RunnerEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return vec![RunnerEvent::Other {
                raw: Value::String(trimmed.to_owned()),
            }];
        };

        if let Some(session_id) = value
            .get("session_id")
            .and_then(Value::as_str)
            .and_then(|session_id| session_id.parse().ok())
        {
            self.harness_session_id = Some(session_id);
        }

        match value.get("type").and_then(Value::as_str) {
            Some("system") => Vec::new(),
            Some("assistant") => self.parse_assistant(value),
            Some("user") => self.parse_user(&value),
            Some("rate_limit_event") => {
                let info = value.get("rate_limit_info").cloned().unwrap_or(Value::Null);
                if info.get("status").and_then(Value::as_str) == Some("rejected") {
                    self.limit_rejected = true;
                }
                vec![RunnerEvent::RateLimit { info }]
            }
            Some("result") => {
                self.result = Some(read_result(&value));
                Vec::new()
            }
            _ => vec![RunnerEvent::Other { raw: value }],
        }
    }

    /// What the attempt amounted to once its process has exited with `exit_code`.
    pub fn finish(self, exit_code: Option<i64>) -> AttemptReport {
        let Some(result) = self.result else {
            let reason = match exit_code {
                Some(code) => format!("claude exited with status {code} before reporting a result"),
                None => "claude ended before reporting a result".to_owned(),
            };
            let failure = if self.limit_rejected {
                AttemptFailure::SpentCredential(reason)
            } else {
                AttemptFailure::Other(reason)
            };
            return AttemptReport {
                harness_session_id: self.harness_session_id,
                result: None,
                failure: Some(failure),
                cost_usd_micros: None,
                input_tokens: None,
                output_tokens: None,
            };
        };

        let failure = result
            .is_error
            .then(|| classify_failure(result.text.as_deref(), self.limit_rejected));
        AttemptReport {
            harness_session_id: self.harness_session_id,
            result: result.text,
            failure,
            cost_usd_micros: result.cost_usd_micros,
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
        }
    }

    fn parse_assistant(&mut self, value: Value) -> Vec<RunnerEvent> {
        let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) else {
            return vec![RunnerEvent::Other { raw: value }];
        };

        let mut events = Vec::with_capacity(blocks.len());
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        events.push(RunnerEvent::AgentMessage {
                            text: text.to_owned(),
                        });
                    }
                }
                Some("thinking") => {
                    let text = block.get("thinking").and_then(Value::as_str).unwrap_or("");
                    if !text.trim().is_empty() {
                        events.push(RunnerEvent::AgentThinking {
                            text: text.to_owned(),
                        });
                    }
                }
                Some("tool_use") => {
                    let tool_call_id = string_field(block, "id");
                    let tool_name = string_field(block, "name");
                    self.tool_names
                        .insert(tool_call_id.clone(), tool_name.clone());
                    events.push(RunnerEvent::ToolStarted {
                        tool_call_id,
                        tool_name,
                        input: block.get("input").cloned().unwrap_or(Value::Null),
                    });
                }
                // `redacted_thinking` and anything newer carry nothing to show.
                _ => {}
            }
        }
        events
    }

    fn parse_user(&self, value: &Value) -> Vec<RunnerEvent> {
        let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) else {
            // A user line without blocks is the prompt echoed back, which the turn already
            // reported when it started.
            return Vec::new();
        };

        let mut events = Vec::new();
        for block in blocks {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let tool_call_id = string_field(block, "tool_use_id");
            let tool_name = self
                .tool_names
                .get(&tool_call_id)
                .cloned()
                .unwrap_or_default();
            let is_error = block
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            events.push(RunnerEvent::ToolCompleted {
                tool_call_id,
                tool_name,
                ok: !is_error,
                output_preview: preview(&flatten_tool_output(block.get("content"))),
            });
        }
        events
    }
}

fn read_result(value: &Value) -> ResultLine {
    let usage = value.get("usage");
    let token_count = |key: &str| {
        usage
            .and_then(|usage| usage.get(key))
            .and_then(Value::as_i64)
    };
    ResultLine {
        text: value
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_owned),
        // `is_error` decides; an older shape only had the subtype. A "Not logged in" result
        // reports `subtype: success` with `is_error: true`.
        is_error: value
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                value
                    .get("subtype")
                    .and_then(Value::as_str)
                    .is_some_and(|subtype| subtype != "success")
            }),
        cost_usd_micros: value.get("total_cost_usd").and_then(dollars_to_micros),
        input_tokens: token_count("input_tokens"),
        output_tokens: token_count("output_tokens"),
    }
}

fn classify_failure(text: Option<&str>, limit_rejected: bool) -> AttemptFailure {
    let reason = text
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("claude reported an error without a message")
        .to_owned();
    let lowered = reason.to_lowercase();
    if limit_rejected || SPENT_PHRASES.iter().any(|phrase| lowered.contains(phrase)) {
        return AttemptFailure::SpentCredential(reason);
    }
    if REJECTED_PHRASES
        .iter()
        .any(|phrase| lowered.contains(phrase))
    {
        return AttemptFailure::RejectedCredential(reason);
    }
    AttemptFailure::Other(reason)
}

/// Converts Claude's `total_cost_usd` to millionths of a dollar from its decimal text, so no
/// float arithmetic touches money. Claude prints plain decimals (`0.0123`); anything else
/// (an exponent, a negative) is refused rather than guessed at.
fn dollars_to_micros(value: &Value) -> Option<i64> {
    let text = match value {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        _ => return None,
    };
    let (whole, fraction) = text.split_once('.').unwrap_or((text.as_str(), ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    // Digits past the sixth are a fraction of a micro-dollar, below anything billed.
    let micros: String = fraction.chars().chain("000000".chars()).take(6).collect();
    let whole: i64 = whole.parse().ok()?;
    let micros: i64 = micros.parse().ok()?;
    whole.checked_mul(1_000_000)?.checked_add(micros)
}

fn string_field(block: &Value, key: &str) -> String {
    block
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Tool output is a string or a list of content blocks; only text blocks carry anything a
/// preview can show.
fn flatten_tool_output(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => {
            let mut text = String::new();
            for block in blocks {
                let Some(part) = block.get("text").and_then(Value::as_str) else {
                    continue;
                };
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(part);
            }
            text
        }
        _ => String::new(),
    }
}

fn preview(text: &str) -> String {
    let mut characters = text.char_indices();
    match characters.nth(OUTPUT_PREVIEW_CHARACTERS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse_all(lines: &[&str]) -> (Vec<RunnerEvent>, StreamParser) {
        let mut parser = StreamParser::new();
        let mut events = Vec::new();
        for line in lines {
            events.extend(parser.parse_line(line));
        }
        (events, parser)
    }

    #[test]
    fn a_tool_round_trip_names_the_tool_on_both_ends() {
        let (events, _parser) = parse_all(&[
            r#"{"type":"assistant","message":{"content":[
                {"type":"text","text":"Reading it."},
                {"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"a.rs"}}
            ]}}"#,
            r#"{"type":"user","message":{"content":[
                {"type":"tool_result","tool_use_id":"toolu_1","content":[{"type":"text","text":"fn main() {}"}]}
            ]}}"#,
        ]);
        assert_eq!(
            events,
            vec![
                RunnerEvent::AgentMessage {
                    text: "Reading it.".to_owned()
                },
                RunnerEvent::ToolStarted {
                    tool_call_id: "toolu_1".to_owned(),
                    tool_name: "Read".to_owned(),
                    input: json!({ "file_path": "a.rs" }),
                },
                RunnerEvent::ToolCompleted {
                    tool_call_id: "toolu_1".to_owned(),
                    tool_name: "Read".to_owned(),
                    ok: true,
                    output_preview: "fn main() {}".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_failed_tool_call_is_not_ok() {
        let (events, _parser) = parse_all(&[r#"{"type":"user","message":{"content":[
                {"type":"tool_result","tool_use_id":"toolu_9","content":"no such file","is_error":true}
            ]}}"#]);
        assert!(matches!(
            &*events,
            [RunnerEvent::ToolCompleted { ok: false, output_preview, .. }] if output_preview == "no such file"
        ));
    }

    #[test]
    fn thinking_is_kept_and_empty_thinking_is_not() {
        let (events, _parser) = parse_all(&[r#"{"type":"assistant","message":{"content":[
                {"type":"thinking","thinking":"Check the config first."},
                {"type":"thinking","thinking":"  "},
                {"type":"redacted_thinking","data":"x"}
            ]}}"#]);
        assert_eq!(
            events,
            vec![RunnerEvent::AgentThinking {
                text: "Check the config first.".to_owned()
            }]
        );
    }

    #[test]
    fn system_telemetry_is_dropped_and_unknown_lines_are_kept() {
        let (events, _parser) = parse_all(&[
            r#"{"type":"system","subtype":"init","session_id":"s-1","model":"claude-opus-5"}"#,
            r#"{"type":"system","subtype":"compact_boundary"}"#,
            r#"{"type":"something_new","value":1}"#,
            "warning: not json",
        ]);
        assert_eq!(
            events,
            vec![
                RunnerEvent::Other {
                    raw: json!({ "type": "something_new", "value": 1 })
                },
                RunnerEvent::Other {
                    raw: json!("warning: not json")
                },
            ]
        );
    }

    #[test]
    fn a_successful_result_reports_its_session_cost_and_tokens() {
        let (_events, parser) = parse_all(&[
            r#"{"type":"system","subtype":"init","session_id":"0f8d-session"}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Done.","session_id":"0f8d-session",
               "total_cost_usd":0.0123456789,"usage":{"input_tokens":1200,"output_tokens":340}}"#,
        ]);
        let report = parser.finish(Some(0));
        assert_eq!(report.failure, None);
        assert_eq!(report.result.as_deref(), Some("Done."));
        assert_eq!(
            report
                .harness_session_id
                .as_ref()
                .map(HarnessSessionId::as_str),
            Some("0f8d-session")
        );
        assert_eq!(report.cost_usd_micros, Some(12_345));
        assert_eq!(report.input_tokens, Some(1_200));
        assert_eq!(report.output_tokens, Some(340));
    }

    #[test]
    fn a_spent_subscription_rolls_over() {
        let (events, parser) = parse_all(&[
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour"}}"#,
            r#"{"type":"result","subtype":"success","is_error":true,"result":"You've hit your limit · resets 3pm"}"#,
        ]);
        assert!(matches!(&*events, [RunnerEvent::RateLimit { .. }]));
        let failure = parser.finish(Some(1)).failure.expect("a failure");
        assert!(matches!(failure, AttemptFailure::SpentCredential(_)));
        assert!(failure.rolls_over());
    }

    #[test]
    fn a_refused_credential_rolls_over_and_an_ordinary_error_does_not() {
        let (_events, parser) = parse_all(&[
            r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key · Please run /login"}"#,
        ]);
        let failure = parser.finish(Some(1)).failure.expect("a failure");
        assert!(matches!(failure, AttemptFailure::RejectedCredential(_)));

        let (_events, parser) = parse_all(&[
            r#"{"type":"result","subtype":"error_max_turns","result":"Reached the turn limit"}"#,
        ]);
        let failure = parser.finish(Some(1)).failure.expect("a failure");
        assert!(matches!(failure, AttemptFailure::Other(_)));
        assert!(!failure.rolls_over());
    }

    #[test]
    fn a_process_that_dies_without_a_result_is_a_failure() {
        let (_events, parser) =
            parse_all(&[r#"{"type":"system","subtype":"init","session_id":"s-2"}"#]);
        let report = parser.finish(Some(137));
        assert_eq!(
            report.failure,
            Some(AttemptFailure::Other(
                "claude exited with status 137 before reporting a result".to_owned()
            ))
        );
        assert!(
            report.harness_session_id.is_some(),
            "the session can still be resumed"
        );
    }

    #[test]
    fn dollars_convert_to_micros_without_floats() {
        assert_eq!(dollars_to_micros(&json!(0.5)), Some(500_000));
        assert_eq!(dollars_to_micros(&json!(2)), Some(2_000_000));
        assert_eq!(dollars_to_micros(&json!("1.000001")), Some(1_000_001));
        assert_eq!(dollars_to_micros(&json!(-1.0)), None);
        assert_eq!(dollars_to_micros(&json!("1e-5")), None);
    }

    #[test]
    fn long_tool_output_is_cut_to_a_preview() {
        let long = "x".repeat(OUTPUT_PREVIEW_CHARACTERS + 50);
        let cut = preview(&long);
        assert_eq!(cut.chars().count(), OUTPUT_PREVIEW_CHARACTERS + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(preview("short"), "short");
    }

    #[test]
    fn events_serialize_with_the_conversation_views_kinds() {
        let event = RunnerEvent::ToolStarted {
            tool_call_id: "toolu_1".to_owned(),
            tool_name: "Bash".to_owned(),
            input: json!({ "command": "ls" }),
        };
        assert_eq!(
            serde_json::to_value(&event).expect("serializes"),
            json!({ "kind": "toolStarted", "toolCallId": "toolu_1", "toolName": "Bash", "input": { "command": "ls" } })
        );
    }

    #[test]
    fn harness_session_ids_round_trip_through_text() {
        let id: HarnessSessionId = " abc ".parse().expect("parses");
        assert_eq!(id.to_string(), "abc");
        assert_eq!(serde_json::to_value(&id).expect("serializes"), json!("abc"));
        assert_eq!("  ".parse::<HarnessSessionId>(), Err(BlankHarnessSessionId));
    }
}
