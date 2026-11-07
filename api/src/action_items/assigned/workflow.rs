// Copyright © 2026 Jalapeno Labs

//! The assigned issue workflow: what Elysium does with an issue that came in assigned to its
//! person, as an inbox item in no project.
//!
//! It is the segmenting step alone, which reads an action item for the projects it belongs to
//! ([`SegmentMeeting`], `crate::models::action_item_attribution`): the item joins them with
//! Elysia's confidence, led by the one she is surest of. Nothing else starts: attributing an item
//! wakes no Project Manager. A run is about one item (source `action_item`), so an item has one
//! live run at a time.

use uuid::Uuid;

use crate::meetings::segment::{ItemToAttribute, SegmentMeeting, TranscriptToSegment};
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::NewRun;
use crate::workflows::{self, Start, Workflow};

/// What an assigned issue run is about, as `workflow_runs.source_kind` names it.
pub const SOURCE_KIND: &str = "action_item";

/// The assigned issue workflow: every step an assigned issue's run goes through.
pub fn workflow() -> Workflow<TranscriptToSegment> {
    workflows::declare("assigned_issue")
        .first::<SegmentMeeting>()
        .build()
}

/// A run of the assigned issue workflow on item `action_item_id`, which the watcher starts itself
/// as the issue comes in.
pub fn run_for(action_item_id: Uuid) -> NewRun {
    workflow().new_run(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: action_item_id.to_string(),
        input: TranscriptToSegment::ActionItem(ItemToAttribute { action_item_id }),
        created_by: SYSTEM_USER_ID,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent_models::AgentKind;
    use crate::meetings::correct::CorrectedMeeting;
    use crate::meetings::segment::ConversationToSegment;

    #[test]
    fn an_assigned_issue_is_attributed_by_the_segmenting_step() {
        let outline = workflow().outline();
        let [attributing] = outline.steps() else {
            panic!("an assigned issue has one step");
        };
        assert_eq!(attributing.name(), "segment_meeting");
        assert_eq!(attributing.agent_kind(), Some(AgentKind::Segmenting));
        assert!(
            workflows::declared()
                .iter()
                .any(|workflow| workflow.name() == "assigned_issue"),
            "the engine checks and the settings page list the assigned issue workflow"
        );
    }

    #[test]
    fn a_run_reads_its_item_by_id() {
        let item = Uuid::now_v7();
        let run = run_for(item);
        assert_eq!(run.workflow, "assigned_issue");
        assert_eq!(run.source_kind, SOURCE_KIND);
        assert_eq!(run.source_id, item.to_string());
        assert_eq!(run.steps, ["segment_meeting"]);
        assert_eq!(run.input, json!({ "actionItemId": item }));
    }

    #[test]
    fn each_thing_to_segment_reads_as_its_own_shape() {
        let item = Uuid::now_v7();
        let read: TranscriptToSegment =
            serde_json::from_value(json!({ "actionItemId": item })).expect("an item");
        assert_eq!(
            read,
            TranscriptToSegment::ActionItem(ItemToAttribute {
                action_item_id: item
            })
        );

        let meeting = Uuid::now_v7();
        let read: TranscriptToSegment =
            serde_json::from_value(json!({ "meetingId": meeting, "recording": null }))
                .expect("a meeting");
        assert!(matches!(
            read,
            TranscriptToSegment::Meeting(CorrectedMeeting { meeting_id, .. }) if meeting_id == meeting
        ));

        let record = Uuid::now_v7();
        let read: TranscriptToSegment =
            serde_json::from_value(json!({ "title": "#launch", "recordIds": [record] }))
                .expect("a conversation");
        assert_eq!(
            read,
            TranscriptToSegment::Conversation(ConversationToSegment {
                title: "#launch".to_owned(),
                record_ids: vec![record],
            })
        );

        serde_json::from_value::<TranscriptToSegment>(json!({ "actionItemId": item, "x": 1 }))
            .expect_err("an item holds its id alone");
    }
}
