// Copyright © 2026 Jalapeno Labs

//! The migration that teaches the default guidance about pages (#205). A release never replaces
//! what an admin wrote, so it adds a version to the seed alone, and takes back only that.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretString;

use super::migrations::{self, MIGRATIONS, MigrationCommand};
use crate::agent_guidance::{self, Audience};
use crate::models::agent_guidance::{self as guidance, GuidanceVersion};
use crate::models::user::SYSTEM_USER_ID;
use crate::test_support::{TEST_PERSON_ID, migrated_database};

/// The migration under test.
const TEACH: &str = "2026-11-06-160000-0000_teach_agents_about_pages";

/// Rolls back [`TEACH`], however many migrations land after it.
async fn before_it(url: &SecretString) {
    let names: Vec<String> =
        diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .expect("embedded migrations load")
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
    let position = names
        .iter()
        .position(|name| name == TEACH)
        .expect("the migration is embedded");
    let count = u32::try_from(names.len() - position).expect("a handful of migrations");
    migrations::execute(url.clone(), MigrationCommand::Revert { count })
        .await
        .expect("it rolls back");
}

async fn apply(url: &SecretString) {
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("it applies");
}

/// Saves `content` as `version`, by the admin tests act as.
async fn admin_writes(connection: &mut AsyncPgConnection, version: i32, content: &str) {
    diesel::sql_query(format!(
        "INSERT INTO agent_guidance_versions (version, content, created_by) \
         VALUES ({version}, '{content}', '{TEST_PERSON_ID}')"
    ))
    .execute(connection)
    .await
    .expect("the admin's version is stored");
}

async fn every_version(connection: &mut AsyncPgConnection) -> Vec<GuidanceVersion> {
    guidance::versions(connection)
        .await
        .expect("the versions read")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_seed_gains_a_version_saying_when_to_write_a_page() {
    let (url, mut connection) = migrated_database().await;

    let current = guidance::current(&mut connection)
        .await
        .expect("a version exists");
    assert_eq!(current.version, 2);
    assert_eq!(current.created_by, SYSTEM_USER_ID);
    assert_eq!(current.restored_from, None);
    let seed = every_version(&mut connection)
        .await
        .pop()
        .expect("the seed is kept");
    assert_eq!(seed.version, 1);
    assert!(
        current.content.starts_with(&seed.content),
        "it keeps every word of the seed: {}",
        current.content
    );
    assert!(current.content.contains("page_save"), "{}", current.content);
    assert!(
        current.content.contains("rather than a learning"),
        "{}",
        current.content
    );

    let public_url = url::Url::parse("https://elysium.example.com").expect("a static URL parses");
    assert!(
        current.content.chars().count() <= agent_guidance::content_max_characters(&public_url),
        "every agent reads it whole"
    );
    for audience in Audience::ALL {
        assert!(
            agent_guidance::instructions(audience, &current.content, &public_url)
                .contains("rather than a learning")
        );
    }

    before_it(&url).await;
    let left = every_version(&mut connection).await;
    assert_eq!(left.len(), 1, "down takes back what up added");
    apply(&url).await;
    let current = guidance::current(&mut connection)
        .await
        .expect("a version exists");
    assert_eq!(current.version, 2, "and up applies again");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn what_an_admin_wrote_is_left_alone() {
    let (url, mut connection) = migrated_database().await;
    before_it(&url).await;
    admin_writes(&mut connection, 2, "Our own guidance.").await;

    apply(&url).await;

    let kept = every_version(&mut connection).await;
    assert_eq!(kept.len(), 2, "no version was added");
    assert_eq!(kept[0].content, "Our own guidance.");
    assert_eq!(kept[0].created_by, TEST_PERSON_ID);

    before_it(&url).await;
    assert_eq!(
        every_version(&mut connection).await.len(),
        2,
        "down removes nothing an admin wrote"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn down_keeps_the_version_an_admin_wrote_over() {
    let (url, mut connection) = migrated_database().await;
    admin_writes(&mut connection, 3, "Ours, on top of it.").await;

    before_it(&url).await;

    let numbers: Vec<i32> = every_version(&mut connection)
        .await
        .iter()
        .map(|version| version.version)
        .collect();
    assert_eq!(numbers, [3, 2, 1]);
}
