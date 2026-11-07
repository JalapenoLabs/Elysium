// Copyright © 2026 Jalapeno Labs

use serde_json::json;

use super::*;
use crate::models::action_item::{ActionItemPriority, NewActionItem, Owner};
use crate::models::action_item_event::list_for_item;
use crate::models::action_item_link::LinkProvider;
use crate::models::project::NewProject;
use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, migrated_database};

/// The watcher, which brings an assigned issue in with no project.
const WATCHER: Actor = Actor::Watcher(LinkProvider::Jira);

async fn project(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id
}

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
        WATCHER,
        Utc::now(),
    )
    .await
    .expect("created")
    .record
    .id
}

fn guess(project_id: Uuid, confidence: f32) -> SegmentProject {
    SegmentProject {
        project_id,
        confidence,
    }
}

/// The item's projects as `(project, confidence, who)`, in project id order, and its lead.
async fn held(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> (Vec<(Uuid, f32, String)>, Option<Uuid>) {
    let mut projects: Vec<(Uuid, f32, String)> = action_item::project_attributions(connection, id)
        .await
        .expect("reads")
        .into_iter()
        .map(|membership| {
            (
                membership.project_id,
                membership.confidence,
                membership.attributed_by,
            )
        })
        .collect();
    projects.sort_by_key(|(project_id, _, _)| *project_id);
    let lead = action_item::find(connection, id)
        .await
        .expect("an item")
        .lead_project_id;
    (projects, lead)
}

async fn latest_entry(connection: &mut AsyncPgConnection, id: Uuid, kind: &str) -> HistoryEntry {
    list_for_item(connection, id)
        .await
        .expect("history")
        .into_iter()
        .rev()
        .find(|entry| entry.kind == kind)
        .expect("an entry of that kind")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn elysias_reading_puts_an_item_on_its_projects_led_by_her_surest() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let launch = project(&mut connection, "Launch").await;
    let item = arrived(&mut connection).await;

    let read = attribute(
        &mut connection,
        item,
        &[guess(website, 0.4), guess(launch, 0.9)],
        Utc::now(),
    )
    .await
    .expect("attributed");
    // Joined surest first, so the first project it joined, which leads it, is her surest.
    let kinds: Vec<&str> = read
        .history
        .iter()
        .map(|entry| entry.kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        ["project_added", "lead_project_changed", "project_added"]
    );
    let mut expected = vec![
        (website, 0.4, "elysia".to_owned()),
        (launch, 0.9, "elysia".to_owned()),
    ];
    expected.sort_by_key(|(project_id, _, _)| *project_id);
    assert_eq!(held(&mut connection, item).await, (expected, Some(launch)));

    // Read again as it stands, nothing changes.
    let again = attribute(
        &mut connection,
        item,
        &[guess(website, 0.4), guess(launch, 0.9)],
        Utc::now(),
    )
    .await
    .expect("attributed");
    assert!(again.history.is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_persons_correction_stands_against_elysias_next_reading() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let launch = project(&mut connection, "Launch").await;
    let billing = project(&mut connection, "Billing").await;
    let item = arrived(&mut connection).await;
    let now = Utc::now();
    attribute(
        &mut connection,
        item,
        &[guess(website, 0.6), guess(launch, 0.8)],
        now,
    )
    .await
    .expect("attributed");

    // The person takes it off the launch, which led it, and makes the website its lead.
    let removed = action_item::remove_project(&mut connection, item, launch, TEST_PERSON, now)
        .await
        .expect("removed");
    // The lead passed to the project left, the website.
    assert_eq!(removed.record.lead_project_id, Some(website));
    action_item::add_project(&mut connection, item, billing, TEST_PERSON, now)
        .await
        .expect("added");
    set_lead(&mut connection, item, billing, TEST_PERSON, now)
        .await
        .expect("led");

    // A fuller reading guesses the launch again, surest of all, and drops the website and the
    // billing. Only her own website guess goes; the person's choices stand.
    attribute(&mut connection, item, &[guess(launch, 0.95)], now)
        .await
        .expect("attributed");
    assert_eq!(
        held(&mut connection, item).await,
        (
            vec![(billing, 1.0, format!("user:{TEST_PERSON_ID}"))],
            Some(billing)
        )
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_lead_is_one_of_the_items_projects() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let launch = project(&mut connection, "Launch").await;
    let item = arrived(&mut connection).await;
    let now = Utc::now();

    assert!(matches!(
        set_lead(&mut connection, item, website, TEST_PERSON, now).await,
        Err(WorkError::Invalid(_))
    ));
    // Its first project leads it, and the next one joins beside it.
    action_item::add_project(&mut connection, item, website, TEST_PERSON, now)
        .await
        .expect("added");
    action_item::add_project(&mut connection, item, launch, TEST_PERSON, now)
        .await
        .expect("added");
    assert_eq!(held(&mut connection, item).await.1, Some(website));
    // Making the lead the lead again records nothing.
    let unchanged = set_lead(&mut connection, item, website, TEST_PERSON, now)
        .await
        .expect("led");
    assert!(unchanged.history.is_empty());

    // Leaving its last project leaves it led by none.
    action_item::remove_project(&mut connection, item, website, TEST_PERSON, now)
        .await
        .expect("removed");
    let left = action_item::remove_project(&mut connection, item, launch, TEST_PERSON, now)
        .await
        .expect("removed");
    assert_eq!(left.record.lead_project_id, None);
    let moved = latest_entry(&mut connection, item, "lead_project_changed").await;
    assert_eq!(
        moved.data,
        json!({ "from": launch, "to": null, "passedOn": true })
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn every_correction_can_be_undone_while_it_still_holds() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let launch = project(&mut connection, "Launch").await;
    let item = arrived(&mut connection).await;
    let now = Utc::now();
    attribute(
        &mut connection,
        item,
        &[guess(website, 0.9), guess(launch, 0.5)],
        now,
    )
    .await
    .expect("attributed");

    // Undoing the person's removal of Elysia's guess puts it back as she held it.
    action_item::remove_project(&mut connection, item, launch, TEST_PERSON, now)
        .await
        .expect("removed");
    let removal = latest_entry(&mut connection, item, "project_removed").await;
    let restored = undo(&mut connection, item, removal.id, TEST_PERSON, now)
        .await
        .expect("undone");
    assert_eq!(restored.history[0].data["undid"], json!(removal.id));
    assert!(
        held(&mut connection, item)
            .await
            .0
            .contains(&(launch, 0.5, "elysia".to_owned()))
    );
    // It is in the project again, so undoing the removal a second time would overwrite that.
    assert!(matches!(
        undo(&mut connection, item, removal.id, TEST_PERSON, now).await,
        Err(WorkError::Conflict(_))
    ));
    // Put back by the person, the guess is theirs to keep: Elysia's next reading leaves it.
    attribute(&mut connection, item, &[guess(website, 0.9)], now)
        .await
        .expect("attributed");
    assert_eq!(held(&mut connection, item).await.0.len(), 2);

    // Undoing a lead change moves it back.
    set_lead(&mut connection, item, launch, TEST_PERSON, now)
        .await
        .expect("led");
    let moved = latest_entry(&mut connection, item, "lead_project_changed").await;
    undo(&mut connection, item, moved.id, TEST_PERSON, now)
        .await
        .expect("undone");
    assert_eq!(held(&mut connection, item).await.1, Some(website));
    // The lead moved since, so the same undo is refused.
    assert!(matches!(
        undo(&mut connection, item, moved.id, TEST_PERSON, now).await,
        Err(WorkError::Conflict(_))
    ));

    // Undoing Elysia's own addition takes the item off the project, and the lead passes on.
    let added = list_for_item(&mut connection, item)
        .await
        .expect("history")
        .into_iter()
        .find(|entry| entry.kind == "project_added" && entry.data["projectId"] == json!(website))
        .expect("her addition");
    let undone = undo(&mut connection, item, added.id, TEST_PERSON, now)
        .await
        .expect("undone");
    assert_eq!(undone.record.lead_project_id, Some(launch));

    // Only a change to its projects or its lead is undone here, and only the item's own.
    let created = latest_entry(&mut connection, item, "created").await;
    assert!(matches!(
        undo(&mut connection, item, created.id, TEST_PERSON, now).await,
        Err(WorkError::Invalid(_))
    ));
    let other = arrived(&mut connection).await;
    assert!(matches!(
        undo(&mut connection, other, removal.id, TEST_PERSON, now).await,
        Err(WorkError::Database(diesel::result::Error::NotFound))
    ));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_lead_a_person_chose_keeps_its_project_through_elysias_next_reading() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let launch = project(&mut connection, "Launch").await;
    let item = arrived(&mut connection).await;
    let now = Utc::now();
    attribute(
        &mut connection,
        item,
        &[guess(website, 0.9), guess(launch, 0.5)],
        now,
    )
    .await
    .expect("attributed");

    // The person moves only the lead, onto her less likely guess.
    set_lead(&mut connection, item, launch, TEST_PERSON, now)
        .await
        .expect("led");
    // A reading that drops the launch leaves it on the item, still leading it.
    attribute(&mut connection, item, &[guess(website, 0.9)], now)
        .await
        .expect("attributed");
    let (projects, lead) = held(&mut connection, item).await;
    assert_eq!(lead, Some(launch));
    assert!(
        projects
            .iter()
            .any(|(project_id, _, _)| *project_id == launch)
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_lead_passes_to_the_surest_project_left() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let launch = project(&mut connection, "Launch").await;
    let billing = project(&mut connection, "Billing").await;
    let item = arrived(&mut connection).await;
    let now = Utc::now();
    // Joined in one transaction, so the three share one moment and only confidence orders them.
    attribute(
        &mut connection,
        item,
        &[
            guess(billing, 0.35),
            guess(website, 0.9),
            guess(launch, 0.8),
        ],
        now,
    )
    .await
    .expect("attributed");

    let left = action_item::remove_project(&mut connection, item, website, TEST_PERSON, now)
        .await
        .expect("removed");
    assert_eq!(left.record.lead_project_id, Some(launch));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_undo_never_overwrites_a_later_change_that_put_things_back() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let item = arrived(&mut connection).await;
    let now = Utc::now();
    action_item::add_project(&mut connection, item, website, TEST_PERSON, now)
        .await
        .expect("added");
    let first = latest_entry(&mut connection, item, "project_added").await;
    action_item::remove_project(&mut connection, item, website, TEST_PERSON, now)
        .await
        .expect("removed");
    action_item::add_project(&mut connection, item, website, TEST_PERSON, now)
        .await
        .expect("added again");

    // The item is in the project as the first addition left it, but by a later change.
    assert!(matches!(
        undo(&mut connection, item, first.id, TEST_PERSON, now).await,
        Err(WorkError::Conflict(_))
    ));
    assert_eq!(held(&mut connection, item).await.0.len(), 1);
    // The latest addition is still undone.
    let latest = latest_entry(&mut connection, item, "project_added").await;
    undo(&mut connection, item, latest.id, TEST_PERSON, now)
        .await
        .expect("undone");
    assert!(held(&mut connection, item).await.0.is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_lead_that_passed_on_is_nobodys_choice_and_her_next_reading_moves_it() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let launch = project(&mut connection, "Launch").await;
    let billing = project(&mut connection, "Billing").await;
    let item = arrived(&mut connection).await;
    let now = Utc::now();
    attribute(
        &mut connection,
        item,
        &[guess(website, 0.9), guess(launch, 0.5)],
        now,
    )
    .await
    .expect("attributed");

    // Taking it off the website passes the lead to the launch, which the person never chose.
    action_item::remove_project(&mut connection, item, website, TEST_PERSON, now)
        .await
        .expect("removed");
    let passed = latest_entry(&mut connection, item, "lead_project_changed").await;
    assert_eq!(passed.data["passedOn"], json!(true));

    // So a fuller reading that drops the launch for the billing moves the lead with it.
    attribute(&mut connection, item, &[guess(billing, 0.95)], now)
        .await
        .expect("attributed");
    assert_eq!(
        held(&mut connection, item).await,
        (vec![(billing, 0.95, "elysia".to_owned())], Some(billing))
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_item_finished_while_she_read_it_is_not_attributed() {
    let (_url, mut connection) = migrated_database().await;
    let website = project(&mut connection, "Website").await;
    let item = arrived(&mut connection).await;
    let now = Utc::now();
    action_item::transition(
        &mut connection,
        item,
        crate::action_items::Transition::Resolve,
        TEST_PERSON,
        now,
    )
    .await
    .expect("resolved");

    assert!(matches!(
        attribute(&mut connection, item, &[guess(website, 0.9)], now).await,
        Err(WorkError::Conflict(_))
    ));
    assert_eq!(held(&mut connection, item).await, (Vec::new(), None));
}
