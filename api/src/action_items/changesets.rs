// Copyright © 2026 Jalapeno Labs

//! Changesets: the changes anyone but the user proposes, waiting for the user's approval.
//!
//! Elysia and a coding session's agent never write action items or initiatives directly.
//! They propose a changeset, a batch of [`Operation`]s each with its reason and, where there
//! is one, the quote and source it came from, and nothing is written until the user decides.
//! `docs/action-items.md` is the design; the queries that store and apply changesets are in
//! `crate::models::changeset`.
//!
//! This module holds the parts that are pure decisions:
//!
//! - [`Operation`], what one operation does, as proposers send it and the database stores it.
//! - [`validate`], the checks a proposal passes before it is stored.
//! - [`dependencies`]: an operation that acts on something an earlier one creates depends on
//!   it, and [`decide`] keeps the user's decisions consistent with that: rejecting an
//!   operation rejects everything that depends on it.
//! - [`restorable_fields`], which fields of an update an undo may put back.
//!
//! It also reads, before a changeset is applied, the things its link operations name
//! ([`read_link_targets`]), since a link is only made to what its credential can reach.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::action_items::Actor;
use crate::action_items::links::{LinkError, Links};
use crate::models::action_item::ActionItemPriority;
use crate::models::action_item_link::NewLink;
use crate::models::changeset::{ChangesetDecision, ChangesetOperation};
use crate::routes::v1::action_items::LinkTarget;

/// The most operations one changeset holds. A meeting's worth of follow-ups fits; a
/// proposer with more splits them, so each review stays readable.
pub const OPERATIONS_MAX: usize = 50;

/// The longest summary, reason, quote, and source, matching the database.
const SUMMARY_MAX_CHARACTERS: usize = 500;
const REASON_MAX_CHARACTERS: usize = 2000;
const QUOTE_MAX_CHARACTERS: usize = 2000;
const SOURCE_MAX_CHARACTERS: usize = 500;

/// The longest item title, item notes, initiative name and description, and comment,
/// matching their columns.
const TITLE_MAX_CHARACTERS: usize = 500;
const NOTES_MAX_CHARACTERS: usize = 20_000;
const NAME_MAX_CHARACTERS: usize = 200;
const DESCRIPTION_MAX_CHARACTERS: usize = 20_000;
const COMMENT_MAX_CHARACTERS: usize = 20_000;

/// The longest reference a link names, as the link route accepts.
const REFERENCE_MAX_CHARACTERS: usize = 300;

/// Who proposed a changeset. The user never proposes: the user acts directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proposer {
    Elysia,
    /// The agent of the coding session with this number.
    Session(i64),
}

impl Proposer {
    /// The actor that applied operations record, so each change names who proposed it.
    pub const fn actor(self) -> Actor {
        match self {
            Self::Elysia => Actor::Elysia,
            Self::Session(number) => Actor::Session(number),
        }
    }

    /// Reads a proposer in the form it is stored: `elysia` or `session:<number>`.
    pub fn parse(text: &str) -> Option<Self> {
        if text == "elysia" {
            return Some(Self::Elysia);
        }
        let number = text.strip_prefix("session:")?.parse().ok()?;
        Some(Self::Session(number))
    }
}

/// An existing record an operation names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Existing {
    pub id: Uuid,
}

/// A record an earlier operation of the same changeset creates, named by its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposed {
    pub operation: u32,
}

/// What an operation acts on: `{ "id": ... }` for a record that exists, or
/// `{ "operation": 2 }` for the item or initiative operation 2 creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Target {
    Existing(Existing),
    Proposed(Proposed),
}

impl Target {
    /// The position of the operation this names, when it names one.
    const fn proposed(self) -> Option<u32> {
        match self {
            Self::Existing(_) => None,
            Self::Proposed(Proposed { operation }) => Some(operation),
        }
    }
}

/// What a target must be: an operation naming an item cannot name an initiative's create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Item,
    Initiative,
}

/// One change a changeset proposes, as proposers send it and the database stores it: an
/// object whose `kind` says what it does, with that kind's fields beside it.
///
/// Items and initiatives it creates start the way the user's own do: an item `open` and
/// owned by the user, an initiative `active`. The user approving the operation is the user
/// accepting the item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Operation {
    CreateItem(CreateItem),
    UpdateItem(UpdateItem),
    ResolveItem(FinishItem),
    DismissItem(FinishItem),
    Comment(AddComment),
    Link(AddLink),
    AddToInitiative(Membership),
    RemoveFromInitiative(Membership),
    CreateInitiative(CreateInitiative),
}

/// Creates an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateItem {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "normal_priority")]
    pub priority: ActionItemPriority,
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub project_ids: Vec<Uuid>,
    /// Initiatives it joins, existing or created earlier in the changeset.
    #[serde(default)]
    pub initiatives: Vec<Target>,
}

/// Replaces the fields it names; an absent field stays as it is, and `dueAt: null` removes
/// the due date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateItem {
    pub item: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<ActionItemPriority>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and a date are three distinct requests"
    )]
    pub due_at: Option<Option<DateTime<Utc>>>,
}

/// Resolves or dismisses an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinishItem {
    pub item: Target,
}

/// Writes a comment on an item, which is posted to its primary link like the user's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddComment {
    pub item: Target,
    pub body: String,
}

/// Links an item to a thing a credential reaches, read through it when applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddLink {
    pub item: Target,
    pub target: LinkTarget,
}

/// Puts an item into an initiative, or takes it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Membership {
    pub item: Target,
    pub initiative: Target,
}

/// Creates an initiative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateInitiative {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub target_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub project_ids: Vec<Uuid>,
}

const fn normal_priority() -> ActionItemPriority {
    ActionItemPriority::Normal
}

impl Operation {
    /// Everything the operation names, with what each must be.
    pub fn targets(&self) -> Vec<(Target, Subject)> {
        match self {
            Self::CreateItem(create) => create
                .initiatives
                .iter()
                .map(|&initiative| (initiative, Subject::Initiative))
                .collect(),
            Self::UpdateItem(UpdateItem { item, .. })
            | Self::ResolveItem(FinishItem { item })
            | Self::DismissItem(FinishItem { item })
            | Self::Comment(AddComment { item, .. })
            | Self::Link(AddLink { item, .. }) => vec![(*item, Subject::Item)],
            Self::AddToInitiative(membership) | Self::RemoveFromInitiative(membership) => vec![
                (membership.item, Subject::Item),
                (membership.initiative, Subject::Initiative),
            ],
            Self::CreateInitiative(_) => Vec::new(),
        }
    }

    /// The projects a create puts what it creates in.
    pub const fn project_ids_mut(&mut self) -> Option<&mut Vec<Uuid>> {
        match self {
            Self::CreateItem(CreateItem { project_ids, .. })
            | Self::CreateInitiative(CreateInitiative { project_ids, .. }) => Some(project_ids),
            _ => None,
        }
    }

    /// What the operation creates, which later operations may name.
    const fn creates(&self) -> Option<Subject> {
        match self {
            Self::CreateItem(_) => Some(Subject::Item),
            Self::CreateInitiative(_) => Some(Subject::Initiative),
            _ => None,
        }
    }

    /// Why the operation's own fields cannot be stored, if they cannot.
    fn field_refusal(&self) -> Option<String> {
        match self {
            Self::CreateItem(create) => text_refusal("title", &create.title, TITLE_MAX_CHARACTERS)
                .or_else(|| length_refusal("notes", &create.notes, NOTES_MAX_CHARACTERS)),
            Self::UpdateItem(update) => update_refusal(update),
            Self::Comment(comment) => text_refusal("body", &comment.body, COMMENT_MAX_CHARACTERS),
            Self::Link(link) => text_refusal(
                "target.reference",
                &link.target.reference,
                REFERENCE_MAX_CHARACTERS,
            ),
            Self::CreateInitiative(create) => {
                text_refusal("name", &create.name, NAME_MAX_CHARACTERS).or_else(|| {
                    length_refusal(
                        "description",
                        &create.description,
                        DESCRIPTION_MAX_CHARACTERS,
                    )
                })
            }
            Self::ResolveItem(_)
            | Self::DismissItem(_)
            | Self::AddToInitiative(_)
            | Self::RemoveFromInitiative(_) => None,
        }
    }
}

/// Why an update cannot be stored: it changes nothing, or a field does not fit.
fn update_refusal(update: &UpdateItem) -> Option<String> {
    let changes_nothing = update.title.is_none()
        && update.notes.is_none()
        && update.priority.is_none()
        && update.due_at.is_none();
    if changes_nothing {
        return Some("an update-item changes at least one field".to_owned());
    }
    let title = update
        .title
        .as_ref()
        .and_then(|title| text_refusal("title", title, TITLE_MAX_CHARACTERS));
    title.or_else(|| {
        update
            .notes
            .as_ref()
            .and_then(|notes| length_refusal("notes", notes, NOTES_MAX_CHARACTERS))
    })
}

/// Why `value` is blank or longer than `max` characters, naming `field`.
fn text_refusal(field: &str, value: &str, max: usize) -> Option<String> {
    if value.trim().is_empty() {
        return Some(format!("{field} must not be blank"));
    }
    length_refusal(field, value, max)
}

/// Why `value` is longer than `max` characters, naming `field`.
fn length_refusal(field: &str, value: &str, max: usize) -> Option<String> {
    (value.chars().count() > max).then(|| format!("{field} must be {max} characters or fewer"))
}

/// One operation of a proposal, with why it is proposed.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedOperation {
    pub operation: Operation,
    pub reason: String,
    /// The words it came from, such as a line of a meeting's transcript.
    pub quote: Option<String>,
    /// Where those words are, such as the meeting and a time in it, or a URL.
    pub source: Option<String>,
}

/// A changeset as its proposer sends it.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub proposer: Proposer,
    /// The project a coding session proposed it in; `None` for Elysia.
    pub project_id: Option<Uuid>,
    pub summary: String,
    pub operations: Vec<ProposedOperation>,
}

/// Why a proposal cannot be stored, naming the operation that is wrong. `None` for one
/// that can.
///
/// Every operation's fields must fit their columns, and every operation it names must come
/// earlier and create the right kind of record: an item operation names an earlier
/// create-item, an initiative operation an earlier create-initiative. Whether the existing
/// records it names are there is the database's to answer.
pub fn validate(proposal: &Proposal) -> Option<String> {
    if let Some(refusal) = text_refusal("summary", &proposal.summary, SUMMARY_MAX_CHARACTERS) {
        return Some(refusal);
    }
    let count = proposal.operations.len();
    if count == 0 || count > OPERATIONS_MAX {
        return Some(format!(
            "a changeset holds 1 to {OPERATIONS_MAX} operations"
        ));
    }

    for (index, proposed) in proposal.operations.iter().enumerate() {
        let position = index + 1;
        let refusal = text_refusal("reason", &proposed.reason, REASON_MAX_CHARACTERS)
            .or_else(|| {
                proposed
                    .quote
                    .as_ref()
                    .and_then(|quote| text_refusal("quote", quote, QUOTE_MAX_CHARACTERS))
            })
            .or_else(|| {
                proposed
                    .source
                    .as_ref()
                    .and_then(|source| text_refusal("source", source, SOURCE_MAX_CHARACTERS))
            })
            .or_else(|| proposed.operation.field_refusal())
            .or_else(|| target_refusal(&proposal.operations, position, &proposed.operation));
        if let Some(refusal) = refusal {
            return Some(format!("operation {position}: {refusal}"));
        }
    }
    None
}

/// Why an operation at `position` names an operation it cannot.
fn target_refusal(
    operations: &[ProposedOperation],
    position: usize,
    operation: &Operation,
) -> Option<String> {
    for (target, subject) in operation.targets() {
        let Some(named) = target.proposed() else {
            continue;
        };
        let named = named as usize;
        if named == 0 || named >= position {
            return Some(format!(
                "it names operation {named}, but an operation can only name one before it"
            ));
        }
        if operations[named - 1].operation.creates() != Some(subject) {
            let wanted = match subject {
                Subject::Item => "create-item",
                Subject::Initiative => "create-initiative",
            };
            return Some(format!("operation {named} is not a {wanted}"));
        }
    }
    None
}

/// For each operation, in order, the positions of the operations it depends on: the earlier
/// ones whose records it acts on. Validation already refused forward and wrong names.
pub fn dependencies(operations: &[Operation]) -> Vec<Vec<u32>> {
    operations
        .iter()
        .map(|operation| {
            let named: BTreeSet<u32> = operation
                .targets()
                .into_iter()
                .filter_map(|(target, _subject)| target.proposed())
                .collect();
            named.into_iter().collect()
        })
        .collect()
}

/// Applies the user's `decision` to the operations at `chosen` positions, keeping every
/// decision consistent with `dependencies`.
///
/// Rejecting an operation rejects every operation that depends on it, directly or through
/// another. Approving one whose dependency stays rejected is refused: it could never apply.
/// Setting one back to pending leaves the rest as they are.
///
/// # Errors
/// Returns why the decision cannot be made; `decisions` is then unchanged.
pub fn decide(
    decisions: &mut [ChangesetDecision],
    dependencies: &[Vec<u32>],
    chosen: &BTreeSet<u32>,
    decision: ChangesetDecision,
) -> Result<(), String> {
    if decision == ChangesetDecision::Approved {
        for &position in chosen {
            let blocker = dependencies[position as usize - 1]
                .iter()
                .find(|&&dependency| {
                    decisions[dependency as usize - 1] == ChangesetDecision::Rejected
                        && !chosen.contains(&dependency)
                });
            if let Some(blocker) = blocker {
                return Err(format!(
                    "operation {position} depends on operation {blocker}, which is rejected; \
                     approve that first"
                ));
            }
        }
    }

    for &position in chosen {
        decisions[position as usize - 1] = decision;
    }
    // Dependencies always come earlier, so one pass in order carries a rejection to every
    // operation downstream of it.
    for (index, depends_on) in dependencies.iter().enumerate() {
        let blocked = depends_on
            .iter()
            .any(|&dependency| decisions[dependency as usize - 1] == ChangesetDecision::Rejected);
        if blocked {
            decisions[index] = ChangesetDecision::Rejected;
        }
    }
    Ok(())
}

/// Which fields an undo of an update puts back, and which it leaves.
#[derive(Debug, Default, PartialEq)]
pub struct Restorable {
    /// Each field still holding what the update wrote, with the value it replaced.
    pub restore: Map<String, Value>,
    /// Fields changed again since the update, which an undo leaves as they are now.
    pub kept: Vec<String>,
}

/// Sorts an update's `changes` (each field's `{ from, to }`, as its history entry records
/// them) by whether `current` still holds the value the update wrote. Undo restores only
/// those, so a change the user made after the changeset is never overwritten.
pub fn restorable_fields(changes: &Map<String, Value>, current: &Map<String, Value>) -> Restorable {
    let mut restorable = Restorable::default();
    for (field, change) in changes {
        if current.get(field) == change.get("to") {
            let from = change.get("from").cloned().unwrap_or(Value::Null);
            restorable.restore.insert(field.clone(), from);
        } else {
            restorable.kept.push(field.clone());
        }
    }
    restorable
}

/// What each link operation's target is, read through its credential: the link to make, or
/// the provider's answer when it cannot be read. Keyed by the operation's id.
pub type LinkReads = HashMap<Uuid, Result<NewLink, String>>;

/// Reads the target of every approved link operation, before the changeset is applied, the
/// way a link the user adds is read before it is stored. A target that cannot be read makes
/// its operation fail with the provider's answer; the rest still apply.
pub async fn read_link_targets(links: &Links, operations: &[ChangesetOperation]) -> LinkReads {
    let mut reads = LinkReads::new();
    for row in operations {
        if row.decision != ChangesetDecision::Approved {
            continue;
        }
        let Operation::Link(AddLink { target, .. }) = row.parsed() else {
            continue;
        };
        let found = links
            .provider(target.provider)
            .find(target.credential_id, target.kind, &target.reference)
            .await;
        let read = match found {
            Ok(remote) => Ok(NewLink {
                credential: target.credential(),
                kind: target.kind,
                external_id: remote.external_id.clone(),
                observation: remote.observation(),
            }),
            Err(LinkError::Internal(error)) => {
                tracing::event!(
                    name: "changeset.link.read.failed",
                    tracing::Level::ERROR,
                    changeset.operation.id = %row.id,
                    error.message = %error,
                    "reading a link operation's target failed inside Elysium",
                );
                Err("Elysium could not read it; the API's log has the details".to_owned())
            }
            Err(refused) => Err(refused.to_string()),
        };
        reads.insert(row.id, read);
    }
    reads
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::action_item_link::{LinkKind, LinkProvider};

    fn existing() -> Target {
        Target::Existing(Existing { id: Uuid::nil() })
    }

    fn proposed(operation: u32) -> Target {
        Target::Proposed(Proposed { operation })
    }

    fn new_item(title: &str) -> CreateItem {
        CreateItem {
            title: title.to_owned(),
            notes: String::new(),
            priority: ActionItemPriority::Normal,
            due_at: None,
            project_ids: Vec::new(),
            initiatives: Vec::new(),
        }
    }

    fn create_item(title: &str) -> Operation {
        Operation::CreateItem(new_item(title))
    }

    fn create_initiative(name: &str) -> Operation {
        Operation::CreateInitiative(CreateInitiative {
            name: name.to_owned(),
            description: String::new(),
            target_at: None,
            project_ids: Vec::new(),
        })
    }

    fn resolve(item: Target) -> Operation {
        Operation::ResolveItem(FinishItem { item })
    }

    fn proposal(operations: Vec<Operation>) -> Proposal {
        Proposal {
            proposer: Proposer::Elysia,
            project_id: None,
            summary: "Follow-ups from the planning meeting".to_owned(),
            operations: operations
                .into_iter()
                .map(|operation| ProposedOperation {
                    operation,
                    reason: "Sam asked for it".to_owned(),
                    quote: None,
                    source: None,
                })
                .collect(),
        }
    }

    #[test]
    fn operations_read_and_write_the_json_proposers_send() {
        let sent = json!([
            { "kind": "create-item", "title": "Draft the launch post" },
            {
                "kind": "update-item",
                "item": { "id": Uuid::nil() },
                "priority": "high",
                "dueAt": null,
            },
            { "kind": "comment", "item": { "operation": 1 }, "body": "From the meeting." },
            {
                "kind": "link",
                "item": { "operation": 1 },
                "target": {
                    "provider": "github",
                    "credentialId": Uuid::nil(),
                    "kind": "pull-request",
                    "reference": "JalapenoLabs/Elysium#12",
                },
            },
            {
                "kind": "add-to-initiative",
                "item": { "operation": 1 },
                "initiative": { "operation": 6 },
            },
            { "kind": "create-initiative", "name": "Launch" },
        ]);
        let operations: Vec<Operation> = serde_json::from_value(sent).expect("parses");

        assert_eq!(
            operations[1],
            Operation::UpdateItem(UpdateItem {
                item: existing(),
                title: None,
                notes: None,
                priority: Some(ActionItemPriority::High),
                due_at: Some(None),
            })
        );
        assert_eq!(
            serde_json::to_value(&operations[1]).expect("serializes"),
            json!({
                "kind": "update-item",
                "item": { "id": Uuid::nil() },
                "priority": "high",
                "dueAt": null,
            }),
            "only the fields an update names are stored"
        );
        let Operation::Link(link) = &operations[3] else {
            panic!("a link");
        };
        assert_eq!(
            (link.target.provider, link.target.kind),
            (LinkProvider::Github, LinkKind::PullRequest)
        );
        for operation in &operations {
            let stored = serde_json::to_value(operation).expect("serializes");
            let read: Operation = serde_json::from_value(stored).expect("reads back");
            assert_eq!(&read, operation, "what is stored reads back the same");
        }
    }

    #[test]
    fn operations_refuse_unknown_kinds_fields_and_target_shapes() {
        let item = json!({ "id": Uuid::nil() });
        serde_json::from_value::<Operation>(json!({ "kind": "delete-item", "item": item }))
            .expect_err("deleting is not an operation");
        serde_json::from_value::<Operation>(
            json!({ "kind": "resolve-item", "item": item, "state": "open" }),
        )
        .expect_err("a field the kind does not have");
        serde_json::from_value::<Operation>(
            json!({ "kind": "resolve-item", "item": { "id": Uuid::nil(), "operation": 1 } }),
        )
        .expect_err("a target is one or the other");
        serde_json::from_value::<Operation>(json!({ "kind": "resolve-item", "item": "ELY-12" }))
            .expect_err("a target is an object");
    }

    #[test]
    fn proposers_are_elysia_or_a_session_and_act_as_themselves() {
        assert_eq!(Proposer::parse("elysia"), Some(Proposer::Elysia));
        assert_eq!(Proposer::parse("session:12"), Some(Proposer::Session(12)));
        assert_eq!(Proposer::parse("user"), None, "the user acts directly");
        assert_eq!(Proposer::parse("session:twelve"), None);
        assert_eq!(Proposer::Session(12).actor(), Actor::Session(12));
        assert_eq!(Proposer::Elysia.actor().to_string(), "elysia");
    }

    #[test]
    fn a_valid_proposal_names_only_earlier_creates_of_the_right_kind() {
        let valid = proposal(vec![
            create_initiative("Launch"),
            Operation::CreateItem(CreateItem {
                initiatives: vec![proposed(1)],
                ..new_item("Draft the post")
            }),
            Operation::Comment(AddComment {
                item: proposed(2),
                body: "Sam will review it.".to_owned(),
            }),
            resolve(existing()),
        ]);
        assert_eq!(validate(&valid), None);

        let forward = proposal(vec![resolve(proposed(2)), create_item("Later")]);
        assert_eq!(
            validate(&forward).as_deref(),
            Some("operation 1: it names operation 2, but an operation can only name one before it")
        );

        let itself = proposal(vec![resolve(proposed(1))]);
        assert!(
            validate(&itself).is_some(),
            "an operation cannot name itself"
        );

        let wrong_kind = proposal(vec![
            create_item("An item"),
            Operation::AddToInitiative(Membership {
                item: existing(),
                initiative: proposed(1),
            }),
        ]);
        assert_eq!(
            validate(&wrong_kind).as_deref(),
            Some("operation 2: operation 1 is not a create-initiative")
        );
    }

    #[test]
    fn proposals_refuse_blank_or_oversized_text_and_empty_updates() {
        let mut blank_reason = proposal(vec![create_item("Fine")]);
        blank_reason.operations[0].reason = "  ".to_owned();
        assert_eq!(
            validate(&blank_reason).as_deref(),
            Some("operation 1: reason must not be blank")
        );

        let empty_update = proposal(vec![Operation::UpdateItem(UpdateItem {
            item: existing(),
            title: None,
            notes: None,
            priority: None,
            due_at: None,
        })]);
        assert_eq!(
            validate(&empty_update).as_deref(),
            Some("operation 1: an update-item changes at least one field")
        );

        let long_title = proposal(vec![create_item(&"x".repeat(TITLE_MAX_CHARACTERS + 1))]);
        assert_eq!(
            validate(&long_title).as_deref(),
            Some("operation 1: title must be 500 characters or fewer")
        );

        assert!(
            validate(&proposal(Vec::new())).is_some(),
            "nothing to review"
        );
        let too_many = proposal(vec![create_item("One"); OPERATIONS_MAX + 1]);
        assert!(validate(&too_many).is_some());

        let mut no_summary = proposal(vec![create_item("Fine")]);
        no_summary.summary = String::new();
        assert_eq!(
            validate(&no_summary).as_deref(),
            Some("summary must not be blank")
        );
    }

    #[test]
    fn operations_depend_on_the_creates_they_name() {
        let operations = vec![
            create_item("An item"),
            create_initiative("Launch"),
            Operation::AddToInitiative(Membership {
                item: proposed(1),
                initiative: proposed(2),
            }),
            resolve(existing()),
        ];
        assert_eq!(
            dependencies(&operations),
            [vec![], vec![], vec![1, 2], vec![]]
        );
    }

    #[test]
    fn rejecting_an_operation_rejects_everything_downstream_of_it() {
        use ChangesetDecision::{Approved, Pending, Rejected};

        // 1 creates an item, 2 comments on it, 3 links it, and 4 depends on nothing.
        let dependencies = [vec![], vec![1], vec![1], vec![]];
        let mut decisions = [Approved, Approved, Pending, Pending];
        decide(
            &mut decisions,
            &dependencies,
            &BTreeSet::from([1]),
            Rejected,
        )
        .expect("rejecting always works");
        assert_eq!(decisions, [Rejected, Rejected, Rejected, Pending]);

        let refused = decide(
            &mut decisions,
            &dependencies,
            &BTreeSet::from([2]),
            Approved,
        );
        assert_eq!(
            refused,
            Err(
                "operation 2 depends on operation 1, which is rejected; approve that first"
                    .to_owned()
            )
        );
        assert_eq!(
            decisions,
            [Rejected, Rejected, Rejected, Pending],
            "unchanged"
        );

        decide(
            &mut decisions,
            &dependencies,
            &BTreeSet::from([1, 2, 3, 4]),
            Approved,
        )
        .expect("approving a dependency with its dependents works");
        assert_eq!(decisions, [Approved; 4]);
    }

    #[test]
    fn rejection_carries_through_chains() {
        use ChangesetDecision::{Approved, Rejected};

        // 1 creates an initiative, 2 an item in it, and 3 comments on that item.
        let dependencies = [vec![], vec![1], vec![2]];
        let mut decisions = [Approved; 3];
        decide(
            &mut decisions,
            &dependencies,
            &BTreeSet::from([1]),
            Rejected,
        )
        .expect("rejects");
        assert_eq!(decisions, [Rejected; 3]);
    }

    #[test]
    fn undo_restores_only_fields_that_still_hold_what_the_update_wrote() {
        let changes = json!({
            "title": { "from": "Reply to Sam", "to": "Reply to Sam about logs" },
            "priority": { "from": "normal", "to": "high" },
            "dueAt": { "from": null, "to": "2026-09-20T23:59:59.999Z" },
        });
        let current = json!({
            "title": "Reply to Sam about logs",
            "priority": "urgent",
            "dueAt": "2026-09-20T23:59:59.999Z",
            "notes": "",
        });
        let restorable = restorable_fields(
            changes.as_object().expect("an object"),
            current.as_object().expect("an object"),
        );
        assert_eq!(
            Value::Object(restorable.restore),
            json!({ "title": "Reply to Sam", "dueAt": null })
        );
        assert_eq!(restorable.kept, ["priority"], "the user changed it since");
    }
}
