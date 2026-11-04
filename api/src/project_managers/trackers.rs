// Copyright © 2026 Jalapeno Labs

//! Where a project's Project Manager files issues, and through which credential.
//!
//! A project's tracker is the one it names (`crate::models::project_issue_tracker`), or else
//! its precedent: the place most of its items' linked issues already are, the newest breaking a
//! tie ([`precedent`]), so a team working in Jira's `ELY` keeps working there. A project that
//! names none and links no issue has no tracker, and filing is refused saying how to name one.
//!
//! The credential is the tracker's own for Jira and YouTrack: the one the project named, or the
//! one its precedent's issues were linked through, which already reaches that place. GitHub
//! writes go through the token the project's sessions get, its own or the workspace default
//! (`Project::github_credential`), so a project files on GitHub as it codes there; one whose
//! sessions get no token cannot file on GitHub at all.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

use crate::database::schema::{action_item_links, action_item_projects, action_items};
use crate::jira::project_of_issue_key;
use crate::models::action_item_link::{LinkKind, LinkProvider};
use crate::models::{github_credential, project, project_issue_tracker};
use crate::youtrack::is_issue_key;

/// Where a project's tracker came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrackerSource {
    /// The project names it.
    Named,
    /// Most of the project's linked issues are there.
    Precedent,
}

/// A place issues open, on one provider: a Jira project key, a GitHub `owner/name`, or a
/// YouTrack project's short name, with the credential that reaches it. A GitHub place carries
/// no credential: see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub provider: LinkProvider,
    pub credential_id: Option<Uuid>,
    pub destination: String,
}

/// Where a project's Project Manager files issues, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracker {
    pub place: Place,
    /// The credential that files: the place's own, or the project's GitHub token.
    pub credential_id: Uuid,
    pub source: TrackerSource,
}

/// A project with no tracker, in words for whoever asked: the Project Manager, or a person on
/// its tab.
pub const NO_TRACKER: &str = "this project names no tracker and none of its items is linked to \
    an issue yet, so there is nowhere to file one; a person can name a tracker on the project's \
    Project Manager tab";

/// A GitHub tracker on a project whose sessions get no token.
pub const NO_GITHUB_TOKEN: &str = "this project files on GitHub, but its sessions get no GitHub \
    token, so nothing can file there; a person can give the project a token, or name another \
    tracker on its Project Manager tab";

/// One issue linked to one of a project's items, as the precedent counts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedIssue {
    pub place: Place,
    pub linked_at: DateTime<Utc>,
}

/// The place an issue link points into, or `None` for a reference no place can be read from.
pub fn place_of(
    provider: LinkProvider,
    credential_id: Uuid,
    external_id: &str,
    external_key: &str,
) -> Option<Place> {
    let (credential_id, destination) = match provider {
        LinkProvider::Jira => (
            Some(credential_id),
            project_of_issue_key(external_key)?.to_ascii_uppercase(),
        ),
        // `owner/name#12`. The token is the project's, not the link's.
        LinkProvider::Github => (None, external_id.rsplit_once('#')?.0.to_owned()),
        LinkProvider::Youtrack => {
            if !is_issue_key(external_key) {
                return None;
            }
            (
                Some(credential_id),
                external_key.rsplit_once('-')?.0.to_owned(),
            )
        }
    };
    Some(Place {
        provider,
        credential_id,
        destination,
    })
}

/// The place most of `linked` points into, a tie going to the one linked most recently. `None`
/// when nothing is linked.
pub fn precedent(linked: &[LinkedIssue]) -> Option<Place> {
    let mut tally: HashMap<&Place, (usize, DateTime<Utc>)> = HashMap::new();
    for issue in linked {
        let entry = tally.entry(&issue.place).or_insert((0, issue.linked_at));
        entry.0 += 1;
        entry.1 = entry.1.max(issue.linked_at);
    }
    tally
        .into_iter()
        .max_by_key(|(_place, (count, newest))| (*count, *newest))
        .map(|(place, _tally)| place.clone())
}

/// The place `project_id` files issues: the one it names, or its precedent.
///
/// # Errors
/// Propagates any database error.
pub async fn place(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Option<(Place, TrackerSource)>> {
    if let Some(named) = project_issue_tracker::find(connection, project_id).await? {
        let place = Place {
            provider: named.provider,
            credential_id: named.credential_id(),
            destination: named.destination,
        };
        return Ok(Some((place, TrackerSource::Named)));
    }
    let linked = linked_issues(connection, project_id).await?;
    Ok(precedent(&linked).map(|place| (place, TrackerSource::Precedent)))
}

/// The tracker `project_id` files issues in, or why it has none: [`NO_TRACKER`], or
/// [`NO_GITHUB_TOKEN`].
///
/// # Errors
/// Propagates any database error, including a project that does not exist.
pub async fn resolve(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Result<Tracker, &'static str>> {
    let Some((place, source)) = place(connection, project_id).await? else {
        return Ok(Err(NO_TRACKER));
    };
    let credential_id = if let Some(credential_id) = place.credential_id {
        credential_id
    } else {
        let project = project::find(connection, project_id).await?;
        let workspace_default = github_credential::find_default(connection)
            .await?
            .map(|credential| credential.id);
        let Some(credential_id) = project.github_credential(workspace_default) else {
            return Ok(Err(NO_GITHUB_TOKEN));
        };
        credential_id
    };
    Ok(Ok(Tracker {
        place,
        credential_id,
        source,
    }))
}

/// Every issue linked to a live item of `project_id`, with where it points.
async fn linked_issues(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Vec<LinkedIssue>> {
    type Row = (
        LinkProvider,
        Option<Uuid>,
        Option<Uuid>,
        Option<Uuid>,
        String,
        String,
        DateTime<Utc>,
    );
    let rows: Vec<Row> = action_item_links::table
        .inner_join(action_items::table)
        .inner_join(
            action_item_projects::table
                .on(action_item_projects::action_item_id.eq(action_item_links::action_item_id)),
        )
        .filter(action_item_projects::project_id.eq(project_id))
        .filter(action_items::deleted_at.is_null())
        .filter(action_item_links::kind.eq(LinkKind::Issue))
        .select((
            action_item_links::provider,
            action_item_links::jira_credential_id,
            action_item_links::github_credential_id,
            action_item_links::token_account_id,
            action_item_links::external_id,
            action_item_links::external_key,
            action_item_links::created_at,
        ))
        .load(connection)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(
            |(provider, jira, github, token, external_id, external_key, linked_at)| {
                let credential_id = jira.or(github).or(token)?;
                let place = place_of(provider, credential_id, &external_id, &external_key)?;
                Some(LinkedIssue { place, linked_at })
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 4, hour, 0, 0)
            .single()
            .expect("a moment")
    }

    fn jira(credential_id: Uuid, key: &str, hour: u32) -> LinkedIssue {
        LinkedIssue {
            place: place_of(LinkProvider::Jira, credential_id, "10042", key).expect("a key"),
            linked_at: at(hour),
        }
    }

    #[test]
    fn a_place_is_read_from_each_providers_reference() {
        let credential = Uuid::now_v7();
        assert_eq!(
            place_of(LinkProvider::Jira, credential, "10042", "ely-12"),
            Some(Place {
                provider: LinkProvider::Jira,
                credential_id: Some(credential),
                destination: "ELY".to_owned(),
            })
        );
        assert_eq!(
            place_of(
                LinkProvider::Github,
                credential,
                "JalapenoLabs/Elysium#12",
                "JalapenoLabs/Elysium#12"
            ),
            Some(Place {
                provider: LinkProvider::Github,
                credential_id: None,
                destination: "JalapenoLabs/Elysium".to_owned(),
            }),
            "a GitHub place files with the project's token, whoever linked it"
        );
        assert_eq!(
            place_of(LinkProvider::Youtrack, credential, "2-41", "JL-7")
                .map(|place| place.destination),
            Some("JL".to_owned())
        );
        assert_eq!(
            place_of(LinkProvider::Jira, credential, "1", "not a key"),
            None
        );
    }

    #[test]
    fn the_precedent_is_where_most_issues_are_and_the_newest_breaks_a_tie() {
        let work = Uuid::now_v7();
        assert_eq!(precedent(&[]), None, "nothing linked, no precedent");

        let mostly_ely = [
            jira(work, "ELY-1", 1),
            jira(work, "ELY-2", 2),
            jira(work, "OPS-1", 9),
        ];
        assert_eq!(
            precedent(&mostly_ely).map(|place| place.destination),
            Some("ELY".to_owned()),
            "two in ELY beat one newer in OPS"
        );

        let tied = [jira(work, "ELY-1", 1), jira(work, "OPS-1", 5)];
        assert_eq!(
            precedent(&tied).map(|place| place.destination),
            Some("OPS".to_owned()),
            "a tie goes to the place linked most recently"
        );
    }

    #[test]
    fn the_same_project_through_two_credentials_is_two_places() {
        let work = Uuid::now_v7();
        let other = Uuid::now_v7();
        let linked = [
            jira(work, "ELY-1", 1),
            jira(other, "ELY-2", 2),
            jira(other, "ELY-3", 3),
        ];
        assert_eq!(
            precedent(&linked).and_then(|place| place.credential_id),
            Some(other),
            "the credential most of its issues were linked through"
        );
    }
}
