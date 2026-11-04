// Copyright © 2026 Jalapeno Labs

//! The request workflow: what Elysium does with a conversation once it goes quiet.
//!
//! A request has nothing to fetch, transcribe, or correct, so the workflow starts at segmenting:
//! [`SegmentMeeting`] reads the conversation's messages, cuts them into topics, and attributes
//! each message to its topic's projects; then [`HandleRequest`] gives the messages to the
//! conversation's agent, which raises an action item for each question or request the person
//! owes. A run is about one conversation (source `conversation`), so a conversation has one live
//! run at a time, and messages that arrive while it is live go to its agent.

use uuid::Uuid;

use super::handle::HandleRequest;
use crate::meetings::segment::{ConversationToSegment, SegmentMeeting, TranscriptToSegment};
use crate::models::conversation::Conversation;
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::NewRun;
use crate::workflows::{self, Start, Workflow};

/// What a request run is about, as `workflow_runs.source_kind` names it.
pub const SOURCE_KIND: &str = "conversation";

/// The request workflow: every step a conversation's run goes through, first to last.
pub fn workflow() -> Workflow<TranscriptToSegment> {
    workflows::declare("request")
        .first::<SegmentMeeting>()
        .then::<HandleRequest>()
        .build()
}

/// A run of the request workflow on `conversation`, reading `record_ids`, which Elysium starts
/// itself once the conversation goes quiet.
pub fn run_for(conversation: &Conversation, record_ids: Vec<Uuid>) -> NewRun {
    workflow().new_run(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: conversation.id.to_string(),
        input: TranscriptToSegment::Conversation(ConversationToSegment {
            title: conversation.title.clone(),
            record_ids,
        }),
        created_by: SYSTEM_USER_ID,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_models::{self, AgentKind};

    #[test]
    fn a_request_starts_at_segmenting_and_is_handled_as_triage() {
        let outline = workflow().outline();
        let [segmenting, handling] = outline.steps() else {
            panic!("a request has two steps");
        };
        assert_eq!(segmenting.name(), "segment_meeting");
        assert_eq!(handling.name(), "handle_request");
        assert_eq!(handling.agent_kind(), Some(AgentKind::Triage));

        let declared = workflows::declared();
        assert!(
            declared.iter().any(|workflow| workflow.name() == "request"),
            "the engine checks and the settings page list the request workflow"
        );
        let handling = agent_models::catalog()
            .into_iter()
            .find(|entry| entry.name == "handle_request")
            .expect("the settings page lists the handling step");
        assert_eq!(handling.kind, Some(AgentKind::Triage));
    }

    #[test]
    fn a_run_reads_its_conversation_by_its_records() {
        let conversation = Conversation {
            id: Uuid::now_v7(),
            connector_id: Uuid::now_v7(),
            key: "T1/D1".to_owned(),
            title: "Ada in a direct message in Mooreslab".to_owned(),
            harness_session_id: None,
            waiting_until: None,
            waited_in_run_id: None,
            created_by: Uuid::now_v7(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let record = Uuid::now_v7();
        let run = run_for(&conversation, vec![record]);
        assert_eq!(run.workflow, "request");
        assert_eq!(run.source_kind, SOURCE_KIND);
        assert_eq!(run.source_id, conversation.id.to_string());
        assert_eq!(run.steps, ["segment_meeting", "handle_request"]);
        assert_eq!(
            run.input,
            serde_json::json!({
                "title": "Ada in a direct message in Mooreslab",
                "recordIds": [record],
            })
        );
    }
}
