// Copyright © 2026 Jalapeno Labs

//! An action item read by the segmenting step: the projects it belongs to, with Elysia's
//! confidence, the surest to lead it. See `crate::models::action_item_attribution`.
//!
//! An item is not a transcript, so it has no topics to cut: one turn reads its title, notes, and
//! the issues it follows against every project and where each project's work is (the tracker it
//! files issues in, the repositories its workers clone), since an issue in a project's tracker or
//! repository is most likely that project's. The answer is checked as a segment's attributions
//! are (`segmentation::parse_item_answer`), and written as Elysia's reading, which leaves whatever
//! anyone else decided as it is.
//!
//! The log carries counts alone, as every step's does: an item's title and its issues are its
//! person's.

use chrono::Utc;
use uuid::Uuid;

use super::{ItemToAttribute, SegmentMeeting, Segmented, SegmentedTopic};
use crate::action_items::WorkError;
use crate::meetings::segmentation::{
    self, ItemContext, ItemIssue, ProjectChoice, ProjectWork, SegmentProject,
};
use crate::models::action_item::{self, ActionItem, ActionItemState};
use crate::models::{action_item_attribution, action_item_link, worker_setup};
use crate::project_managers::trackers;
use crate::routes::v1::action_items::publish_item_write;
use crate::workflows::{Done, StepContext, StepFailure};

impl SegmentMeeting {
    /// Reads which projects an action item belongs to and puts it on them. See the module's docs.
    pub(super) async fn attribute_item(
        &self,
        input: ItemToAttribute,
        context: &StepContext,
    ) -> Result<Done<Segmented>, StepFailure> {
        let id = input.action_item_id;
        let found = {
            let mut connection = self.connection().await?;
            action_item::find(&mut connection, id)
                .await
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => {
                        StepFailure::permanent("The action item no longer exists")
                    }
                    other => StepFailure::retryable(other),
                })?
        };
        if let Some(reason) = nothing_to_attribute(&found) {
            return Ok(Done::Skipped {
                reason: reason.to_owned(),
            });
        }
        let choices = self.project_choices().await?;
        if choices.is_empty() {
            return Ok(Done::Skipped {
                reason: "There is no project to attribute it to".to_owned(),
            });
        }
        let issues = self.issues_of(id).await?;
        let work = self.work_of(&choices).await?;
        context.log.line(&format!(
            "Reading an item that follows {} issues, against {} projects",
            issues.len(),
            choices.len()
        ));

        let item = ItemContext {
            title: &found.title,
            notes: &found.notes,
            issues: &issues,
        };
        let prompt = segmentation::item_prompt(item, &choices, &work);
        let label = format!("action_item:{id}:attribution");
        let answer = self
            .ask(label, prompt, segmentation::ITEM_SYSTEM_PROMPT, context)
            .await?;
        let guesses = segmentation::parse_item_answer(&answer, &choices).map_err(|error| {
            StepFailure::retryable(format!("The attribution's answer: {error}"))
        })?;
        if !self.write_reading(id, &guesses, context).await? {
            return Ok(Done::Skipped {
                reason: "The item was deleted while it was read".to_owned(),
            });
        }

        let summary = if guesses.is_empty() {
            "No project is about it".to_owned()
        } else {
            format!("Attributed it to {} projects", guesses.len())
        };
        let topic = SegmentedTopic {
            title: found.title,
            summary: String::new(),
            start_ms: 0,
            end_ms: 0,
            projects: guesses,
            record_ids: Vec::new(),
        };
        Ok(Done::Succeeded {
            output: Segmented {
                segments: vec![topic],
                record_ids: Vec::new(),
            },
            summary: Some(summary),
        })
    }

    /// Writes Elysia's reading of item `id` and announces the item. Answers `false` when the item
    /// was deleted while she read it, which writes nothing.
    async fn write_reading(
        &self,
        id: Uuid,
        guesses: &[SegmentProject],
        context: &StepContext,
    ) -> Result<bool, StepFailure> {
        let now = Utc::now();
        let mut connection = self.connection().await?;
        let written =
            match action_item_attribution::attribute(&mut connection, id, guesses, now).await {
                Ok(written) => written,
                Err(WorkError::Conflict(_deleted)) => return Ok(false),
                Err(other) => return Err(StepFailure::retryable(other)),
            };
        context.log.line(&format!(
            "Guessed {} projects; {} changes to the item's projects and lead",
            guesses.len(),
            written.history.len()
        ));
        // The change is in once its transaction commits. A failed announcement leaves pages to
        // show it once reloaded, and asking again would spend another turn for nothing.
        let announced =
            publish_item_write(&self.inner.events, &mut connection, written, &[], now).await;
        if let Err(error) = announced {
            context.log.line(&format!(
                "The item's new projects could not be announced, so pages show them once \
                 reloaded: {error}"
            ));
        }
        Ok(true)
    }

    /// The issues and pull requests item `id` follows, its primary first, each with where it was
    /// filed.
    async fn issues_of(&self, id: Uuid) -> Result<Vec<ItemIssue>, StepFailure> {
        let mut links = {
            let mut connection = self.connection().await?;
            action_item_link::list_for_item(&mut connection, id)
                .await
                .map_err(StepFailure::retryable)?
        };
        links.sort_by_key(|link| !link.is_primary);
        Ok(links
            .into_iter()
            .map(|link| {
                let place = trackers::place_of(
                    link.provider,
                    link.credential().id,
                    &link.external_id,
                    &link.external_key,
                )
                .map(|place| place.destination);
                ItemIssue {
                    provider: link.provider.name().to_owned(),
                    key: link.external_key,
                    title: link.title,
                    url: link.url,
                    place,
                }
            })
            .collect())
    }

    /// Where each of `choices` works: the tracker it files issues in, named or followed from its
    /// items' issues, and the repositories its workers clone.
    async fn work_of(&self, choices: &[ProjectChoice]) -> Result<Vec<ProjectWork>, StepFailure> {
        let mut connection = self.connection().await?;
        let mut work = Vec::with_capacity(choices.len());
        for choice in choices {
            let issues_in = trackers::place(&mut connection, choice.id)
                .await
                .map_err(StepFailure::retryable)?
                .map(|(place, _source)| format!("{} {}", place.provider.name(), place.destination));
            let repositories = match worker_setup::find(&mut connection, choice.id).await {
                Ok(setup) => setup
                    .repositories
                    .into_iter()
                    .map(|repository| repository.url)
                    .collect(),
                // A project removed since the list was read is simply not described.
                Err(diesel::result::Error::NotFound) => continue,
                Err(other) => return Err(StepFailure::retryable(other)),
            };
            work.push(ProjectWork {
                project_id: choice.id,
                issues_in,
                repositories,
            });
        }
        Ok(work)
    }
}

/// Why `item` is not attributed, when it is not: one deleted or dismissed is nobody's work.
fn nothing_to_attribute(item: &ActionItem) -> Option<&'static str> {
    if item.deleted_at.is_some() {
        return Some("The item was deleted, so nothing to attribute");
    }
    if item.state == ActionItemState::Dismissed {
        return Some("The item was dismissed, so nothing to attribute");
    }
    None
}

#[cfg(test)]
mod tests {
    use diesel_async::AsyncPgConnection;

    use super::*;
    use crate::action_items::{Actor, Transition};
    use crate::database::Pool;
    use crate::meetings::segment::TranscriptToSegment;
    use crate::models::action_item::{ActionItemPriority, NewActionItem, Owner};
    use crate::models::action_item_link::LinkProvider;
    use crate::models::project::{self, NewProject};
    use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, migrated_database};
    use crate::workflows::Step;

    /// An item as an assigned issue arrives: in the inbox, in no project.
    async fn arrived(connection: &mut AsyncPgConnection) -> Uuid {
        action_item::create(
            connection,
            NewActionItem {
                title: "Fix the login".to_owned(),
                notes: String::new(),
                state: ActionItemState::Inbox,
                priority: ActionItemPriority::Normal,
                due_at: None,
                owner: Owner::User,
                project_ids: Vec::new(),
                initiative_ids: Vec::new(),
            },
            Actor::Watcher(LinkProvider::Jira),
            Utc::now(),
        )
        .await
        .expect("created")
        .record
        .id
    }

    async fn attribute(database: &Pool, id: Uuid) -> Done<Segmented> {
        SegmentMeeting::for_test(database.clone())
            .run(
                TranscriptToSegment::ActionItem(ItemToAttribute { action_item_id: id }),
                StepContext::for_test::<SegmentMeeting>(database.clone()),
            )
            .await
            .expect("runs")
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_item_nobody_works_or_with_nowhere_to_go_is_not_read() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let item = arrived(&mut connection).await;

        // With no project to put it on, no turn is spent.
        assert_eq!(
            attribute(&database, item).await,
            Done::Skipped {
                reason: "There is no project to attribute it to".to_owned(),
            }
        );

        project::create(
            &mut connection,
            &NewProject {
                name: "Website".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project");
        let now = Utc::now();
        action_item::transition(&mut connection, item, Transition::Dismiss, TEST_PERSON, now)
            .await
            .expect("dismissed");
        assert_eq!(
            attribute(&database, item).await,
            Done::Skipped {
                reason: "The item was dismissed, so nothing to attribute".to_owned(),
            }
        );

        action_item::soft_delete(&mut connection, item, TEST_PERSON, now)
            .await
            .expect("deleted");
        assert_eq!(
            attribute(&database, item).await,
            Done::Skipped {
                reason: "The item was deleted, so nothing to attribute".to_owned(),
            }
        );
    }
}
