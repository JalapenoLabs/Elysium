// Copyright © 2026 Jalapeno Labs

//! The tasks heard in a meeting, and where each one goes. See `docs/meetings.md`, Tasks.
//!
//! Segmenting names every task it hears (`super::segmentation`): its lines, what is to be done,
//! who owes it, and the project it is for. Each becomes a mark of kind `task` over its words and
//! its row (`models::meeting_task`), taken with the meeting's excerpts and black-outs in one change
//! (`super::excerpts`). Where it goes follows its [`Share`]:
//!
//! - **A project's** reaches the project's Project Manager, as a `task` wake, with its words and
//!   the excerpts around it (`crate::project_managers`). It makes the action items, each linked to
//!   the task, or says why it made none, as its changeset under the project's level of autonomy.
//! - **The person's own**, with no project, becomes an action item of theirs at once.
//! - **A coworker's** goes to them when they are on Elysium, and to the person, waiting on them,
//!   when they are not.
//! - **Other**, a task on no project that nobody was named for, is the person's to triage.
//!
//! The items made at once are inbox items raised by Elysia, as every item she raises is, each kept
//! beside its task (`meeting_task_items`) with whom it is for.
//!
//! # Reading a meeting again
//!
//! Done reviewing segments a meeting again (`super::review`), so a task heard before is heard
//! again, often over lines that moved with an edit. [`read_again`] decides which task of Elysia's
//! each answer's task is: the one over exactly its words, else one over words it shares with the
//! same title, else the only one over words it shares owed by the same and for the same project.
//! A task read again keeps its mark, moved onto its new words, so its items and its wakes stay its
//! own, and it reaches anyone only when it [`changed`]: so Done reviewing sends Project Managers
//! the tasks that changed and nothing else, and never raises an item twice.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use super::segmentation::clock;
use crate::action_items::{Actor, WorkError};
use crate::models::action_item::{
    self, ActionItem, ActionItemChanges, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::meeting::Meeting;
use crate::models::meeting_task::{self, MeetingTask, OwedBy, TaskReading};
use crate::models::project_manager;
use crate::models::user::ELYSIA_USER_ID;

/// Whose a task is, which says where it goes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Share {
    /// A project's, whatever who owes it: its Project Manager makes its items.
    Project(Uuid),
    /// The meeting's own person's, on no project.
    Own,
    /// A coworker's on Elysium, on no project.
    Coworker(Uuid),
    /// Someone's not on Elysium, on no project: the person waits on them.
    WaitingOn(String),
    /// On no project, and nobody was named for it.
    Other,
}

/// Whose `reading` is, in a meeting whose person is `owner`: a project's before anyone's, since
/// its Project Manager decides what its work needs, then whoever owes it.
pub fn share_of(reading: &TaskReading, owner: Uuid) -> Share {
    if let Some(project_id) = reading.project_id {
        return Share::Project(project_id);
    }
    match &reading.owed_by {
        OwedBy::Person { user_id } if *user_id == owner => Share::Own,
        OwedBy::Person { user_id } => Share::Coworker(*user_id),
        OwedBy::Named { name } => Share::WaitingOn(name.clone()),
        OwedBy::Nobody => Share::Other,
    }
}

/// Whether `now` is a change from `before` that reaches anyone again: a new project, someone else
/// owing it, or another title. Spacing and letter case are not a change, as a model's rereading
/// varies in both.
pub fn changed(before: &TaskReading, now: &TaskReading) -> bool {
    before.project_id != now.project_id
        || before.owed_by != now.owed_by
        || !same_title(&before.title, &now.title)
}

/// Whether two titles say the same, whatever their spacing and letter case.
fn same_title(first: &str, second: &str) -> bool {
    let first = first.split_whitespace().map(str::to_lowercase);
    let second = second.split_whitespace().map(str::to_lowercase);
    first.eq(second)
}

/// A task by the places of its words in the transcript as it reads, and what it is.
#[derive(Debug, Clone, Copy)]
pub struct Placed<'reading> {
    pub places: &'reading RangeInclusive<usize>,
    pub reading: &'reading TaskReading,
}

impl Placed<'_> {
    fn overlaps(&self, other: &Placed<'_>) -> bool {
        self.places.start() <= other.places.end() && other.places.start() <= self.places.end()
    }
}

/// For each of `answered`, the index in `standing` of the task of Elysia's it reads again, if
/// any, each standing task read again once: first every one over exactly the same words, then
/// one over words they share with the same title, then the one over words they share owed by the
/// same and for the same project, when no other answered task left unmatched shares words with it.
/// A task matched by none is new. Pure, so the rules are tested alone.
pub fn read_again(answered: &[Placed<'_>], standing: &[Placed<'_>]) -> Vec<Option<usize>> {
    let mut matching = Matching {
        answered,
        standing,
        matched: vec![None; answered.len()],
        taken: vec![false; standing.len()],
    };
    matching.pass(|answer, before, _rivals| answer.places == before.places);
    matching
        .pass(|answer, before, _rivals| same_title(&answer.reading.title, &before.reading.title));
    matching.pass(|answer, before, rivals| {
        rivals == 0
            && answer.reading.owed_by == before.reading.owed_by
            && answer.reading.project_id == before.reading.project_id
    });
    matching.matched
}

/// [`read_again`] as it goes: which answered task matched which standing one so far.
struct Matching<'tasks, 'reading> {
    answered: &'tasks [Placed<'reading>],
    standing: &'tasks [Placed<'reading>],
    matched: Vec<Option<usize>>,
    taken: Vec<bool>,
}

impl Matching<'_, '_> {
    /// Matches each answered task still unmatched to the first standing task still free that
    /// shares words with it and `accepts` it, told how many other unmatched answered tasks share
    /// words with that standing task too.
    fn pass(&mut self, accepts: impl Fn(&Placed<'_>, &Placed<'_>, usize) -> bool) {
        for index in 0..self.answered.len() {
            if self.matched[index].is_some() {
                continue;
            }
            let answer = &self.answered[index];
            let found = (0..self.standing.len()).find(|&candidate| {
                let before = &self.standing[candidate];
                if self.taken[candidate] || !answer.overlaps(before) {
                    return false;
                }
                let rivals = (0..self.answered.len())
                    .filter(|&other| {
                        other != index
                            && self.matched[other].is_none()
                            && self.answered[other].overlaps(before)
                    })
                    .count();
                accepts(answer, before, rivals)
            });
            if let Some(candidate) = found {
                self.matched[index] = Some(candidate);
                self.taken[candidate] = true;
            }
        }
    }
}

/// What a meeting's tasks need to reach anyone: the meeting, its people's names, and whether its
/// transcript is timed, so an item says when the task was heard.
#[derive(Debug, Clone, Copy)]
pub struct Reach<'meeting> {
    pub meeting: &'meeting Meeting,
    pub names: &'meeting HashMap<Uuid, String>,
    pub timed: bool,
}

/// Sends `task`, new or changed, where its share says, the moment its words start at `start_ms`:
/// a project's wakes its Project Manager, and anyone else's becomes an inbox item for whoever it
/// is for, unless one was raised for them already. Answers the item it raised, if it did. Call it
/// in the transaction that takes the task.
///
/// # Errors
/// Propagates any database error, and any refusal of the item, which an item on no project never
/// meets.
pub async fn send(
    connection: &mut AsyncPgConnection,
    reach: Reach<'_>,
    task: &MeetingTask,
    start_ms: i64,
) -> Result<Option<Recorded<ActionItem>>, WorkError> {
    let owner = reach.meeting.created_by;
    let share = share_of(&task.reading(), owner);
    if let Share::Project(project_id) = share {
        project_manager::wake_for_task(connection, project_id, task.mark_id, ELYSIA_USER_ID)
            .await?;
        return Ok(None);
    }
    let person_id = match share {
        Share::Coworker(coworker) => coworker,
        Share::Project(_) | Share::Own | Share::WaitingOn(_) | Share::Other => owner,
    };
    if meeting_task::has_item_for(connection, task.mark_id, person_id).await? {
        return Ok(None);
    }
    let name_of = |id: Uuid| {
        reach
            .names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "someone".to_owned())
    };
    let heard = heard_in(reach, start_ms);
    let notes = match &share {
        Share::WaitingOn(name) => format!("For {}, waiting on {name}. {heard}", name_of(owner)),
        Share::Other => format!("For {}; nobody was named to do it. {heard}", name_of(owner)),
        Share::Project(_) | Share::Own | Share::Coworker(_) => {
            format!("For {}. {heard}", name_of(person_id))
        }
    };
    let now = Utc::now();
    let item = NewActionItem {
        title: task.title.clone(),
        notes,
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    };
    let mut raised = action_item::create(connection, item, Actor::Elysia, now).await?;
    if let Share::WaitingOn(name) = share {
        let waiting = ActionItemChanges {
            waiting_on: Some(Some(name)),
            ..ActionItemChanges::default()
        };
        let updated =
            action_item::update(connection, raised.record.id, waiting, Actor::Elysia, now).await?;
        raised.record = updated.record;
        raised.history.extend(updated.history);
    }
    meeting_task::record_item(
        connection,
        task.mark_id,
        raised.record.id,
        Some(person_id),
        ELYSIA_USER_ID,
    )
    .await?;
    Ok(Some(raised))
}

/// Where a task was heard, for its item's notes: the meeting by its title, and when in it.
fn heard_in(reach: Reach<'_>, start_ms: i64) -> String {
    let title = reach.meeting.title.trim();
    let title = if title.is_empty() { "a meeting" } else { title };
    if reach.timed {
        return format!("Heard in {title} at {}.", clock(start_ms));
    }
    format!("Heard in {title}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(title: &str, owed_by: OwedBy, project_id: Option<Uuid>) -> TaskReading {
        TaskReading {
            title: title.to_owned(),
            owed_by,
            project_id,
        }
    }

    #[test]
    fn a_tasks_share_is_its_projects_before_anyones() {
        let owner = Uuid::now_v7();
        let coworker = Uuid::now_v7();
        let launch = Uuid::now_v7();
        let ada = OwedBy::Person { user_id: coworker };
        assert_eq!(
            share_of(&reading("Fix it", ada.clone(), Some(launch)), owner),
            Share::Project(launch)
        );
        assert_eq!(
            share_of(
                &reading("Send it", OwedBy::Person { user_id: owner }, None),
                owner
            ),
            Share::Own
        );
        assert_eq!(
            share_of(&reading("Fix it", ada, None), owner),
            Share::Coworker(coworker)
        );
        let sharath = OwedBy::Named {
            name: "Sharath".to_owned(),
        };
        assert_eq!(
            share_of(&reading("Book it", sharath, None), owner),
            Share::WaitingOn("Sharath".to_owned())
        );
        assert_eq!(
            share_of(&reading("Ask legal", OwedBy::Nobody, None), owner),
            Share::Other
        );
    }

    #[test]
    fn spacing_and_case_are_no_change_but_owner_project_and_words_are() {
        let before = reading("Send the deck", OwedBy::Nobody, None);
        assert!(!changed(
            &before,
            &reading("  send THE   deck", OwedBy::Nobody, None)
        ));
        assert!(changed(
            &before,
            &reading("Send the slides", OwedBy::Nobody, None)
        ));
        assert!(changed(
            &before,
            &reading("Send the deck", OwedBy::Nobody, Some(Uuid::now_v7()))
        ));
        let named = OwedBy::Named {
            name: "Ada".to_owned(),
        };
        assert!(changed(&before, &reading("Send the deck", named, None)));
    }

    #[test]
    fn a_task_read_again_is_the_one_over_its_words_then_its_title_then_its_owner() {
        let ada = OwedBy::Named {
            name: "Ada".to_owned(),
        };
        let deck = reading("Send the deck", ada.clone(), None);
        let room = reading("Book the room", OwedBy::Nobody, None);
        let bug = reading("Fix the login bug", ada.clone(), None);
        let standing_places = [0..=4, 10..=12, 20..=25];
        let standing = [
            Placed {
                places: &standing_places[0],
                reading: &deck,
            },
            Placed {
                places: &standing_places[1],
                reading: &room,
            },
            Placed {
                places: &standing_places[2],
                reading: &bug,
            },
        ];

        // The room over exactly its words, the deck over moved words with its title, the bug
        // reworded over moved words but owed by the same, and a new task elsewhere.
        let reworded = reading("Fix the sign-in bug", ada, None);
        let other = reading("Write the notes", OwedBy::Nobody, None);
        let answered_places = [10..=12, 2..=6, 19..=24, 40..=41];
        let answered = [
            Placed {
                places: &answered_places[0],
                reading: &room,
            },
            Placed {
                places: &answered_places[1],
                reading: &deck,
            },
            Placed {
                places: &answered_places[2],
                reading: &reworded,
            },
            Placed {
                places: &answered_places[3],
                reading: &other,
            },
        ];

        assert_eq!(
            read_again(&answered, &standing),
            vec![Some(1), Some(0), Some(2), None]
        );
    }

    #[test]
    fn a_task_two_answers_share_words_with_goes_to_neither_without_its_title() {
        let deck = reading("Send the deck", OwedBy::Nobody, None);
        let slides = reading("Send the slides", OwedBy::Nobody, None);
        let notes = reading("Send the notes", OwedBy::Nobody, None);
        let places = [0..=9, 0..=3, 5..=9];
        let standing = [Placed {
            places: &places[0],
            reading: &deck,
        }];
        let answered = [
            Placed {
                places: &places[1],
                reading: &slides,
            },
            Placed {
                places: &places[2],
                reading: &notes,
            },
        ];

        // Neither shares its title, and neither is alone over its words, so both are new.
        assert_eq!(read_again(&answered, &standing), vec![None, None]);
    }
}
