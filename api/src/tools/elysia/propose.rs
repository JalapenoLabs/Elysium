// Copyright © 2026 Jalapeno Labs

//! `elysia_propose_changes`: Elysia proposes changes to action items and initiatives as a
//! changeset, which waits for a person's review.
//!
//! Nothing she proposes is written until a person approves it. She acts for the whole
//! workspace, so a proposal may name any live item, initiative, or project, which the
//! changeset model checks before anything is stored; what she creates joins the projects she
//! names. Linking provider issues and pull requests stays the person's to do.

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use serde::Deserialize;
use serde_json::Value;

use crate::action_items::WorkError;
use crate::action_items::changesets::{
    OPERATIONS_MAX, Operation, Proposal, ProposedOperation, Proposer,
};
use crate::models::changeset::{self, Staged};
use crate::routes::v1::changesets::publish_changeset;
use crate::tools::work::propose::{Proposing, schema_for, staged_answer};
use crate::tools::{CallScope, ToolContext, ToolError, internal, parse_arguments};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProposeArguments {
    summary: String,
    operations: Vec<ProposedChange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProposedChange {
    change: Value,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
}

/// The tool's arguments as a JSON Schema.
pub(super) fn schema() -> Value {
    schema_for(Proposing::Elysia)
}

/// Reads one change, refusing what Elysia may not propose.
fn parse_change(change: Value) -> Result<Operation, String> {
    let operation: Operation = serde_json::from_value(change).map_err(|error| error.to_string())?;
    if matches!(operation, Operation::Link(_)) {
        return Err(
            "links to provider issues and pull requests are the person's to add".to_owned(),
        );
    }
    Ok(operation)
}

/// Parses and stages her proposal, then tells every client about it.
pub(super) async fn run(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let arguments: ProposeArguments = parse_arguments(arguments)?;
    if arguments.operations.len() > OPERATIONS_MAX {
        return Err(ToolError::Invalid(format!(
            "propose at most {OPERATIONS_MAX} operations at once; split the rest into another \
             proposal"
        )));
    }
    let mut operations = Vec::with_capacity(arguments.operations.len());
    for (proposed, position) in arguments.operations.into_iter().zip(1..) {
        let operation = parse_change(proposed.change)
            .map_err(|refusal| ToolError::Invalid(format!("operation {position}: {refusal}")))?;
        operations.push(ProposedOperation {
            operation,
            reason: proposed.reason,
            quote: proposed.quote,
            source: proposed.source,
        });
    }

    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let staged = stage(&mut connection, arguments.summary, operations)
        .await
        .map_err(|error| match error {
            WorkError::Refused(refusal) => ToolError::Invalid(refusal),
            error => internal(scope, "elysia.propose_changes", &error),
        })?;
    let answer = staged_answer(&staged);
    publish_changeset(&context.events, staged);
    Ok(answer)
}

/// Stores the changeset as hers. The model refuses one that names a missing or deleted item,
/// initiative, or project.
async fn stage(
    connection: &mut AsyncPgConnection,
    summary: String,
    operations: Vec<ProposedOperation>,
) -> Result<Staged, WorkError> {
    changeset::propose(
        connection,
        Proposal {
            proposer: Proposer::Elysia,
            project_id: None,
            summary,
            operations,
        },
        Utc::now(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::test_support::migrated_database;

    #[test]
    fn she_may_not_propose_provider_links() {
        let link = json!({
            "kind": "link",
            "item": { "id": Uuid::nil() },
            "target": {
                "provider": "github",
                "credentialId": Uuid::nil(),
                "kind": "pull-request",
                "reference": "owner/name#1",
            },
        });
        parse_change(link).expect_err("a provider link is refused");
    }

    #[test]
    fn her_schema_offers_projects_and_no_pull_requests() {
        let text = schema().to_string();
        assert!(text.contains("projectIds"));
        assert!(!text.contains("link-pull-request"));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn a_proposal_is_staged_as_hers() {
        let (_url, mut connection) = migrated_database().await;
        let operation = parse_change(json!({ "kind": "create-item", "title": "Send the deck" }))
            .expect("a create-item parses");

        let staged = stage(
            &mut connection,
            "Track the deck".to_owned(),
            vec![ProposedOperation {
                operation,
                reason: "Alex asked for it in standup.".to_owned(),
                quote: None,
                source: None,
            }],
        )
        .await
        .expect("the proposal is staged");
        assert_eq!(staged.changeset.proposer, "elysia");
        assert_eq!(staged.operations.len(), 1);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn a_proposal_naming_a_missing_item_is_refused() {
        let (_url, mut connection) = migrated_database().await;
        let operation =
            parse_change(json!({ "kind": "resolve-item", "item": { "id": Uuid::now_v7() } }))
                .expect("a resolve-item parses");

        let refused = stage(
            &mut connection,
            "Close it".to_owned(),
            vec![ProposedOperation {
                operation,
                reason: "Done.".to_owned(),
                quote: None,
                source: None,
            }],
        )
        .await;
        assert!(matches!(refused, Err(WorkError::Refused(_))));
    }
}
