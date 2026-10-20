// Copyright © 2026 Jalapeno Labs

use super::*;
use crate::activity::ActivityKind;
use crate::test_support::{TEST_PERSON_ID, migrated_database};

/// The same vector `updater/updater.sh` produces for `ELYSIUM_ENCRYPTION_KEY=testkey`:
/// `printf '%s' 'elysium/updater/key:testkey' | sha256sum`.
#[test]
fn the_updater_key_matches_the_updater_script() {
    let key = derive_updater_key(&SecretString::from("testkey"));
    assert_eq!(
        key.expose_secret(),
        "81a4c1f2dd411e3a59727e8fb427305aa48385f25c1173971f8c104196595174"
    );
}

#[test]
fn instructions_are_one_line_the_updater_reads() {
    assert_eq!(Instruction::Wait.to_string(), "wait");
    assert_eq!(
        Instruction::Apply(Version::new(0, 4, 2)).to_string(),
        "apply 0.4.2"
    );
}

#[test]
fn a_success_report_names_its_release() {
    let report = Report::parse("succeeded 0.4.2\n").expect("a valid report");
    assert_eq!(report.version, Version::new(0, 4, 2));
    assert_eq!(report.outcome, UpdateOutcome::Succeeded);
}

#[test]
fn a_rollback_report_keeps_its_whole_reason() {
    let report = Report::parse("rolled-back 0.4.2 the api did not become healthy within 300s")
        .expect("a valid report");
    assert_eq!(
        report.outcome,
        UpdateOutcome::RolledBack("the api did not become healthy within 300s".to_owned())
    );
}

#[test]
fn malformed_reports_are_refused() {
    for line in [
        "",
        "succeeded",
        "succeeded main",
        "finished 0.4.2",
        "rolled-back 0.4.2",
        "rolled-back 0.4.2   ",
    ] {
        assert!(Report::parse(line).is_err(), "{line:?} must be refused");
    }
}

#[test]
fn a_long_reason_is_cut_to_what_the_column_holds() {
    let line = format!(
        "rolled-back 0.4.2 {}",
        "x".repeat(MAX_REASON_CHARACTERS + 10)
    );
    let report = Report::parse(&line).expect("a valid report");
    let UpdateOutcome::RolledBack(reason) = report.outcome else {
        panic!("a rollback");
    };
    assert_eq!(reason.chars().count(), MAX_REASON_CHARACTERS);
}

/// Updates for a deployment running `running`, against a migrated database.
async fn updates_running(running: &'static str) -> (Updates, Activities) {
    let (url, _connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("Postgres accepts connections");
    let build = VersionInfo::from_build().expect("build metadata parses");
    let activity = Activities::new();
    let updates = Updates::new(UpdatesDeps {
        database,
        events: EventBus::new(),
        activity: activity.clone(),
        http: reqwest::Client::new(),
        version: Arc::new(VersionInfo {
            release: Some(running),
            ..build
        }),
        encryption_key: SecretString::from("testkey"),
    });
    (updates, activity)
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_newer_release_is_applied_once_and_recorded() {
    let (updates, _activity) = updates_running("0.1.0").await;
    updates.set_newest(Some(Version::new(0, 1, 1)));

    let first = updates.instruction().await.expect("an instruction");
    assert_eq!(first, Instruction::Apply(Version::new(0, 1, 1)));

    let second = updates.instruction().await.expect("an instruction");
    assert_eq!(second, Instruction::Wait, "one update at a time");

    let status = updates.status().await.expect("the status");
    assert_eq!(status.state, UpdateState::Applying);
    assert!(status.updater_connected);
    let applying = &status.history[0];
    assert_eq!(applying.from_version, "0.1.0");
    assert_eq!(applying.to_version, "0.1.1");
    assert_eq!(applying.status, DeploymentUpdateStatus::Applying);
    assert_eq!(applying.created_by, SYSTEM_USER_ID);

    updates
        .report(Report::parse("succeeded 0.1.1").expect("a valid report"))
        .await
        .expect("the report is recorded");
    let status = updates.status().await.expect("the status");
    assert_eq!(status.history[0].status, DeploymentUpdateStatus::Succeeded);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn running_work_holds_the_update_until_it_finishes() {
    let (updates, activity) = updates_running("0.1.0").await;
    updates.set_newest(Some(Version::new(0, 1, 1)));

    let turn = activity.begin(ActivityKind::AgentTurn, "session:1");
    assert_eq!(
        updates.instruction().await.expect("an instruction"),
        Instruction::Wait
    );
    let status = updates.status().await.expect("the status");
    assert_eq!(status.state, UpdateState::WaitingForWork);
    assert_eq!(status.waiting_for.len(), 1);

    drop(turn);
    assert_eq!(
        updates.instruction().await.expect("an instruction"),
        Instruction::Apply(Version::new(0, 1, 1))
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_release_that_rolled_back_waits_for_an_admin() {
    let (updates, _activity) = updates_running("0.1.0").await;
    updates.set_newest(Some(Version::new(0, 1, 1)));

    updates.instruction().await.expect("an instruction");
    updates
        .report(
            Report::parse("rolled-back 0.1.1 nginx never became healthy").expect("a valid report"),
        )
        .await
        .expect("the report is recorded");

    assert_eq!(
        updates.instruction().await.expect("an instruction"),
        Instruction::Wait
    );
    let status = updates.status().await.expect("the status");
    assert_eq!(status.state, UpdateState::HeldBack);
    assert_eq!(
        status.history[0].reason.as_deref(),
        Some("nginx never became healthy")
    );

    updates
        .request(TEST_PERSON_ID)
        .await
        .expect("an admin may ask for it");
    assert_eq!(
        updates.instruction().await.expect("an instruction"),
        Instruction::Apply(Version::new(0, 1, 1))
    );
    let status = updates.status().await.expect("the status");
    assert_eq!(status.history[0].created_by, TEST_PERSON_ID);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn starting_as_the_applying_release_settles_it_as_succeeded() {
    let (old_process, _activity) = updates_running("0.1.0").await;
    old_process.set_newest(Some(Version::new(0, 1, 1)));
    old_process.instruction().await.expect("an instruction");

    // The new release starts against the same database before the updater reports.
    let new_process = Updates::new(UpdatesDeps {
        version: Arc::new(VersionInfo {
            release: Some("0.1.1"),
            ..VersionInfo::from_build().expect("build metadata parses")
        }),
        ..old_process.deps_for_test()
    });
    new_process.settle_after_restart().await.expect("settled");

    let status = new_process.status().await.expect("the status");
    assert_eq!(status.release.as_deref(), Some("0.1.1"));
    assert_eq!(status.history[0].status, DeploymentUpdateStatus::Succeeded);

    // The updater's report arriving afterwards changes nothing.
    new_process
        .report(Report::parse("succeeded 0.1.1").expect("a valid report"))
        .await
        .expect("a late report is accepted");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_build_that_is_not_a_release_is_never_told_to_apply() {
    let (updates, _activity) = updates_running("main").await;
    updates.set_newest(Some(Version::new(9, 0, 0)));
    assert_eq!(
        updates.instruction().await.expect("an instruction"),
        Instruction::Wait
    );
    assert!(matches!(
        updates.request(Uuid::now_v7()).await,
        Err(RequestError::NotARelease)
    ));
}
