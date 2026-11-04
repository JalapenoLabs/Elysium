// Copyright © 2026 Jalapeno Labs

//! Merging against Postgres and the GitHub fake: a ready pull request in a repository Elysium
//! merges is merged once, naming its head, and put on the timeline waking the Project Manager;
//! one that cannot be read or is not ready is held saying why; one a person merges is left to
//! them and recorded as theirs once they do; and a pull request is one batch's.

use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde_json::json;

use super::*;
use crate::database::schema::{project_manager_wakes, timeline_record_projects, timeline_records};
use crate::github::fake::{self, FakeGithub};
use crate::models::project::{self, NewProject};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::pull_request::{NewPullRequest, RecordError};
use crate::models::work_batch::{self, NewWorkBatch, Priority, assign, start, submit};
use crate::models::worker_setup::Declaration;
use crate::test_support::{TEST_PERSON_ID, migrated_database};

const REPOSITORY: &str = "JalapenoLabs/web";

/// A running batch, a project that declares `REPOSITORY` under `policy`, the GitHub fake, and
/// what looks at the batch's pull requests.
struct Fixture {
    connection: AsyncPgConnection,
    github: FakeGithub,
    merging: Merging,
    batch: WorkBatch,
}

impl Fixture {
    async fn new(policy: MergePolicy) -> Self {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("Postgres accepts connections");
        let batch = running_batch(&mut connection, "Web").await;
        worker_setup::replace(
            &mut connection,
            batch.project_id,
            None,
            &[Declaration {
                url: format!("https://github.com/{REPOSITORY}.git"),
                base_branch: None,
                setup_script: None,
                merge_policy: policy,
            }],
            TEST_PERSON_ID,
        )
        .await
        .expect("declared");
        let github = FakeGithub::start().await;
        let merging = Merging::new(github.github.clone(), database, EventBus::in_process());
        Self {
            connection,
            github,
            merging,
            batch,
        }
    }

    /// Records `REPOSITORY#number`, which the fake holds ready, for the batch.
    async fn opened(&mut self, number: u64) -> BatchPullRequest {
        let url = format!("https://github.com/{REPOSITORY}/pull/{number}");
        self.github.put_pull_request_node(
            REPOSITORY,
            number,
            fake::ready_pull_request_node(REPOSITORY, number, "Fix the login page"),
        );
        pull_request::record(
            &mut self.connection,
            NewPullRequest {
                work_batch_id: self.batch.id,
                repository: REPOSITORY,
                number: i32::try_from(number).expect("small"),
                url: &url,
                title: "Fix the login page",
            },
        )
        .await
        .expect("recorded")
    }

    async fn look(&self) {
        self.merging
            .look(&self.batch, Some(&SecretString::from(fake::TOKEN)))
            .await;
    }

    async fn reread(&mut self, id: Uuid) -> BatchPullRequest {
        pull_request::of_batch(&mut self.connection, self.batch.id)
            .await
            .expect("reads")
            .into_iter()
            .find(|recorded| recorded.id == id)
            .expect("still recorded")
    }

    /// The merges the fake was asked for.
    fn merges(&self) -> Vec<serde_json::Value> {
        self.github
            .writes()
            .into_iter()
            .filter(|request| request.path.ends_with("/merge"))
            .map(|request| request.body)
            .collect()
    }

    async fn merge_records(&mut self) -> Vec<TimelineRecord> {
        timeline_records::table
            .filter(timeline_records::source_kind.eq(timeline::SOURCE_KIND))
            .select(TimelineRecord::as_select())
            .load(&mut self.connection)
            .await
            .expect("reads")
    }

    async fn wakes_for(&mut self, record_id: Uuid) -> i64 {
        project_manager_wakes::table
            .filter(project_manager_wakes::project_id.eq(self.batch.project_id))
            .filter(project_manager_wakes::timeline_record_id.eq(record_id))
            .count()
            .get_result(&mut self.connection)
            .await
            .expect("counted")
    }
}

async fn running_batch(connection: &mut AsyncPgConnection, name: &str) -> WorkBatch {
    let project_id = project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id;
    let batch = submit(
        connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id,
            title: "Fix the login page".to_owned(),
            goal: "Passkeys fail on the login page; fix them.".to_owned(),
            repositories: json!([{ "url": format!("https://github.com/{REPOSITORY}.git") }]),
            priority: Priority::Asap,
            scheduled_for: None,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await
    .expect("submitted");
    // A pool as large as a test ever fills, so a second batch gets a slot too.
    let slot = assign(connection, &[batch.id], 4, Utc::now())
        .await
        .expect("assigns")
        .remove(0);
    start(connection, &slot)
        .await
        .expect("runs")
        .expect("started")
}

#[test]
fn a_repository_merges_as_declared_and_an_undeclared_one_waits_for_a_person() {
    let setup = WorkerSetup {
        tools_setup_script: None,
        repositories: vec![crate::models::worker_setup::ProjectRepository {
            id: Uuid::nil(),
            project_id: Uuid::nil(),
            position: 0,
            url: "ssh://git@github.com/JalapenoLabs/Web.git".to_owned(),
            base_branch: None,
            setup_script: None,
            created_by: Uuid::nil(),
            created_at: Utc::now(),
            merge_policy: MergePolicy::Auto,
        }],
    };
    assert_eq!(
        policy_for(&setup, "https://github.com/jalapenolabs/web"),
        MergePolicy::Auto,
        "the same repository however its URL is written"
    );
    assert_eq!(
        policy_for(&setup, "https://github.com/JalapenoLabs/api"),
        MergePolicy::Person
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_ready_pull_request_is_merged_once_naming_its_head_and_wakes_the_project_manager() {
    let mut fixture = Fixture::new(MergePolicy::Auto).await;
    let recorded = fixture.opened(12).await;

    fixture.look().await;

    assert_eq!(
        fixture.merges(),
        [json!({ "merge_method": "squash", "sha": "head-of-12" })],
        "squashed, and only the head that was read"
    );
    let merged = fixture.reread(recorded.id).await;
    assert_eq!(merged.state, PullRequestState::Merged);
    assert!(merged.merged_by_elysium);
    assert_eq!(merged.merge_method, Some(MergeMethod::Squash));
    assert_eq!(merged.merged_by.as_deref(), Some(fake::LOGIN));
    assert_eq!(merged.held_because, None);

    let records = fixture.merge_records().await;
    let [record] = records.as_slice() else {
        panic!("one record, got {}", records.len());
    };
    assert_eq!(merged.timeline_record_id, Some(record.id));
    assert_eq!(record.url.as_deref(), Some(merged.url.as_str()));
    assert_eq!(record.actor, "Elysium");
    assert!(record.title.starts_with("Merged JalapenoLabs/web#12"));
    let attributed_by: String = timeline_record_projects::table
        .filter(timeline_record_projects::timeline_record_id.eq(record.id))
        .filter(timeline_record_projects::project_id.eq(fixture.batch.project_id))
        .select(timeline_record_projects::attributed_by)
        .first(&mut fixture.connection)
        .await
        .expect("attributed to the batch's project");
    assert_eq!(attributed_by, format!("worker:{}", fixture.batch.id));
    assert_eq!(fixture.wakes_for(record.id).await, 1);

    // A merged pull request is not looked at again, so nothing merges or records twice.
    fixture.look().await;
    assert_eq!(fixture.merges().len(), 1);
    assert_eq!(fixture.merge_records().await.len(), 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn what_cannot_be_read_or_is_not_ready_is_held_saying_why() {
    let mut fixture = Fixture::new(MergePolicy::Auto).await;
    let recorded = fixture.opened(7).await;

    fixture.github.fail_graphql(true);
    fixture.look().await;
    let held = fixture.reread(recorded.id).await;
    assert_eq!(held.state, PullRequestState::Open);
    let reason = held.held_because.expect("a reason");
    assert!(reason.starts_with("GitHub could not be read"), "{reason}");
    fixture.github.fail_graphql(false);

    fixture
        .github
        .change_pull_request_node(REPOSITORY, 7, |node| {
            node["reviewThreads"] = json!({ "totalCount": 2, "nodes": [
                { "isResolved": true }, { "isResolved": false },
            ] });
            node["commits"]["nodes"][0]["commit"]["statusCheckRollup"] = json!(null);
        });
    fixture.look().await;
    let held = fixture.reread(recorded.id).await;
    assert_eq!(
        held.held_because.as_deref(),
        Some("No check ran on its latest commit; 1 review thread is not resolved.")
    );

    fixture.merging.look(&fixture.batch, None).await;
    let held = fixture.reread(recorded.id).await;
    assert!(
        held.held_because
            .as_deref()
            .is_some_and(|reason| reason.contains("no GitHub token")),
        "{held:?}"
    );

    // Ready, but its branch merges through a merge queue, which Elysium never bypasses.
    fixture
        .github
        .change_pull_request_node(REPOSITORY, 7, |node| {
            *node = fake::ready_pull_request_node(REPOSITORY, 7, "Fix the login page");
        });
    fixture.github.put_branch_rules(
        REPOSITORY,
        json!([{ "type": "merge_queue", "parameters": { "merge_method": "SQUASH" } }]),
    );
    fixture.look().await;
    let queued = fixture.reread(recorded.id).await;
    assert!(
        queued
            .held_because
            .as_deref()
            .is_some_and(|reason| reason.contains("merge queue")),
        "{queued:?}"
    );
    assert!(fixture.merges().is_empty());
    fixture.github.put_branch_rules(REPOSITORY, json!([]));

    // A merge GitHub refuses stays open with its answer.
    fixture.github.refuse_writes(true);
    fixture.look().await;
    let refused = fixture.reread(recorded.id).await;
    assert_eq!(refused.state, PullRequestState::Open);
    assert!(
        refused
            .held_because
            .as_deref()
            .is_some_and(|reason| reason.starts_with("GitHub refused the merge")),
        "{refused:?}"
    );
    assert!(fixture.merge_records().await.is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_person_merges_where_the_repository_waits_and_their_merge_is_recorded() {
    let mut fixture = Fixture::new(MergePolicy::Person).await;
    let recorded = fixture.opened(3).await;
    let closed = fixture.opened(4).await;

    fixture.look().await;
    assert!(fixture.merges().is_empty(), "Elysium never merges here");
    let waiting = fixture.reread(recorded.id).await;
    assert_eq!(
        waiting.held_because.as_deref(),
        Some("A person merges pull requests in this repository.")
    );

    fixture
        .github
        .change_pull_request_node(REPOSITORY, 3, |node| {
            node["state"] = json!("MERGED");
            node["mergedAt"] = json!(Utc::now());
            node["mergedBy"] = json!({ "login": "octocat" });
        });
    fixture
        .github
        .change_pull_request_node(REPOSITORY, 4, |node| {
            node["state"] = json!("CLOSED");
        });
    fixture.look().await;

    let merged = fixture.reread(recorded.id).await;
    assert_eq!(merged.state, PullRequestState::Merged);
    assert_eq!(merged.merged_by.as_deref(), Some("octocat"));
    assert!(!merged.merged_by_elysium);
    assert_eq!(
        fixture.reread(closed.id).await.state,
        PullRequestState::Closed
    );
    let records = fixture.merge_records().await;
    let [record] = records.as_slice() else {
        panic!("one record, got {}", records.len());
    };
    assert_eq!(record.actor, "octocat");
    assert_eq!(fixture.wakes_for(record.id).await, 1);
    assert_eq!(
        fixture.github.pull_request_node(REPOSITORY, 3)["mergedBy"]["login"],
        "octocat",
        "the merge was theirs, untouched"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_pull_request_is_one_batchs_however_its_repository_is_spelled() {
    let mut fixture = Fixture::new(MergePolicy::Auto).await;
    let first = fixture.opened(9).await;

    let again = pull_request::record(
        &mut fixture.connection,
        NewPullRequest {
            work_batch_id: fixture.batch.id,
            repository: "jalapenolabs/WEB",
            number: 9,
            url: "https://github.com/JalapenoLabs/web/pull/9",
            title: "Fix the login page",
        },
    )
    .await
    .expect("the same batch names it again");
    assert_eq!(again.id, first.id);

    let other = running_batch(&mut fixture.connection, "Mobile").await;
    let refused = pull_request::record(
        &mut fixture.connection,
        NewPullRequest {
            work_batch_id: other.id,
            repository: REPOSITORY,
            number: 9,
            url: "https://github.com/JalapenoLabs/web/pull/9",
            title: "Fix the login page",
        },
    )
    .await
    .expect_err("another batch's");
    assert!(
        matches!(refused, RecordError::OtherBatch(batch) if batch == fixture.batch.id),
        "{refused:?}"
    );
    assert_eq!(
        work_batch::find(&mut fixture.connection, other.id)
            .await
            .expect("reads")
            .state,
        work_batch::BatchState::Running
    );
}
