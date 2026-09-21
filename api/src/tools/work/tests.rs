// Copyright © 2026 Jalapeno Labs

//! The work tools: their arguments against their schemas, and each tool's query against a
//! real Postgres, scoped to one project while another holds work it must never reach.

use chrono::TimeDelta;
use serde_json::json;

use super::*;
use crate::action_items::Transition;
use crate::models::action_item::{ActionItemChanges, ActionItemPriority, NewActionItem, Owner};
use crate::models::initiative::NewInitiative;
use crate::models::project::NewProject;
use crate::test_support::migrated_database;

#[test]
fn every_schema_property_is_an_argument_the_tool_reads() {
    let id = Uuid::nil();
    let everything = [
        ("work_project", json!({})),
        (
            "work_items",
            json!({
                "states": ["open", "resolved"],
                "initiativeId": id,
                "waiting": false,
                "search": "login",
                "limit": 10,
            }),
        ),
        ("work_item", json!({ "itemId": id })),
        ("work_initiatives", json!({ "states": ["achieved"] })),
        ("work_initiative", json!({ "initiativeId": id })),
        (
            "work_comment",
            json!({ "itemId": id, "body": "Opened #12." }),
        ),
        (
            "work_link_pull_request",
            json!({ "url": "https://github.com/JalapenoLabs/Elysium/pull/12" }),
        ),
    ];
    assert_eq!(everything.len(), TOOLS.len(), "every tool has an example");
    for (name, arguments) in everything {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.name == name)
            .expect("a tool by that name");
        let schema = (tool.input_schema)();
        let properties = schema["properties"].as_object().expect("properties");
        let given = arguments.as_object().expect("object");
        assert_eq!(
            properties.len(),
            given.len(),
            "{name}: the example sets every property"
        );

        let text = arguments.to_string();
        let parsed = match name {
            "work_project" => parse_arguments::<NoArguments>(&text).map(|_parsed| ()),
            "work_items" => parse_arguments::<ItemsArguments>(&text).map(|_parsed| ()),
            "work_item" => parse_arguments::<ItemArguments>(&text).map(|_parsed| ()),
            "work_initiatives" => parse_arguments::<InitiativesArguments>(&text).map(|_parsed| ()),
            "work_initiative" => parse_arguments::<InitiativeArguments>(&text).map(|_parsed| ()),
            "work_link_pull_request" => {
                parse_arguments::<LinkPullRequestArguments>(&text).map(|_parsed| ())
            }
            _ => parse_arguments::<CommentArguments>(&text).map(|_parsed| ()),
        };
        parsed.unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn arguments_refuse_unknown_states_and_snake_case_names() {
    parse_arguments::<ItemsArguments>(r#"{"states":["archived"]}"#)
        .expect_err("archived is not an item state");
    parse_arguments::<ItemArguments>(&format!(r#"{{"item_id":"{}"}}"#, Uuid::nil()))
        .expect_err("names are camelCase, as the schema says");
    parse_arguments::<CommentArguments>(&format!(r#"{{"itemId":"{}"}}"#, Uuid::nil()))
        .expect_err("a comment needs a body");
}

/// Two projects: the session's, and another whose work it must never reach.
struct Fixture {
    scope: WorkScope,
    other_project: Uuid,
}

async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    let new_project = NewProject {
        name: name.to_owned(),
        description: format!("{name}'s description"),
    };
    project::create(connection, &new_project)
        .await
        .expect("project")
        .id
}

async fn fixture(connection: &mut AsyncPgConnection) -> Fixture {
    let project_id = project_named(connection, "Elysium").await;
    let other_project = project_named(connection, "Farworlds").await;
    Fixture {
        scope: WorkScope {
            session_id: 7,
            project_id,
            action_item_id: None,
        },
        other_project,
    }
}

async fn item_in(
    connection: &mut AsyncPgConnection,
    title: &str,
    project_ids: Vec<Uuid>,
    initiative_ids: Vec<Uuid>,
) -> ActionItem {
    let new_item = NewActionItem {
        title: title.to_owned(),
        notes: format!("Notes about {title}."),
        state: ActionItemState::Open,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids,
        initiative_ids,
    };
    // Items created a moment apart list newest first in a stable order.
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    action_item::create(connection, new_item, Actor::User, Utc::now())
        .await
        .expect("item")
        .record
}

async fn initiative_in(
    connection: &mut AsyncPgConnection,
    name: &str,
    project_ids: Vec<Uuid>,
) -> Initiative {
    let new_initiative = NewInitiative {
        name: name.to_owned(),
        description: format!("Why {name} matters."),
        target_at: None,
        project_ids,
    };
    initiative::create(connection, new_initiative, Actor::User, Utc::now())
        .await
        .expect("initiative")
        .record
}

fn titles(items: &Value) -> Vec<&str> {
    items
        .as_array()
        .expect("a list of items")
        .iter()
        .map(|item| item["title"].as_str().expect("a title"))
        .collect()
}

fn refusal(failure: Failure) -> ToolError {
    match failure {
        Failure::Refused(error) => error,
        Failure::Database(error) => panic!("expected a refusal, got {error}"),
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn items_list_only_the_projects_own_and_filter_as_asked() {
    let (_url, mut connection) = migrated_database().await;
    let Fixture {
        scope,
        other_project,
    } = fixture(&mut connection).await;
    let launch = initiative_in(&mut connection, "Launch", vec![scope.project_id]).await;

    let login = item_in(
        &mut connection,
        "Fix the login bug",
        vec![scope.project_id],
        vec![launch.id],
    )
    .await;
    let docs = item_in(
        &mut connection,
        "Write the docs",
        vec![scope.project_id],
        vec![],
    )
    .await;
    item_in(&mut connection, "Elsewhere", vec![other_project], vec![]).await;
    item_in(&mut connection, "Nowhere", vec![], vec![]).await;
    let resolved = item_in(&mut connection, "Old work", vec![scope.project_id], vec![]).await;
    action_item::transition(
        &mut connection,
        resolved.id,
        Transition::Resolve,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("resolve");
    let waiting = ActionItemChanges {
        waiting_on: Some(Some("sam@example.com".to_owned())),
        ..ActionItemChanges::default()
    };
    action_item::update(&mut connection, docs.id, waiting, Actor::User, Utc::now())
        .await
        .expect("wait");

    let now = Utc::now();
    let open = list_items(&mut connection, scope, ItemsArguments::default(), now)
        .await
        .expect("list");
    assert_eq!(
        titles(&open["items"]),
        ["Write the docs", "Fix the login bug"]
    );
    assert_eq!(open["moreItems"], false);
    assert_eq!(open["items"][1]["initiativeIds"], json!([launch.id]));
    assert!(
        open["items"][0].get("notes").is_none(),
        "lists leave notes out"
    );

    let every_state = ItemsArguments {
        states: Some(vec![ActionItemState::Open, ActionItemState::Resolved]),
        ..ItemsArguments::default()
    };
    let every = list_items(&mut connection, scope, every_state, now)
        .await
        .expect("list");
    assert_eq!(
        titles(&every["items"]),
        ["Old work", "Write the docs", "Fix the login bug"]
    );

    let searched = ItemsArguments {
        search: Some("LOGIN".to_owned()),
        ..ItemsArguments::default()
    };
    let searched = list_items(&mut connection, scope, searched, now)
        .await
        .expect("list");
    assert_eq!(titles(&searched["items"]), ["Fix the login bug"]);

    let by_notes = ItemsArguments {
        search: Some("about write".to_owned()),
        ..ItemsArguments::default()
    };
    let by_notes = list_items(&mut connection, scope, by_notes, now)
        .await
        .expect("list");
    assert_eq!(titles(&by_notes["items"]), ["Write the docs"]);

    let not_waiting = ItemsArguments {
        waiting: Some(false),
        ..ItemsArguments::default()
    };
    let not_waiting = list_items(&mut connection, scope, not_waiting, now)
        .await
        .expect("list");
    assert_eq!(titles(&not_waiting["items"]), ["Fix the login bug"]);

    let in_launch = ItemsArguments {
        initiative_id: Some(launch.id),
        ..ItemsArguments::default()
    };
    let in_launch = list_items(&mut connection, scope, in_launch, now)
        .await
        .expect("list");
    assert_eq!(titles(&in_launch["items"]), ["Fix the login bug"]);

    assert_ne!(login.id, docs.id);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn item_lists_stop_at_their_limit_and_say_more_matched() {
    let (_url, mut connection) = migrated_database().await;
    let Fixture { scope, .. } = fixture(&mut connection).await;
    item_in(&mut connection, "Older", vec![scope.project_id], vec![]).await;
    item_in(&mut connection, "Newer", vec![scope.project_id], vec![]).await;

    let one = ItemsArguments {
        limit: Some(1),
        ..ItemsArguments::default()
    };
    let one = list_items(&mut connection, scope, one, Utc::now())
        .await
        .expect("list");
    assert_eq!(titles(&one["items"]), ["Newer"]);
    assert_eq!(one["moreItems"], true);

    let too_many = ItemsArguments {
        limit: Some(ITEMS_MAX + 1),
        ..ItemsArguments::default()
    };
    let refused = list_items(&mut connection, scope, too_many, Utc::now())
        .await
        .expect_err("over the limit");
    assert!(matches!(refusal(refused), ToolError::Invalid(_)));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_item_shows_in_full_only_while_it_is_live_and_in_the_project() {
    let (_url, mut connection) = migrated_database().await;
    let Fixture {
        scope,
        other_project,
    } = fixture(&mut connection).await;
    let launch = initiative_in(&mut connection, "Launch", vec![scope.project_id]).await;
    let login = item_in(
        &mut connection,
        "Fix the login bug",
        vec![scope.project_id, other_project],
        vec![launch.id],
    )
    .await;
    action_item_comment::create(
        &mut connection,
        login.id,
        "Seen on Safari.".to_owned(),
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("comment");

    let detail = item_detail(&mut connection, scope, login.id, Utc::now())
        .await
        .expect("detail");
    let item = &detail["item"];
    assert_eq!(item["title"], "Fix the login bug");
    assert_eq!(item["notes"], "Notes about Fix the login bug.");
    assert_eq!(item["comments"][0]["body"], "Seen on Safari.");
    assert_eq!(item["comments"][0]["author"], "user");
    let project_names: Vec<&str> = item["projects"]
        .as_array()
        .expect("projects")
        .iter()
        .map(|project| project["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(project_names, ["Elysium", "Farworlds"]);
    assert_eq!(item["initiatives"][0]["name"], "Launch");
    assert_eq!(
        item["initiatives"][0]["progress"],
        json!({ "resolved": 0, "total": 1 })
    );
    let kinds: Vec<&str> = item["history"]
        .as_array()
        .expect("history")
        .iter()
        .map(|entry| entry["kind"].as_str().expect("a kind"))
        .collect();
    assert_eq!(kinds.first(), Some(&"created"), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&"commented"), "{kinds:?}");
    assert_eq!(item["historyCount"], json!(kinds.len()));

    // Checked against the database on every call: an item taken out of the project after
    // the session started is out of reach.
    action_item::remove_project(
        &mut connection,
        login.id,
        scope.project_id,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("remove");
    let moved = item_detail(&mut connection, scope, login.id, Utc::now())
        .await
        .expect_err("no longer in the project");
    assert_eq!(refusal(moved), ToolError::ItemUnavailable(login.id));

    let deleted = item_in(&mut connection, "Gone", vec![scope.project_id], vec![]).await;
    action_item::soft_delete(&mut connection, deleted.id, Actor::User, Utc::now())
        .await
        .expect("delete");
    let refused = item_detail(&mut connection, scope, deleted.id, Utc::now())
        .await
        .expect_err("deleted");
    assert_eq!(refusal(refused), ToolError::ItemUnavailable(deleted.id));

    let unknown = Uuid::now_v7();
    let refused = item_detail(&mut connection, scope, unknown, Utc::now())
        .await
        .expect_err("no such item");
    assert_eq!(refusal(refused), ToolError::ItemUnavailable(unknown));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn initiatives_list_and_show_only_the_projects_own() {
    let (_url, mut connection) = migrated_database().await;
    let Fixture {
        scope,
        other_project,
    } = fixture(&mut connection).await;
    let launch = initiative_in(&mut connection, "Launch", vec![scope.project_id]).await;
    let finished = initiative_in(&mut connection, "Beta", vec![scope.project_id]).await;
    let achieved = initiative::InitiativeChanges {
        state: Some(InitiativeState::Achieved),
        ..initiative::InitiativeChanges::default()
    };
    initiative::update(
        &mut connection,
        finished.id,
        achieved,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("achieve");
    let elsewhere = initiative_in(&mut connection, "Elsewhere", vec![other_project]).await;

    let ours = item_in(
        &mut connection,
        "Ours",
        vec![scope.project_id],
        vec![launch.id],
    )
    .await;
    item_in(
        &mut connection,
        "Theirs",
        vec![other_project],
        vec![launch.id],
    )
    .await;
    action_item::transition(
        &mut connection,
        ours.id,
        Transition::Resolve,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("resolve");

    let now = Utc::now();
    let active = list_initiatives(&mut connection, scope, InitiativesArguments::default(), now)
        .await
        .expect("list");
    assert_eq!(active["initiatives"].as_array().expect("list").len(), 1);
    assert_eq!(active["initiatives"][0]["name"], "Launch");
    assert_eq!(
        active["initiatives"][0]["progress"],
        json!({ "resolved": 1, "total": 2 }),
        "progress counts every member, in any project"
    );

    let achieved_only = InitiativesArguments {
        states: Some(vec![InitiativeState::Achieved]),
    };
    let achieved_only = list_initiatives(&mut connection, scope, achieved_only, now)
        .await
        .expect("list");
    assert_eq!(achieved_only["initiatives"][0]["name"], "Beta");

    let detail = initiative_detail(&mut connection, scope, launch.id, now)
        .await
        .expect("detail");
    let initiative = &detail["initiative"];
    assert_eq!(initiative["description"], "Why Launch matters.");
    assert_eq!(
        titles(&initiative["items"]),
        ["Ours"],
        "only the project's own members are shown"
    );
    assert_eq!(initiative["itemsOutsideProject"], 1);
    assert_eq!(initiative["moreItems"], false);

    let refused = initiative_detail(&mut connection, scope, elsewhere.id, now)
        .await
        .expect_err("another project's initiative");
    assert_eq!(
        refusal(refused),
        ToolError::InitiativeUnavailable(elsewhere.id)
    );

    initiative::soft_delete(&mut connection, launch.id, Actor::User, Utc::now())
        .await
        .expect("delete");
    let refused = initiative_detail(&mut connection, scope, launch.id, Utc::now())
        .await
        .expect_err("deleted");
    assert_eq!(
        refusal(refused),
        ToolError::InitiativeUnavailable(launch.id)
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_project_overview_names_the_item_the_session_started_from() {
    let (_url, mut connection) = migrated_database().await;
    let Fixture {
        mut scope,
        other_project,
    } = fixture(&mut connection).await;
    let launch = initiative_in(&mut connection, "Launch", vec![scope.project_id]).await;
    initiative_in(&mut connection, "Elsewhere", vec![other_project]).await;
    let login = item_in(
        &mut connection,
        "Fix the login bug",
        vec![scope.project_id],
        vec![launch.id],
    )
    .await;
    item_in(&mut connection, "Elsewhere", vec![other_project], vec![]).await;
    scope.action_item_id = Some(login.id);

    let overview = project_overview(&mut connection, scope, Utc::now())
        .await
        .expect("overview");
    assert_eq!(overview["project"]["name"], "Elysium");
    assert_eq!(overview["project"]["description"], "Elysium's description");
    assert_eq!(overview["sessionItemId"], json!(login.id));
    assert_eq!(titles(&overview["items"]), ["Fix the login bug"]);
    assert_eq!(overview["moreItems"], false);
    let initiative_names: Vec<&str> = overview["initiatives"]
        .as_array()
        .expect("initiatives")
        .iter()
        .map(|initiative| initiative["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(initiative_names, ["Launch"]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn comments_are_written_as_the_session_on_the_projects_items_only() {
    let (_url, mut connection) = migrated_database().await;
    let Fixture {
        scope,
        other_project,
    } = fixture(&mut connection).await;
    let login = item_in(
        &mut connection,
        "Fix the login bug",
        vec![scope.project_id],
        vec![],
    )
    .await;
    let theirs = item_in(&mut connection, "Theirs", vec![other_project], vec![]).await;

    let comment = |item_id: Uuid, body: &str| CommentArguments {
        item_id,
        body: body.to_owned(),
    };
    let written = comment_on_item(
        &mut connection,
        scope,
        comment(login.id, "  Opened #12 with the fix.\n"),
        Utc::now(),
    )
    .await
    .expect("comment");
    assert_eq!(written.record.author, "session:7");
    assert_eq!(written.record.body, "Opened #12 with the fix.");
    assert_eq!(written.history.len(), 1);
    assert_eq!(written.history[0].actor, "session:7");
    assert_eq!(written.history[0].kind, "commented");

    let refused = comment_on_item(
        &mut connection,
        scope,
        comment(theirs.id, "Not mine to comment on."),
        Utc::now(),
    )
    .await
    .expect_err("another project's item");
    assert_eq!(refusal(refused), ToolError::ItemUnavailable(theirs.id));

    let blank = comment_on_item(
        &mut connection,
        scope,
        comment(login.id, " \n "),
        Utc::now(),
    )
    .await
    .expect_err("blank");
    assert!(matches!(refusal(blank), ToolError::Invalid(_)));

    let comments = action_item_comment::list(&mut connection, theirs.id)
        .await
        .expect("comments");
    assert!(comments.is_empty(), "a refused comment writes nothing");

    // The session's own comment is still the session's: the user cannot edit it.
    let edit = action_item_comment::update(
        &mut connection,
        login.id,
        written.record.id,
        "Rewritten".to_owned(),
        Actor::User,
        Utc::now() + TimeDelta::seconds(1),
    )
    .await;
    assert!(matches!(edit, Err(WorkError::Conflict(_))));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_items_history_keeps_its_latest_entries_and_counts_them_all() {
    let (_url, mut connection) = migrated_database().await;
    let Fixture { scope, .. } = fixture(&mut connection).await;
    let item = item_in(&mut connection, "Busy", vec![scope.project_id], vec![]).await;
    for index in 0..HISTORY_MAX {
        action_item_comment::create(
            &mut connection,
            item.id,
            format!("Update {index}"),
            Actor::User,
            Utc::now(),
        )
        .await
        .expect("comment");
    }

    let detail = item_detail(&mut connection, scope, item.id, Utc::now())
        .await
        .expect("detail");
    let history = detail["item"]["history"].as_array().expect("history");
    assert_eq!(history.len(), HISTORY_MAX);
    assert_eq!(detail["item"]["historyCount"], json!(HISTORY_MAX + 1));
    assert_eq!(
        history[0]["kind"], "commented",
        "the oldest entry, the item's creation, is the one left out"
    );
    assert_eq!(
        detail["item"]["comments"]
            .as_array()
            .expect("comments")
            .len(),
        HISTORY_MAX,
        "every comment is shown"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn items_cannot_be_filtered_on_another_projects_initiative() {
    let (_url, mut connection) = migrated_database().await;
    let Fixture {
        scope,
        other_project,
    } = fixture(&mut connection).await;
    let elsewhere = initiative_in(&mut connection, "Elsewhere", vec![other_project]).await;
    // Ours, and in their initiative: the filter must not become a way to read it.
    item_in(
        &mut connection,
        "Ours",
        vec![scope.project_id],
        vec![elsewhere.id],
    )
    .await;

    let filtered = ItemsArguments {
        initiative_id: Some(elsewhere.id),
        ..ItemsArguments::default()
    };
    let refused = list_items(&mut connection, scope, filtered, Utc::now())
        .await
        .expect_err("another project's initiative");
    assert_eq!(
        refusal(refused),
        ToolError::InitiativeUnavailable(elsewhere.id)
    );
}

/// Records a session of `scope.project_id` on a new satellite, with `github_credential_id`
/// as its token, and answers the scope with the session's number.
async fn session_with(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    github_credential_id: Option<Uuid>,
) -> WorkScope {
    use secrecy::SecretString;

    use crate::models::coding_session::NewCodingSession;
    use crate::models::satellite::{self, NewSatellite};
    use crate::test_support::cipher;

    let satellite = satellite::create(
        connection,
        &cipher(),
        &NewSatellite {
            name: format!("Satellite {}", Uuid::now_v7()),
            description: String::new(),
            url: "http://arsox:8080".to_owned(),
            secret: SecretString::from("bearer"),
            is_active: true,
        },
    )
    .await
    .expect("satellite");
    let id = coding_session::reserve_id(connection)
        .await
        .expect("a number");
    let session = coding_session::create(
        connection,
        &NewCodingSession {
            id,
            project_id: scope.project_id,
            satellite_id: satellite.id,
            thread_id: format!("thread-{}", Uuid::now_v7()),
            title: "Fix it".to_owned(),
            github_credential_id,
            action_item_id: scope.action_item_id,
        },
    )
    .await
    .expect("session");
    WorkScope {
        session_id: session.id,
        ..scope
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_pull_request_links_only_to_the_sessions_own_item_through_its_token() {
    use secrecy::SecretString;

    use crate::models::github_credential::{self, GithubTokenKind, NewGithubCredential};
    use crate::test_support::cipher;

    let (_url, mut connection) = migrated_database().await;
    let Fixture { scope, .. } = fixture(&mut connection).await;
    let item = item_in(
        &mut connection,
        "Fix the login bug",
        vec![scope.project_id],
        vec![],
    )
    .await;
    let token = github_credential::create(
        &mut connection,
        &cipher(),
        &NewGithubCredential {
            name: "Personal".to_owned(),
            verified: github_credential::VerifiedToken {
                kind: GithubTokenKind::Classic,
                token: SecretString::from("ghp_token"),
                account: crate::github::Account {
                    login: "alex".to_owned(),
                    scopes: Vec::new(),
                    token_expires_at: None,
                },
            },
        },
    )
    .await
    .expect("a token")
    .id;

    let without_item = session_with(&mut connection, scope, Some(token)).await;
    let refused = pull_request_target(&mut connection, without_item)
        .await
        .expect_err("no item to link to");
    let ToolError::Invalid(message) = refusal(refused) else {
        panic!("a session without an item is told why");
    };
    assert!(message.contains("work_comment"), "{message}");

    let from_item = WorkScope {
        action_item_id: Some(item.id),
        ..scope
    };
    let without_token = session_with(&mut connection, from_item, None).await;
    let refused = pull_request_target(&mut connection, without_token)
        .await
        .expect_err("no token to read the pull request with");
    let ToolError::Invalid(message) = refusal(refused) else {
        panic!("a session without a token is told why");
    };
    assert!(message.contains("GitHub token"), "{message}");

    let linked = session_with(&mut connection, from_item, Some(token)).await;
    let target = pull_request_target(&mut connection, linked)
        .await
        .expect("a target");
    assert_eq!(target, (item.id, token));

    action_item::soft_delete(&mut connection, item.id, Actor::User, Utc::now())
        .await
        .expect("deleted");
    let refused = pull_request_target(&mut connection, linked)
        .await
        .expect_err("a deleted item");
    assert_eq!(refusal(refused), ToolError::ItemUnavailable(item.id));
}
