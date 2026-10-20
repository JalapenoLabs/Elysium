// Copyright © 2026 Jalapeno Labs

//! Backing up a harness session end to end: the real SDK against the fleet's fake satellite,
//! and Postgres, read back the way a continued item reads it.

use std::sync::{Arc, Mutex};

use arsox_sdk::proto::harness::v1::Harness;
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::{Layer, Registry};

use super::*;
use crate::fleet::fake_satellite::{FakeSatellite, SessionArchive, THREAD_ID};
use crate::test_support::{cipher, migrated_database, studio_item_with_session};

/// The names of every event logged at warn or above while it is the default subscriber.
#[derive(Debug, Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl Warnings {
    fn names(&self) -> Vec<String> {
        self.0.lock().expect("lock").clone()
    }
}

impl<Inner: Subscriber> Layer<Inner> for Warnings {
    // Tests on other threads log with no subscriber at all, so a callsite's cached interest
    // must leave every event to be checked as it happens.
    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn on_event(&self, event: &Event<'_>, _context: Context<'_, Inner>) {
        // Levels order by verbosity: ERROR is the least, so `<= WARN` is warn and error.
        if *event.metadata().level() <= Level::WARN {
            self.0
                .lock()
                .expect("lock")
                .push(event.metadata().name().to_owned());
        }
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_completed_turns_session_is_kept_and_reopens_to_the_same_bytes() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 2)
        .await
        .expect("pool");
    let cipher = cipher();
    let satellite = FakeSatellite::start().await;
    let studio = studio_item_with_session(&mut connection, &cipher, satellite.url()).await;
    let workspace = satellite.attach(THREAD_ID).await;

    let archive = b"a Claude transcript, as one tar archive".to_vec();
    satellite.set_session_export(SessionArchive {
        harness: Harness::Claude,
        harness_session_id: "claude-session-1".to_owned(),
        bytes: archive.clone(),
    });
    back_up(&database, &cipher, studio.session.id, &workspace).await;

    let kept = session_transcript::latest_for_studio_item(&mut connection, studio.item.id)
        .await
        .expect("read")
        .expect("the export was kept");
    assert_eq!(kept.session_id, studio.session.id);
    assert_eq!(
        kept.harness().expect("a known harness"),
        HarnessFamily::Claude
    );
    assert_eq!(
        kept.open(&cipher).expect("it unseals"),
        archive,
        "a later thread imports exactly what was exported"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_thread_with_no_session_yet_keeps_nothing_and_warns_of_nothing() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 2)
        .await
        .expect("pool");
    let cipher = cipher();
    // The fake answers HARNESS_SESSION_NOT_FOUND until a test sets an export.
    let satellite = FakeSatellite::start().await;
    let studio = studio_item_with_session(&mut connection, &cipher, satellite.url()).await;
    let workspace = satellite.attach(THREAD_ID).await;

    let warnings = Warnings::default();
    let subscriber = Registry::default().with(warnings.clone());
    // `#[tokio::test]` runs on this one thread, so the thread's default covers the backup.
    let default = tracing::subscriber::set_default(subscriber);
    back_up(&database, &cipher, studio.session.id, &workspace).await;
    drop(default);

    let kept = session_transcript::latest_for_studio_item(&mut connection, studio.item.id)
        .await
        .expect("read");
    assert!(kept.is_none(), "there was nothing to keep");
    assert!(
        warnings.names().is_empty(),
        "an export with nothing in it yet is not a failure: {:?}",
        warnings.names()
    );
}
