// Copyright © 2026 Jalapeno Labs

//! What timeline records are attributed to, and the history of every change to it. See
//! `docs/connectors.md`, The timeline.
//!
//! A record is attributed to a project with a confidence and who attributed it
//! (`timeline_record_projects`). Elysia attributes as segmenting reads a meeting or a
//! conversation; a person confirms, adds, or removes; a Project Manager attributes what it puts
//! on its own project's timeline, and a session's agent its turn summaries to the session's
//! project. Every change goes through this module, which records it in
//! `timeline_record_events` in the same transaction, with what the attribution was before and
//! after, so any change can be shown and undone.
//!
//! # A new attribution wakes the project's Project Manager
//!
//! A record newly on a project, whoever put it there and however (segmenting a meeting or a
//! conversation, a person adding it, an undo putting it back), is news for that project's
//! Project Manager, so the change wakes it in the same transaction
//! (`crate::models::project_manager::wake_for_record`). A change of how sure an attribution is
//! wakes nothing, and neither does what Elysium's own agents put on the timeline: a Project
//! Manager's records are its own, and a session's turn summaries are work the Project Manager
//! hears of through what it dispatched (a batch's result) or reads on its timeline when something
//! else wakes it. A wake per turn would spend a Project Manager's turn on every prompt anyone
//! types into a session of the project.
//!
//! # A person's word stands
//!
//! A person's attribution is certain, so it carries a confidence of 1 and replaces whatever
//! Elysia guessed for that project. Elysia never overrides a person: she does not change or
//! remove a person's attribution, and she does not attribute a record to a project a person
//! already decided on for it, either way. So a segment a person took off a project stays off it
//! however often segmenting reads its meeting or conversation again.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::database::schema::{timeline_record_events, timeline_record_projects};
use crate::models::project_manager;
use crate::models::timeline_record::Attribution;
use crate::models::user::{CODING_AGENT_USER_ID, ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID};

/// How sure a person's own attribution is.
pub const PERSON_CONFIDENCE: f32 = 1.0;

/// How Elysia is named in `attributed_by`, the actor form history uses.
pub const ELYSIA_ACTOR: &str = "elysia";

/// How a Project Manager is named in `attributed_by`. Its records are on its own project alone,
/// so the project names which one.
pub const PROJECT_MANAGER_ACTOR: &str = "project_manager";

/// How a session's agent is named in `attributed_by`, before the session's number, as history
/// names it everywhere (`session:<n>`).
pub const SESSION_ACTOR_PREFIX: &str = "session:";

/// Who changes an attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attributor {
    /// Elysia, as segmenting reads a meeting or a conversation.
    Elysia,
    /// A person, the record's owner.
    Person(Uuid),
    /// A Project Manager, putting its own turns and decisions on its project's timeline.
    ProjectManager,
    /// The agent of coding session `n`, putting its turn summaries on the session's project.
    Session(i64),
}

impl Attributor {
    /// The actor `attributed_by` names: `elysia`, `user:<id>`, `project_manager`, or
    /// `session:<n>`.
    fn actor(self) -> String {
        match self {
            Self::Elysia => ELYSIA_ACTOR.to_owned(),
            Self::Person(id) => format!("user:{id}"),
            Self::ProjectManager => PROJECT_MANAGER_ACTOR.to_owned(),
            Self::Session(id) => format!("{SESSION_ACTOR_PREFIX}{id}"),
        }
    }

    /// The user the history entry is created by.
    fn user_id(self) -> Uuid {
        match self {
            Self::Elysia => ELYSIA_USER_ID,
            Self::Person(id) => id,
            Self::ProjectManager => PROJECT_MANAGER_USER_ID,
            Self::Session(_) => CODING_AGENT_USER_ID,
        }
    }

    /// Whether a record this puts newly on a project wakes the project's Project Manager: what
    /// Elysia and people attribute does, what Elysium's own agents put on the timeline does not
    /// (the module's docs).
    const fn wakes_project_manager(self) -> bool {
        match self {
            Self::Elysia | Self::Person(_) => true,
            Self::ProjectManager | Self::Session(_) => false,
        }
    }
}

/// What an attribution held at one moment, as history records it before and after a change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Held {
    pub confidence: f32,
    pub attributed_by: String,
}

impl From<&Attribution> for Held {
    fn from(attribution: &Attribution) -> Self {
        Self {
            confidence: attribution.confidence,
            attributed_by: attribution.attributed_by.clone(),
        }
    }
}

/// The kinds of change history records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// A project added to the record, or how sure its attribution is changed.
    Attributed,
    /// A project taken off the record.
    Unattributed,
}

impl HistoryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attributed => "attributed",
            Self::Unattributed => "unattributed",
        }
    }
}

/// One change to a record's attributions.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = timeline_record_events, check_for_backend(diesel::pg::Pg))]
pub struct HistoryEntry {
    pub id: Uuid,
    pub timeline_record_id: Uuid,
    pub project_id: Uuid,
    /// A [`HistoryKind`] name. Kept as text so entries written by later versions still load.
    pub kind: String,
    /// Who made the change: a person, or Elysia's machine user.
    pub created_by: Uuid,
    /// `{ before, after }`, each a [`Held`] or `null`, and `undid` for an undo.
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

impl HistoryEntry {
    /// What the attribution held before this change, `None` when there was none.
    fn before(&self) -> Option<Held> {
        serde_json::from_value(self.data["before"].clone()).ok()
    }

    /// What the attribution held after this change, `None` when it was removed.
    fn after(&self) -> Option<Held> {
        serde_json::from_value(self.data["after"].clone()).ok()
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = timeline_record_events)]
struct NewHistoryEntry {
    id: Uuid,
    timeline_record_id: Uuid,
    project_id: Uuid,
    kind: String,
    created_by: Uuid,
    data: Value,
    created_at: DateTime<Utc>,
}

/// How a write to an attribution went.
#[derive(Debug, Clone)]
pub enum Written {
    /// It changed, and history records it.
    Changed(HistoryEntry),
    /// It already held what was asked, so nothing was written.
    Unchanged,
    /// Elysia's write was left out: a person decided this record's attribution to this project.
    PersonDecided,
}

impl Written {
    /// The history entry the write recorded, when it changed anything.
    pub fn entry(self) -> Option<HistoryEntry> {
        match self {
            Self::Changed(entry) => Some(entry),
            Self::Unchanged | Self::PersonDecided => None,
        }
    }
}

/// Attributes record `record_id` to project `project_id` as `by`, or changes how sure the
/// attribution is, and records the change. A person's attribution has a confidence of 1, whatever
/// `confidence` says; Elysia's is left out when a person decided the pair (the module's docs).
///
/// # Errors
/// Propagates any database error, including a foreign key violation for an unknown record or
/// project.
pub async fn attribute(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    project_id: Uuid,
    confidence: f32,
    by: Attributor,
) -> QueryResult<Written> {
    let confidence = match by {
        Attributor::Person(_) => PERSON_CONFIDENCE,
        Attributor::Elysia | Attributor::ProjectManager | Attributor::Session(_) => confidence,
    };
    let after = Held {
        confidence,
        attributed_by: by.actor(),
    };
    // A savepoint when the caller's transaction is already open, as the step's is.
    connection
        .transaction(async |connection| {
            let before = locked(connection, record_id, project_id).await?;
            if by == Attributor::Elysia {
                // A person's attribution from before history was kept has no entry of its own.
                let persons = before
                    .as_ref()
                    .is_some_and(|held| held.attributed_by != ELYSIA_ACTOR);
                if persons || person_decided(connection, record_id, project_id).await? {
                    return Ok(Written::PersonDecided);
                }
            }
            if before.as_ref() == Some(&after) {
                return Ok(Written::Unchanged);
            }
            write(connection, record_id, project_id, &after).await?;
            if before.is_none() && by.wakes_project_manager() {
                project_manager::wake_for_record(connection, project_id, record_id, by.user_id())
                    .await?;
            }
            let entry = record(
                connection,
                Change {
                    record_id,
                    project_id,
                    kind: HistoryKind::Attributed,
                    by,
                    before,
                    after: Some(after),
                    undid: None,
                },
            )
            .await?;
            Ok(Written::Changed(entry))
        })
        .await
}

/// Takes record `record_id` off project `project_id` as `by`, and records it. Elysia only ever
/// removes her own attributions, and none a person decided on; a person's stays.
///
/// # Errors
/// Propagates any database error.
pub async fn unattribute(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    project_id: Uuid,
    by: Attributor,
) -> QueryResult<Written> {
    connection
        .transaction(async |connection| {
            let Some(before) = locked(connection, record_id, project_id).await? else {
                return Ok(Written::Unchanged);
            };
            if by == Attributor::Elysia {
                // An attribution of hers a person put back, by undoing its removal, is theirs to keep.
                let persons = before.attributed_by != ELYSIA_ACTOR;
                if persons || person_decided(connection, record_id, project_id).await? {
                    return Ok(Written::PersonDecided);
                }
            }
            remove(connection, record_id, project_id).await?;
            let entry = record(
                connection,
                Change {
                    record_id,
                    project_id,
                    kind: HistoryKind::Unattributed,
                    by,
                    before: Some(before),
                    after: None,
                    undid: None,
                },
            )
            .await?;
            Ok(Written::Changed(entry))
        })
        .await
}

/// Why a change could not be undone.
#[derive(Debug, thiserror::Error)]
pub enum UndoError {
    /// The entry is not one of the record's.
    #[error("not found")]
    NotFound,
    /// The attribution changed since the entry, so undoing it would overwrite the later change.
    #[error("the attribution changed since")]
    ChangedSince,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Undoes history entry `entry_id` of record `record_id` as person `person`: the attribution
/// gets back what the entry replaced, and the undo is an entry of its own naming the one it
/// reversed (`undid`), so it can be undone in turn. Only an attribution still holding what the
/// entry wrote is undone, so a later change is never overwritten.
///
/// # Errors
/// [`UndoError::NotFound`] when the entry is not the record's, [`UndoError::ChangedSince`] when
/// the attribution moved on since; any database error.
pub async fn undo(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    entry_id: Uuid,
    person: Uuid,
) -> Result<HistoryEntry, UndoError> {
    connection
        .transaction(async |connection| {
            let undone: HistoryEntry = timeline_record_events::table
                .find(entry_id)
                .filter(timeline_record_events::timeline_record_id.eq(record_id))
                .select(HistoryEntry::as_select())
                .first(connection)
                .await
                .optional()?
                .ok_or(UndoError::NotFound)?;
            let project_id = undone.project_id;
            let current = locked(connection, record_id, project_id).await?;
            if current != undone.after() {
                return Err(UndoError::ChangedSince);
            }
            let restored = undone.before();
            match &restored {
                Some(held) => {
                    write(connection, record_id, project_id, held).await?;
                    if current.is_none() {
                        project_manager::wake_for_record(connection, project_id, record_id, person)
                            .await?;
                    }
                }
                None => remove(connection, record_id, project_id).await?,
            }
            let kind = match restored {
                Some(_) => HistoryKind::Attributed,
                None => HistoryKind::Unattributed,
            };
            Ok(record(
                connection,
                Change {
                    record_id,
                    project_id,
                    kind,
                    by: Attributor::Person(person),
                    before: current,
                    after: restored,
                    undid: Some(undone.id),
                },
            )
            .await?)
        })
        .await
}

/// Record `record_id`'s history, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn history(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
) -> QueryResult<Vec<HistoryEntry>> {
    timeline_record_events::table
        .filter(timeline_record_events::timeline_record_id.eq(record_id))
        .order(timeline_record_events::id.asc())
        .select(HistoryEntry::as_select())
        .load(connection)
        .await
}

/// What `record_id`'s attribution to `project_id` holds, locked until the transaction ends so
/// two writers decide one after the other.
async fn locked(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    project_id: Uuid,
) -> QueryResult<Option<Held>> {
    let attribution: Option<Attribution> = timeline_record_projects::table
        .filter(timeline_record_projects::timeline_record_id.eq(record_id))
        .filter(timeline_record_projects::project_id.eq(project_id))
        .for_update()
        .select(Attribution::as_select())
        .first(connection)
        .await
        .optional()?;
    Ok(attribution.as_ref().map(Held::from))
}

/// Whether a person ever changed `record_id`'s attribution to `project_id`, which Elysia's writes
/// then leave alone.
async fn person_decided(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    project_id: Uuid,
) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        timeline_record_events::table
            .filter(timeline_record_events::timeline_record_id.eq(record_id))
            .filter(timeline_record_events::project_id.eq(project_id))
            // Only a person decides; Elysia and the Project Managers are machines.
            .filter(
                timeline_record_events::created_by
                    .ne_all([ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID]),
            ),
    ))
    .get_result(connection)
    .await
}

async fn write(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    project_id: Uuid,
    held: &Held,
) -> QueryResult<()> {
    diesel::insert_into(timeline_record_projects::table)
        .values((
            timeline_record_projects::timeline_record_id.eq(record_id),
            timeline_record_projects::project_id.eq(project_id),
            timeline_record_projects::confidence.eq(held.confidence),
            timeline_record_projects::attributed_by.eq(&held.attributed_by),
        ))
        .on_conflict((
            timeline_record_projects::timeline_record_id,
            timeline_record_projects::project_id,
        ))
        .do_update()
        .set((
            timeline_record_projects::confidence.eq(excluded(timeline_record_projects::confidence)),
            timeline_record_projects::attributed_by
                .eq(excluded(timeline_record_projects::attributed_by)),
        ))
        .execute(connection)
        .await
        .map(drop)
}

async fn remove(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    project_id: Uuid,
) -> QueryResult<()> {
    diesel::delete(
        timeline_record_projects::table
            .filter(timeline_record_projects::timeline_record_id.eq(record_id))
            .filter(timeline_record_projects::project_id.eq(project_id)),
    )
    .execute(connection)
    .await
    .map(drop)
}

/// One change, as history records it.
struct Change {
    record_id: Uuid,
    project_id: Uuid,
    kind: HistoryKind,
    by: Attributor,
    before: Option<Held>,
    after: Option<Held>,
    undid: Option<Uuid>,
}

async fn record(connection: &mut AsyncPgConnection, change: Change) -> QueryResult<HistoryEntry> {
    let mut data = json!({ "before": change.before, "after": change.after });
    if let Some(undid) = change.undid {
        data["undid"] = json!(undid);
    }
    diesel::insert_into(timeline_record_events::table)
        .values(NewHistoryEntry {
            id: Uuid::now_v7(),
            timeline_record_id: change.record_id,
            project_id: change.project_id,
            kind: change.kind.as_str().to_owned(),
            created_by: change.by.user_id(),
            data,
            created_at: Utc::now(),
        })
        .returning(HistoryEntry::as_returning())
        .get_result(connection)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::project::{self, NewProject};
    use crate::models::timeline_record::{self, NewTimelineRecord};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    /// A record of the test person's, and a project to attribute it to.
    async fn record_and_project(connection: &mut AsyncPgConnection) -> (Uuid, Uuid) {
        let record = timeline_record::insert_new(
            connection,
            NewTimelineRecord {
                id: Uuid::now_v7(),
                source_kind: "meeting".to_owned(),
                connector_id: None,
                external_id: None,
                occurred_at: Utc::now(),
                actor: "Ada".to_owned(),
                title: "Launch".to_owned(),
                body: String::new(),
                url: None,
                media: None,
                metadata: json!({}),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("stored")
        .expect("new");
        let project = project::create(
            connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project");
        (record.id, project.id)
    }

    async fn held(connection: &mut AsyncPgConnection, record_id: Uuid) -> Vec<Held> {
        timeline_record::attributions_of(connection, &[record_id])
            .await
            .expect("reads")
            .iter()
            .map(Held::from)
            .collect()
    }

    fn elysias(confidence: f32) -> Held {
        Held {
            confidence,
            attributed_by: ELYSIA_ACTOR.to_owned(),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_persons_correction_stands_and_every_change_can_be_undone() {
        let (_url, mut connection) = migrated_database().await;
        let (record, project) = record_and_project(&mut connection).await;
        let person = Attributor::Person(TEST_PERSON_ID);

        // Elysia's guess, recorded once however often segmenting writes it.
        let guessed = attribute(&mut connection, record, project, 0.7, Attributor::Elysia)
            .await
            .expect("attributes")
            .entry()
            .expect("a change");
        assert!(matches!(
            attribute(&mut connection, record, project, 0.7, Attributor::Elysia)
                .await
                .expect("attributes"),
            Written::Unchanged
        ));

        // The person takes the record off the project, and Elysia never puts it back.
        let removed = unattribute(&mut connection, record, project, person)
            .await
            .expect("removes")
            .entry()
            .expect("a change");
        assert!(matches!(
            attribute(&mut connection, record, project, 0.9, Attributor::Elysia)
                .await
                .expect("runs"),
            Written::PersonDecided
        ));
        assert!(held(&mut connection, record).await.is_empty());

        // Undoing the removal puts back Elysia's guess as it was.
        let restored = undo(&mut connection, record, removed.id, TEST_PERSON_ID)
            .await
            .expect("undone");
        assert_eq!(restored.data["undid"], json!(removed.id));
        assert_eq!(held(&mut connection, record).await, vec![elysias(0.7)]);
        // Put back by the person, it is theirs to keep: a later reading that drops it leaves it.
        assert!(matches!(
            unattribute(&mut connection, record, project, Attributor::Elysia)
                .await
                .expect("runs"),
            Written::PersonDecided
        ));
        assert_eq!(held(&mut connection, record).await, vec![elysias(0.7)]);
        // The removal was undone since, so undoing it again would overwrite that.
        assert!(matches!(
            undo(&mut connection, record, removed.id, TEST_PERSON_ID).await,
            Err(UndoError::ChangedSince)
        ));

        // Confirming makes it the person's, certain, and Elysia leaves it alone.
        attribute(&mut connection, record, project, 0.2, person)
            .await
            .expect("confirms");
        let persons = Held {
            confidence: PERSON_CONFIDENCE,
            attributed_by: format!("user:{TEST_PERSON_ID}"),
        };
        assert_eq!(held(&mut connection, record).await, vec![persons.clone()]);
        assert!(matches!(
            unattribute(&mut connection, record, project, Attributor::Elysia)
                .await
                .expect("runs"),
            Written::PersonDecided
        ));
        assert_eq!(held(&mut connection, record).await, vec![persons]);
        // Elysia's first guess was confirmed since, so undoing it would lose the confirmation.
        assert!(matches!(
            undo(&mut connection, record, guessed.id, TEST_PERSON_ID).await,
            Err(UndoError::ChangedSince)
        ));

        let kinds: Vec<(String, Uuid)> = history(&mut connection, record)
            .await
            .expect("history")
            .into_iter()
            .map(|entry| (entry.kind, entry.created_by))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("attributed".to_owned(), ELYSIA_USER_ID),
                ("unattributed".to_owned(), TEST_PERSON_ID),
                ("attributed".to_owned(), TEST_PERSON_ID),
                ("attributed".to_owned(), TEST_PERSON_ID),
            ]
        );
        assert!(matches!(
            undo(&mut connection, record, Uuid::now_v7(), TEST_PERSON_ID).await,
            Err(UndoError::NotFound)
        ));
    }
}
