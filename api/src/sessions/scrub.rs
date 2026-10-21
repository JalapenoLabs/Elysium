// Copyright © 2026 Jalapeno Labs

//! Keeps a turn's secrets out of what Elysium records and shows of it.
//!
//! A turn's environment carries secrets: the session's GitHub token and every environment
//! variable marked secret. The agent can read them and may print one, in a command it runs or
//! in what it says. Before an event is recorded or published, every string in it has each
//! secret replaced with [`REDACTED`]. The agent still holds the secrets; this only keeps them
//! out of the history, the event stream, and the browser.
//!
//! Values shorter than [`MIN_SECRET_CHARACTERS`] are not scrubbed: replacing every `1` or
//! `yes` would garble the conversation and protect nothing.

use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use crate::runner::RunnerEvent;

/// What a secret is replaced with.
pub const REDACTED: &str = "[secret]";

/// Shorter values are left alone; see the module documentation.
const MIN_SECRET_CHARACTERS: usize = 6;

/// The secrets of one turn.
#[derive(Debug, Default)]
pub struct Scrubber {
    secrets: Vec<SecretString>,
}

impl Scrubber {
    /// Scrubs every value in `secrets` long enough to matter, longest first, so a secret that
    /// contains another is replaced whole.
    pub fn new(secrets: impl IntoIterator<Item = SecretString>) -> Self {
        let mut secrets: Vec<SecretString> = secrets
            .into_iter()
            .filter(|secret| secret.expose_secret().chars().count() >= MIN_SECRET_CHARACTERS)
            .collect();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.expose_secret().len()));
        Self { secrets }
    }

    /// `event` with every secret replaced. An event that cannot be read as JSON is returned
    /// as it is, which only an event type without a JSON form could cause.
    pub fn scrub(&self, event: RunnerEvent) -> RunnerEvent {
        if self.secrets.is_empty() {
            return event;
        }
        let Ok(mut value) = serde_json::to_value(&event) else {
            return event;
        };
        self.scrub_value(&mut value);
        serde_json::from_value(value).unwrap_or(event)
    }

    /// `text` with every secret replaced, such as a git error a clone printed.
    pub fn scrub_text(&self, text: &str) -> String {
        let mut scrubbed = text.to_owned();
        self.scrub_string(&mut scrubbed);
        scrubbed
    }

    fn scrub_string(&self, text: &mut String) {
        for secret in &self.secrets {
            let exposed = secret.expose_secret();
            if text.contains(exposed) {
                *text = text.replace(exposed, REDACTED);
            }
        }
    }

    fn scrub_value(&self, value: &mut Value) {
        match value {
            Value::String(text) => self.scrub_string(text),
            Value::Array(items) => {
                for item in items {
                    self.scrub_value(item);
                }
            }
            Value::Object(fields) => {
                for field in fields.values_mut() {
                    self.scrub_value(field);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn secrets_are_replaced_wherever_they_appear() {
        let scrubber = Scrubber::new([
            SecretString::from("ghp_abcdef123456"),
            SecretString::from("abc"),
        ]);
        let event = RunnerEvent::ToolStarted {
            tool_call_id: "toolu_1".to_owned(),
            tool_name: "Bash".to_owned(),
            input: json!({ "command": "curl -H 'Authorization: token ghp_abcdef123456' https://api.github.com" }),
        };
        let RunnerEvent::ToolStarted { input, .. } = scrubber.scrub(event) else {
            panic!("the event keeps its kind");
        };
        assert_eq!(
            input["command"],
            "curl -H 'Authorization: token [secret]' https://api.github.com"
        );

        let RunnerEvent::AgentMessage { text } = scrubber.scrub(RunnerEvent::AgentMessage {
            text: "abc is too short to scrub".to_owned(),
        }) else {
            panic!("the event keeps its kind");
        };
        assert_eq!(text, "abc is too short to scrub");
    }

    #[test]
    fn the_longer_secret_is_replaced_whole() {
        let scrubber = Scrubber::new([
            SecretString::from("token-1234"),
            SecretString::from("token-1234-extended"),
        ]);
        let RunnerEvent::AgentMessage { text } = scrubber.scrub(RunnerEvent::AgentMessage {
            text: "token-1234-extended".to_owned(),
        }) else {
            panic!("the event keeps its kind");
        };
        assert_eq!(text, REDACTED);
    }
}
