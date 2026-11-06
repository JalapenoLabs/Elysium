// Copyright © 2026 Jalapeno Labs

//! `elysia_work_queue`, `elysia_change_batch`, and `elysia_cancel_batch`: Elysia reads the worker
//! pool's queue and changes what waits in it, as a person does on the work queue's page. She lists
//! the batches proposed for a person's approval too, but never decides on one: approving and
//! rejecting take a person's session (`crate::worker_pool::actions`), which no tool holds.
//!
//! What she changes she changes directly, as `docs/engine.md` gives her (Elysia: "she can see and
//! change anything in it"): every change goes through `crate::worker_pool::actions`, so it is
//! checked, written to the batch's history as hers, and announced exactly as a person's is. A
//! batch she cancels wakes its Project Manager with her reason.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::models::project;
use crate::models::user::ELYSIA_USER_ID;
use crate::models::work_batch::{ChangeError, Changes, Priority, RESULT_MAX_CHARACTERS, WorkBatch};
use crate::tools::{CallScope, ToolContext, ToolError, internal, parse_arguments};
use crate::worker_pool::{actions, queue};

pub fn queue_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

pub fn change_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "batchId": {
                "type": "string",
                "format": "uuid",
                "description": "The batch, from elysia_work_queue.",
            },
            "priority": {
                "enum": ["asap", "normal", "scheduled"],
                "description": "Its new priority; scheduled needs scheduledFor.",
            },
            "scheduledFor": {
                "type": "string",
                "format": "date-time",
                "description": "When a scheduled batch is due, with its UTC offset, within a year.",
            },
            "paused": {
                "type": "boolean",
                "description": "True holds it back, giving back a slot no worker started on; \
                    false lets it wait again.",
            },
            "runNext": {
                "type": "boolean",
                "description": "True runs it before everything else, after any batch already \
                    asked to run next; false puts it back in its place.",
            },
        },
        "required": ["batchId"],
        "additionalProperties": false,
    })
}

pub fn cancel_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "batchId": { "type": "string", "format": "uuid" },
            "reason": {
                "type": "string",
                "minLength": 1,
                "maxLength": RESULT_MAX_CHARACTERS,
                "description": "Why, for its Project Manager, which is told at once.",
            },
        },
        "required": ["batchId", "reason"],
        "additionalProperties": false,
    })
}

#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde reads {} only into a braced struct; a unit struct takes null"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArguments {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangeArguments {
    batch_id: Uuid,
    priority: Option<Priority>,
    scheduled_for: Option<DateTime<Utc>>,
    paused: Option<bool>,
    run_next: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelArguments {
    batch_id: Uuid,
    reason: String,
}

pub async fn run_queue(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let NoArguments {} = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    read_queue(&mut connection, Utc::now())
        .await
        .map_err(|error| internal(scope, "elysia.work_queue", &error))
}

/// The queue as she reads it, for [`run_queue`].
async fn read_queue(
    connection: &mut AsyncPgConnection,
    now: DateTime<Utc>,
) -> diesel::QueryResult<Value> {
    let waiting = queue::read(connection, now).await?;
    let projects: HashMap<Uuid, String> = project::list(connection)
        .await?
        .into_iter()
        .map(|project| (project.id, project.name))
        .collect();
    let view = |batch: &WorkBatch| {
        json!({
            "batchId": batch.id,
            "projectId": batch.project_id,
            "project": projects.get(&batch.project_id),
            "title": batch.title,
            "priority": batch.priority,
            "scheduledFor": batch.scheduled_for,
            "state": batch.state,
            "runNext": batch.pinned_at.is_some(),
            "submittedAt": batch.created_at,
        })
    };
    Ok(json!({
        "pool": {
            "size": waiting.pool.size,
            "inUse": waiting.pool.in_use,
            "heldByUsageUntil": waiting.pool.usage_held_until,
        },
        "inSlots": waiting.in_slots.iter().map(view).collect::<Vec<_>>(),
        "next": waiting.next.iter().map(view).collect::<Vec<_>>(),
        "scheduledLater": waiting.scheduled.iter().map(view).collect::<Vec<_>>(),
        "paused": waiting.paused.iter().map(view).collect::<Vec<_>>(),
        "proposed": waiting.proposed.iter().map(view).collect::<Vec<_>>(),
        "note": "next is the order the pool hands out free slots in. A batch in a slot whose \
            state is assigned waits for its worker and may still be paused or cancelled. A \
            proposed batch waits for a person to approve, edit, or reject it on the work queue; \
            only a person does, so tell them when one needs deciding.",
    }))
}

pub async fn run_change(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let arguments: ChangeArguments = parse_arguments(arguments)?;
    if arguments.priority.is_none() && arguments.scheduled_for.is_some() {
        return Err(ToolError::Invalid(
            "scheduledFor goes with priority scheduled".to_owned(),
        ));
    }
    let changes = Changes {
        priority: arguments
            .priority
            .map(|priority| (priority, arguments.scheduled_for)),
        paused: arguments.paused,
        run_next: arguments.run_next,
    };
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let batch = actions::change(
        &mut connection,
        &context.events,
        arguments.batch_id,
        changes,
        ELYSIA_USER_ID,
    )
    .await
    .map_err(|error| {
        refused_or_internal(scope, "elysia.change_batch", arguments.batch_id, error)
    })?;
    Ok(json!({ "batch": batch_view(&batch) }))
}

pub async fn run_cancel(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let arguments: CancelArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let batch = actions::cancel(
        &mut connection,
        &context.events,
        arguments.batch_id,
        None,
        &arguments.reason,
        ELYSIA_USER_ID,
    )
    .await
    .map_err(|error| {
        refused_or_internal(scope, "elysia.cancel_batch", arguments.batch_id, error)
    })?;
    Ok(json!({
        "batch": batch_view(&batch),
        "note": "Cancelled. Its Project Manager is told why on its next turn.",
    }))
}

fn batch_view(batch: &WorkBatch) -> Value {
    json!({
        "batchId": batch.id,
        "projectId": batch.project_id,
        "title": batch.title,
        "priority": batch.priority,
        "scheduledFor": batch.scheduled_for,
        "state": batch.state,
        "runNext": batch.pinned_at.is_some(),
        "result": batch.result,
    })
}

fn refused_or_internal(
    scope: &CallScope,
    operation: &'static str,
    batch_id: Uuid,
    error: ChangeError,
) -> ToolError {
    match error {
        ChangeError::NotFound => ToolError::NotFound(format!(
            "no batch {batch_id}; call elysia_work_queue for the batches that wait"
        )),
        ChangeError::Refused(reason) | ChangeError::Invalid(reason) => ToolError::Invalid(reason),
        ChangeError::Database(error) => internal(scope, operation, &error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::project::NewProject;
    use crate::models::user::PROJECT_MANAGER_USER_ID;
    use crate::models::work_batch::{self, NewWorkBatch};
    use crate::test_support::{TEST_PERSON_ID, approved_batch, migrated_database};

    #[test]
    fn a_change_reads_as_the_tool_sends_it() {
        let read: ChangeArguments = serde_json::from_value(json!({
            "batchId": Uuid::nil(),
            "priority": "scheduled",
            "scheduledFor": "2026-12-01T09:00:00+01:00",
            "runNext": false,
        }))
        .expect("parses");
        assert_eq!(read.priority, Some(Priority::Scheduled));
        assert_eq!(read.run_next, Some(false));
        assert_eq!(read.paused, None);
        serde_json::from_value::<ChangeArguments>(json!({
            "batchId": Uuid::nil(),
            "scheduledFor": "2026-12-01T09:00:00",
        }))
        .expect_err("a time without its offset");
        serde_json::from_value::<ChangeArguments>(json!({
            "batchId": Uuid::nil(),
            "worker": "me",
        }))
        .expect_err("nothing else");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn the_queue_names_each_batchs_project_in_the_order_slots_go() {
        let (_url, mut connection) = migrated_database().await;
        let launch = project::create(
            &mut connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project")
        .id;
        let mut ids = Vec::new();
        for priority in [Priority::Normal, Priority::Asap] {
            let batch = approved_batch(
                &mut connection,
                NewWorkBatch {
                    id: Uuid::now_v7(),
                    project_id: launch,
                    title: format!("{priority:?} work"),
                    goal: "Do the work.".to_owned(),
                    repositories: json!([]),
                    priority,
                    scheduled_for: None,
                    turn_id: None,
                    source_record_ids: json!([]),
                    action_item_ids: json!([]),
                    created_by: PROJECT_MANAGER_USER_ID,
                },
            )
            .await;
            ids.push(batch.id);
        }
        // A proposal waits for a person: she sees it, apart from what the pool hands slots to.
        let waiting = work_batch::submit(
            &mut connection,
            NewWorkBatch {
                id: Uuid::now_v7(),
                project_id: launch,
                title: "Proposed work".to_owned(),
                goal: "Do the work.".to_owned(),
                repositories: json!([]),
                priority: Priority::Asap,
                scheduled_for: None,
                turn_id: None,
                source_record_ids: json!([]),
                action_item_ids: json!([]),
                created_by: PROJECT_MANAGER_USER_ID,
            },
        )
        .await
        .expect("submitted")
        .batch;

        let answer = read_queue(&mut connection, Utc::now())
            .await
            .expect("reads");
        assert_eq!(answer["next"][0]["batchId"], json!(ids[1]), "ASAP first");
        assert_eq!(answer["next"][1]["batchId"], json!(ids[0]));
        assert_eq!(answer["next"][0]["project"], "Launch");
        assert_eq!(answer["inSlots"], json!([]));
        assert_eq!(answer["proposed"][0]["batchId"], json!(waiting.id));
        assert_eq!(answer["proposed"][0]["state"], "proposed");
        assert_eq!(answer["next"].as_array().map(Vec::len), Some(2));
    }
}
