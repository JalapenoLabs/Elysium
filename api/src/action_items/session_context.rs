// Copyright © 2026 Jalapeno Labs

//! The first turn of a coding session started from an action item.
//!
//! The agent starts with the full picture: the item, the session's project, the item's
//! initiatives with their progress, and its latest comments, then the user's request. It is
//! Markdown, since that is what agents read best, and compact: long text is cut short at a
//! fixed length and the oldest comments are left out, because the `elysium_work` tools
//! (`crate::tools::work`) read all of it on demand. Timestamps are UTC, as everywhere on the
//! server.
//!
//! Links' descriptions join the turn once links exist; see `docs/action-items.md`.

use std::fmt::{self, Write as _};

use chrono::{DateTime, SecondsFormat, Utc};

use super::progress::Progress;
use crate::models::action_item::{ActionItem, ActionItemPriority, ActionItemState, Owner};
use crate::models::action_item_comment::Comment;
use crate::models::initiative::{Initiative, InitiativeState};
use crate::models::project::Project;

/// The most of an item's notes the turn carries. Enough for a bug report with its steps;
/// the rest is one `work_item` call away.
const NOTES_MAX_CHARACTERS: usize = 4000;

/// The most of the project's description the turn carries.
const PROJECT_DESCRIPTION_MAX_CHARACTERS: usize = 1000;

/// The most of each initiative's description the turn carries: a line of purpose, not
/// the plan.
const INITIATIVE_DESCRIPTION_MAX_CHARACTERS: usize = 300;

/// The most of each comment the turn carries.
const COMMENT_MAX_CHARACTERS: usize = 1000;

/// How many of the latest comments the turn carries. Recent discussion is what the agent
/// acts on; older comments stay readable through `work_item`.
const RECENT_COMMENTS: usize = 10;

/// What a first turn is built from.
#[derive(Debug, Clone, Copy)]
pub struct ItemContext<'context> {
    pub item: &'context ActionItem,
    /// The session's project, which every `elysium_work` call is scoped to.
    pub project: &'context Project,
    /// The initiatives the item is in now, with their progress.
    pub initiatives: &'context [(Initiative, Progress)],
    /// Every comment on the item, oldest first.
    pub comments: &'context [Comment],
}

/// The text of the session's first turn: the item's context, then `request`.
///
/// # Panics
/// Never in practice: writing to a `String` cannot fail.
pub fn first_turn(context: ItemContext<'_>, request: &str) -> String {
    let mut turn = String::new();
    write_turn(&mut turn, context, request).expect("writing to a String cannot fail");
    turn
}

fn write_turn(turn: &mut String, context: ItemContext<'_>, request: &str) -> fmt::Result {
    let ItemContext {
        item,
        project,
        initiatives,
        comments,
    } = context;

    turn.push_str(
        "This coding session was started from an action item in Elysium, the user's work \
         tracker. The item, its project, its initiatives, and its latest comments follow, \
         then the user's request. Long text is cut short here; the elysium_work tools read \
         this project's items and initiatives in full and comment on items. Refer to this \
         item by its id.\n\n",
    );

    writeln!(turn, "# Action item: {}\n", item.title)?;
    writeln!(turn, "- Id: {}", item.id)?;
    writeln!(turn, "- State: {}", state_name(item.state))?;
    writeln!(turn, "- Priority: {}", priority_name(item.priority))?;
    if let Some(due_at) = item.due_at {
        writeln!(turn, "- Due: {}", timestamp(due_at))?;
    }
    if let Some(waiting_on) = &item.waiting_on {
        writeln!(turn, "- Waiting on: {waiting_on}")?;
    }
    match item.owner() {
        Owner::User => {}
        Owner::Other { name } => writeln!(turn, "- Owner: {name}, not the user")?,
        Owner::Nobody => turn.push_str("- Owner: nobody\n"),
    }

    turn.push_str("\n## Notes\n\n");
    push_block(turn, &item.notes, NOTES_MAX_CHARACTERS);

    writeln!(turn, "\n## Project: {}\n", project.name)?;
    push_block(
        turn,
        &project.description,
        PROJECT_DESCRIPTION_MAX_CHARACTERS,
    );

    turn.push_str("\n## Initiatives\n\n");
    if initiatives.is_empty() {
        turn.push_str("None.\n");
    }
    for (initiative, progress) in initiatives {
        write!(
            turn,
            "- {} ({}, {} of {} items resolved",
            initiative.name,
            initiative_state_name(initiative.state),
            progress.resolved,
            progress.total,
        )?;
        if let Some(target_at) = initiative.target_at {
            write!(turn, ", target {}", timestamp(target_at))?;
        }
        turn.push(')');
        let description = initiative.description.trim();
        if !description.is_empty() {
            write!(
                turn,
                ": {}",
                excerpt(description, INITIATIVE_DESCRIPTION_MAX_CHARACTERS)
            )?;
        }
        turn.push('\n');
    }

    turn.push_str("\n## Comments\n\n");
    write_comments(turn, comments)?;

    turn.push_str("\n# Request\n\n");
    turn.push_str(request.trim());
    turn.push('\n');
    Ok(())
}

/// The latest comments, oldest first, each as a list entry with its later lines indented
/// under it.
fn write_comments(turn: &mut String, comments: &[Comment]) -> fmt::Result {
    if comments.is_empty() {
        turn.push_str("None.\n");
        return Ok(());
    }
    let recent = &comments[comments.len().saturating_sub(RECENT_COMMENTS)..];
    if recent.len() < comments.len() {
        writeln!(
            turn,
            "The latest {} of {}, oldest first.\n",
            recent.len(),
            comments.len()
        )?;
    }
    for comment in recent {
        let body = excerpt(comment.body.trim(), COMMENT_MAX_CHARACTERS).replace('\n', "\n  ");
        writeln!(
            turn,
            "- {} {}: {body}",
            timestamp(comment.created_at),
            comment.author
        )?;
    }
    Ok(())
}

/// A block of free text, or `None.` when there is none.
fn push_block(turn: &mut String, text: &str, max_characters: usize) {
    let text = text.trim();
    if text.is_empty() {
        turn.push_str("None.\n");
        return;
    }
    turn.push_str(&excerpt(text, max_characters));
    turn.push('\n');
}

/// `text` cut to `max_characters`, marked where it was cut.
fn excerpt(text: &str, max_characters: usize) -> String {
    let Some((cut, _rest)) = text.char_indices().nth(max_characters) else {
        return text.to_owned();
    };
    format!("{} [cut short]", text[..cut].trim_end())
}

fn timestamp(moment: DateTime<Utc>) -> String {
    moment.to_rfc3339_opts(SecondsFormat::Secs, true)
}

const fn state_name(state: ActionItemState) -> &'static str {
    match state {
        ActionItemState::Inbox => "inbox",
        ActionItemState::Open => "open",
        ActionItemState::Resolved => "resolved",
        ActionItemState::Dismissed => "dismissed",
    }
}

const fn priority_name(priority: ActionItemPriority) -> &'static str {
    match priority {
        ActionItemPriority::Urgent => "urgent",
        ActionItemPriority::High => "high",
        ActionItemPriority::Normal => "normal",
        ActionItemPriority::Low => "low",
    }
}

const fn initiative_state_name(state: InitiativeState) -> &'static str {
    match state {
        InitiativeState::Active => "active",
        InitiativeState::Achieved => "achieved",
        InitiativeState::Abandoned => "abandoned",
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use uuid::Uuid;

    use super::*;
    use crate::models::action_item::OwnerKind;
    use crate::models::project::{GithubAccess, ProjectCoverFit};

    fn moment(offset_minutes: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
            + TimeDelta::minutes(offset_minutes)
    }

    fn item() -> ActionItem {
        ActionItem {
            id: Uuid::nil(),
            title: "Fix the login bug".to_owned(),
            notes: "Users on Safari are logged out after a refresh.".to_owned(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::High,
            due_at: Some(moment(60 * 24)),
            snoozed_until: None,
            waiting_on: None,
            owner_kind: OwnerKind::User,
            owner_name: None,
            resolved_at: None,
            dismissed_at: None,
            deleted_at: None,
            created_at: moment(0),
            updated_at: moment(0),
        }
    }

    fn project() -> Project {
        Project {
            id: Uuid::nil(),
            name: "Elysium".to_owned(),
            description: "The user's workspace.".to_owned(),
            created_at: moment(0),
            updated_at: moment(0),
            cover_image_updated_at: None,
            cover_fit: ProjectCoverFit::Fit,
            github_access: GithubAccess::Default,
            github_credential_id: None,
        }
    }

    fn initiative(name: &str, description: &str) -> Initiative {
        Initiative {
            id: Uuid::now_v7(),
            name: name.to_owned(),
            description: description.to_owned(),
            target_at: Some(moment(60 * 24 * 7)),
            state: InitiativeState::Active,
            deleted_at: None,
            created_at: moment(0),
            updated_at: moment(0),
        }
    }

    fn comment(index: i64, author: &str, body: &str) -> Comment {
        Comment {
            id: Uuid::now_v7(),
            action_item_id: Uuid::nil(),
            author: author.to_owned(),
            body: body.to_owned(),
            created_at: moment(index),
            updated_at: moment(index),
        }
    }

    #[test]
    fn the_turn_carries_the_item_its_project_initiatives_and_comments_then_the_request() {
        let item = item();
        let project = project();
        let initiatives = [(
            initiative("Ship sign-in", "Every browser stays signed in."),
            Progress {
                resolved: 3,
                total: 7,
            },
        )];
        let comments = [
            comment(1, "user", "Seen on Safari 18."),
            comment(
                2,
                "session:4",
                "The cookie lacks SameSite.\nIt is set in auth.rs.",
            ),
        ];
        let turn = first_turn(
            ItemContext {
                item: &item,
                project: &project,
                initiatives: &initiatives,
                comments: &comments,
            },
            "  Fix it and open a pull request.  ",
        );

        let expected = "\
# Action item: Fix the login bug

- Id: 00000000-0000-0000-0000-000000000000
- State: open
- Priority: high
- Due: 2026-09-19T12:00:00Z

## Notes

Users on Safari are logged out after a refresh.

## Project: Elysium

The user's workspace.

## Initiatives

- Ship sign-in (active, 3 of 7 items resolved, target 2026-09-25T12:00:00Z): Every browser stays signed in.

## Comments

- 2026-09-18T12:01:00Z user: Seen on Safari 18.
- 2026-09-18T12:02:00Z session:4: The cookie lacks SameSite.
  It is set in auth.rs.

# Request

Fix it and open a pull request.
";
        assert!(turn.ends_with(expected), "{turn}");
        assert!(
            turn.starts_with("This coding session was started from an action item"),
            "{turn}"
        );
        assert!(turn.contains("elysium_work"), "{turn}");
    }

    #[test]
    fn empty_sections_say_none_and_someone_elses_item_names_its_owner() {
        let mut item = item();
        item.notes = "  ".to_owned();
        item.due_at = None;
        item.waiting_on = Some("sam@example.com".to_owned());
        item.owner_kind = OwnerKind::Other;
        item.owner_name = Some("Sam".to_owned());
        let mut project = project();
        project.description = String::new();

        let turn = first_turn(
            ItemContext {
                item: &item,
                project: &project,
                initiatives: &[],
                comments: &[],
            },
            "Look into it.",
        );

        assert!(!turn.contains("- Due:"), "{turn}");
        assert!(turn.contains("- Waiting on: sam@example.com\n"), "{turn}");
        assert!(turn.contains("- Owner: Sam, not the user\n"), "{turn}");
        assert!(turn.contains("## Notes\n\nNone.\n"), "{turn}");
        assert!(turn.contains("## Project: Elysium\n\nNone.\n"), "{turn}");
        assert!(turn.contains("## Initiatives\n\nNone.\n"), "{turn}");
        assert!(turn.contains("## Comments\n\nNone.\n"), "{turn}");
    }

    #[test]
    fn long_text_is_cut_short_and_only_the_latest_comments_are_carried() {
        let mut item = item();
        item.notes = "é".repeat(NOTES_MAX_CHARACTERS + 50);
        let project = project();
        let comments: Vec<Comment> = (0..15)
            .map(|index| {
                let body = if index == 14 {
                    "x".repeat(COMMENT_MAX_CHARACTERS * 2)
                } else {
                    format!("comment {index}")
                };
                comment(index, "user", &body)
            })
            .collect();

        let turn = first_turn(
            ItemContext {
                item: &item,
                project: &project,
                initiatives: &[],
                comments: &comments,
            },
            "Go.",
        );

        let notes = format!("{} [cut short]\n", "é".repeat(NOTES_MAX_CHARACTERS));
        assert!(
            turn.contains(&notes),
            "notes are cut at a character boundary"
        );
        assert!(
            turn.contains("The latest 10 of 15, oldest first."),
            "{turn}"
        );
        assert!(!turn.contains("comment 4\n"), "{turn}");
        assert!(turn.contains("comment 5\n"), "{turn}");
        let longest = format!("{} [cut short]", "x".repeat(COMMENT_MAX_CHARACTERS));
        assert!(turn.contains(&longest), "{turn}");
        assert!(!turn.contains(&"x".repeat(COMMENT_MAX_CHARACTERS + 1)));
    }

    #[test]
    fn excerpts_keep_short_text_whole() {
        assert_eq!(excerpt("short", 5), "short");
        assert_eq!(excerpt("longer text", 6), "longer [cut short]");
    }
}
