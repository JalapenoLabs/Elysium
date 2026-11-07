// Copyright © 2026 Jalapeno Labs

//! Every project's brief: the knowledge base page that holds its goals, its decisions, and what is
//! out of scope. See `docs/autonomy.md`, The project brief.
//!
//! A brief is a page like any other (`crate::models::page`), versioned, indexed, and edited by
//! people and agents alike, with three things of its own:
//!
//! - **Every project has one.** Creating a project creates its brief in the same transaction
//!   ([`create`]), under the Project briefs page, linked to the project, and named for it, and
//!   renaming the project renames it ([`retitle`]). It is the system user's: Elysium made it.
//! - **It holds its place.** `pages.brief_project_id` names its project, which makes it fixed
//!   (`Page::is_fixed`): nobody moves, deletes, or renames it, and its link to its project stays
//!   (`crate::models::page_project::unlink`). Its Project Manager reads it every turn and every
//!   batch's goal is checked against it, so neither may find it gone. Deleting the project leaves
//!   it an ordinary page, as every page linked to a project stays.
//! - **Someone else's edit wakes its Project Manager.** A publish by anyone but that Project
//!   Manager or the system user ([`wakes_its_manager`]) writes a wake in the same transaction,
//!   so a person's correction reaches the next turn however the edit was made: the editor, a
//!   restore, or an agent acting for them.
//!
//! Its goals are what batches serve. Each is a heading one level under `## Goals` ([`goals`]),
//! read from the Markdown deterministically, so a batch that names one ([`find_goal`]) is checked
//! without a model.

use chrono::Utc;
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::pages;
use crate::models::agent::{Agent, Author};
use crate::models::page::{self, PROJECT_BRIEFS_PAGE_ID, Page, PageContent, PageEdit, PageError};
use crate::models::page_project;
use crate::models::project::Project;
use crate::models::user::SYSTEM_USER_ID;

/// What a new brief says: its three sections and how each is used. The migration that gave every
/// project a brief wrote the same words.
pub const TEMPLATE: &str = "## Goals\n\n\
    Each goal is a heading of its own under this one, with what it means and who asked for it \
    beneath it. Every work batch names the goal it serves.\n\n\
    ## Decisions\n\n\
    What has been decided about the project, by whom, and when. A work batch a person rejected is \
    recorded here with their reason.\n\n\
    ## Out of scope\n\n\
    What the project does not do, so no work batch does it.";

/// The section whose headings are the brief's goals, compared as a goal is ([`find_goal`]).
const GOALS_SECTION: &str = "goals";

/// What a brief is called: its project's name, so the tree and a search say whose it is.
pub fn title_for(project: &Project) -> String {
    format!("{} brief", project.name.trim())
}

/// Creates `project`'s brief as the system user: a page under Project briefs holding [`TEMPLATE`],
/// marked as the project's and linked to it. Call it in the transaction that creates the project.
///
/// # Errors
/// Propagates any database error, including a project that already has a brief.
pub async fn create(
    connection: &mut AsyncPgConnection,
    project: &Project,
) -> Result<Page, PageError> {
    let author = Author {
        user_id: SYSTEM_USER_ID,
        agent: None,
        at: Utc::now(),
    };
    let edit = PageEdit {
        title: title_for(project),
        body: TEMPLATE.to_owned(),
        learning: None,
    };
    connection
        .transaction(async move |connection| {
            let created =
                page::create(connection, Some(PROJECT_BRIEFS_PAGE_ID), edit, author).await?;
            diesel::update(pages::table.find(created.page.id))
                .set(pages::brief_project_id.eq(project.id))
                .execute(connection)
                .await?;
            let linked =
                page_project::link(connection, created.page.id, project.id, author).await?;
            Ok(linked.page)
        })
        .await
}

/// `project_id`'s brief as it stands.
///
/// # Errors
/// `NotFound` for a project with no brief, which only a project that does not exist has; any other
/// database error.
pub async fn find(connection: &mut AsyncPgConnection, project_id: Uuid) -> QueryResult<Page> {
    pages::table
        .filter(pages::brief_project_id.eq(project_id))
        .select(Page::as_select())
        .first(connection)
        .await
}

/// Renames `project`'s brief for its name now, as the system user, which wakes no Project Manager.
/// Answers the brief when its title changed. Call it in the transaction that renames the project.
///
/// # Errors
/// `NotFound` for a project with no brief; any other database error.
pub async fn retitle(
    connection: &mut AsyncPgConnection,
    project: &Project,
) -> Result<Option<Page>, PageError> {
    let brief = find(connection, project.id).await?;
    let brief = page::lock(connection, brief.id).await?;
    let title = title_for(project);
    if brief.title == title {
        return Ok(None);
    }
    let content = PageContent {
        title,
        ..brief.content()
    };
    let author = Author {
        user_id: SYSTEM_USER_ID,
        agent: None,
        at: Utc::now(),
    };
    let published = page::publish_content(
        connection,
        &brief,
        content,
        page::Provenance::default(),
        author,
    )
    .await?;
    Ok(Some(published.page))
}

/// Whether a publish of `project_id`'s brief by `author` wakes its Project Manager: everyone's but
/// its own, which would wake it for what it just wrote, and the system user's, which only renames
/// the brief with its project.
pub fn wakes_its_manager(author: Author<'_>, project_id: Uuid) -> bool {
    if author.user_id == SYSTEM_USER_ID {
        return false;
    }
    let its_own = Agent::project_manager(project_id);
    author.agent.is_none_or(|agent| agent.id() != its_own.id())
}

/// The goals in a brief's Markdown `body`, in order, as their headings spell them: every heading
/// one level under `## Goals`, up to the next heading at that level or above.
///
/// Read line by line as Markdown reads a heading (up to three spaces, then `#`s and a space,
/// with any closing `#`s dropped), skipping fenced code, so a `#` in a code sample is never a goal.
/// A goal's own subheadings are part of it, not goals. Underlined (setext) headings are not read.
pub fn goals(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut fence: Option<&str> = None;
    let mut in_goals = false;
    for line in body.lines() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let trimmed = line.trim_start_matches(' ');
        if indent <= 3 {
            let opens = ["```", "~~~"]
                .into_iter()
                .find(|marker| trimmed.starts_with(marker));
            match (fence, opens) {
                (None, Some(marker)) => {
                    fence = Some(marker);
                    continue;
                }
                (Some(open), Some(marker)) if open == marker => {
                    fence = None;
                    continue;
                }
                _ => {}
            }
        }
        if fence.is_some() || indent > 3 {
            continue;
        }

        let level = trimmed
            .chars()
            .take_while(|character| *character == '#')
            .count();
        let rest = &trimmed[level..];
        let is_heading =
            (1..=6).contains(&level) && (rest.is_empty() || rest.starts_with([' ', '\t']));
        if !is_heading {
            continue;
        }
        // A closing run of `#`s is no part of the text, when a space sets it apart.
        let mut text = rest.trim();
        let unclosed = text.trim_end_matches('#');
        if unclosed.is_empty() || unclosed.ends_with([' ', '\t']) {
            text = unclosed.trim_end();
        }
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");

        if level <= 2 {
            in_goals = level == 2 && text.to_lowercase() == GOALS_SECTION;
            continue;
        }
        if in_goals && level == 3 && !text.is_empty() {
            found.push(text);
        }
    }
    found
}

/// The goal in `goals` that `serves` names, as the brief spells it. Spacing and case do not count,
/// so a goal is found however a Project Manager capitalizes it, and nothing else is guessed.
pub fn find_goal<'a>(goals: &'a [String], serves: &str) -> Option<&'a str> {
    let wanted = serves
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    goals
        .iter()
        .find(|goal| goal.to_lowercase() == wanted)
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::page::Placement;
    use crate::models::project::{self, NewProject, ProjectChanges};
    use crate::models::project_manager;
    use crate::models::user::PROJECT_MANAGER_USER_ID;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    #[test]
    fn a_new_brief_names_no_goals_yet() {
        assert!(goals(TEMPLATE).is_empty());
        for section in ["## Goals", "## Decisions", "## Out of scope"] {
            assert!(TEMPLATE.contains(section), "{section}");
        }
    }

    #[test]
    fn the_goals_are_the_headings_one_level_under_goals() {
        let body = "# Launch\n\n\
            ## Goals\n\n\
            ### Annual pricing\n\
            Ada asked for it.\n\n\
            #### How we check it\n\n\
            ###   Spring   launch   ###\n\n\
            ### Pricing #2\n\n\
            ##### Not a goal\n\n\
            ## Decisions\n\n\
            ### Monthly stays\n\n\
            ## goals\n\n\
            ### Self-serve sign-up\n";
        assert_eq!(
            goals(body),
            [
                "Annual pricing",
                "Spring launch",
                "Pricing #2",
                "Self-serve sign-up"
            ],
        );
    }

    #[test]
    fn a_heading_in_code_or_indented_as_code_is_no_goal() {
        let body = "## Goals\n\n\
            ```markdown\n\
            ### Inside a fence\n\
            ~~~\n\
            ### Still inside\n\
            ```\n\n\
            ~~~\n\
            ### Inside a tilde fence\n\
            ~~~\n\n\
            \x20   ### Indented as code\n\n\
            \x20  ### Three spaces is a heading\n\
            ###No space is no heading\n";
        assert_eq!(goals(body), ["Three spaces is a heading"]);
    }

    #[test]
    fn a_goal_is_found_however_it_is_spaced_or_capitalized_and_nothing_else_is() {
        let goals = vec!["Annual pricing".to_owned(), "Spring launch".to_owned()];
        assert_eq!(
            find_goal(&goals, "  annual   PRICING "),
            Some("Annual pricing")
        );
        assert_eq!(find_goal(&goals, "Spring launch"), Some("Spring launch"));
        assert_eq!(find_goal(&goals, "Annual"), None);
        assert_eq!(find_goal(&goals, "Annual pricing page"), None);
        assert_eq!(find_goal(&[], "Annual pricing"), None);
    }

    #[test]
    fn everyone_but_its_own_project_manager_and_the_system_user_wakes_it() {
        let project_id = Uuid::now_v7();
        let at = Utc::now();
        let person = Author {
            user_id: Uuid::now_v7(),
            agent: None,
            at,
        };
        assert!(wakes_its_manager(person, project_id));

        let session = Agent::session(4);
        let persons_agent = Author {
            agent: Some(&session),
            ..person
        };
        assert!(wakes_its_manager(persons_agent, project_id));

        let its_own = Agent::project_manager(project_id);
        let own_manager = Author {
            user_id: PROJECT_MANAGER_USER_ID,
            agent: Some(&its_own),
            at,
        };
        assert!(!wakes_its_manager(own_manager, project_id));

        let another = Agent::project_manager(Uuid::now_v7());
        let another_manager = Author {
            agent: Some(&another),
            ..own_manager
        };
        assert!(
            wakes_its_manager(another_manager, project_id),
            "another project's Project Manager is someone else"
        );

        let system = Author {
            user_id: SYSTEM_USER_ID,
            agent: None,
            at,
        };
        assert!(!wakes_its_manager(system, project_id));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn every_project_gets_a_brief_that_holds_its_place_while_the_project_exists() {
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
        .expect("a project");

        let brief = find(&mut connection, launch.id).await.expect("a brief");
        assert_eq!(brief.title, "Launch brief");
        assert_eq!(brief.body, TEMPLATE);
        assert_eq!(brief.parent_id, Some(PROJECT_BRIEFS_PAGE_ID));
        assert_eq!(brief.created_by, SYSTEM_USER_ID);
        assert!(brief.is_fixed());
        assert_eq!(
            page_project::projects_of(&mut connection, brief.id)
                .await
                .expect("reads"),
            vec![launch.id],
            "it is linked to its project"
        );

        let person = Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        };
        let unlinked = page_project::unlink(&mut connection, brief.id, launch.id, person)
            .await
            .expect_err("it stays linked");
        assert!(
            matches!(unlinked, PageError::BriefStaysLinked),
            "{unlinked:?}"
        );
        let moved = page::move_page(
            &mut connection,
            brief.id,
            Placement {
                parent_id: None,
                after: None,
            },
            person,
        )
        .await
        .expect_err("it stays where it is");
        assert!(matches!(moved, PageError::Fixed), "{moved:?}");
        let deleted = page::delete(&mut connection, brief.id, person, None)
            .await
            .expect_err("it is not deleted");
        assert!(matches!(deleted, PageError::Fixed), "{deleted:?}");
        let renamed = page::publish(
            &mut connection,
            brief.id,
            PageEdit {
                title: "Something else".to_owned(),
                body: TEMPLATE.to_owned(),
                learning: None,
            },
            brief.current_version,
            person,
        )
        .await
        .expect_err("it keeps its name");
        assert!(matches!(renamed, PageError::Fixed), "{renamed:?}");

        // Renaming the project renames its brief, as the system user, which wakes nothing.
        let renamed = project::update(
            &mut connection,
            launch.id,
            &ProjectChanges {
                name: Some("Spring launch".to_owned()),
                ..ProjectChanges::default()
            },
        )
        .await
        .expect("renames");
        let brief = find(&mut connection, renamed.id).await.expect("a brief");
        assert_eq!(brief.title, "Spring launch brief");
        assert_eq!(brief.current_version, 2);
        assert_eq!(
            project_manager::waiting_count(&mut connection, launch.id)
                .await
                .expect("counts"),
            0
        );

        // Deleting the project leaves its brief an ordinary page, as every linked page stays.
        page_project::remove_all_from_project(&mut connection, launch.id, person)
            .await
            .expect("unlinks");
        project::delete(&mut connection, launch.id)
            .await
            .expect("deletes");
        let left = page::find(&mut connection, brief.id).await.expect("stays");
        assert_eq!(left.brief_project_id, None);
        assert!(!left.is_fixed());
        assert!(left.deleted_at.is_none());
    }
}
