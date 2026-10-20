// Copyright © 2026 Jalapeno Labs

//! Work in progress that a restart of the stack would cut short.
//!
//! Auto-update restarts every container, so it waits until nothing is running that a
//! restart would interrupt: today, an agent turn on this host. Whatever starts such work
//! calls [`Activities::begin`] and holds the returned [`ActivityGuard`] for as long as the
//! work runs. Dropping the guard, on success, failure, cancellation, or a panic that
//! unwinds through it, ends the activity, so a turn can never be left counted by an early
//! return.
//!
//! ```ignore
//! let _activity = state.activity.begin(ActivityKind::AgentTurn, format!("session:{id}"));
//! run_the_turn().await?;
//! // The activity ends here, when `_activity` drops.
//! ```
//!
//! The registry is in-process, like the event bus: it describes this API process alone,
//! which is the only one, and it starts empty, since a restart already ended whatever it
//! counted.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use serde::Serialize;

/// What kind of work is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    /// An agent turn running on this host, for a coding session or for Elysia.
    AgentTurn,
}

/// One piece of running work, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub kind: ActivityKind,
    /// Names the work for a person reading Settings, such as `session:42`.
    pub label: String,
    pub started_at: DateTime<Utc>,
}

/// The work running in this process. Cloning is cheap: every clone shares one registry.
#[derive(Debug, Clone, Default)]
pub struct Activities {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Keyed by an id that only grows, so the map lists activities in the order they began.
    running: Mutex<BTreeMap<u64, Activity>>,
    next_id: AtomicU64,
}

impl Inner {
    /// The poison is ignored: the map holds plain values that every operation leaves whole,
    /// and a guard must be able to remove its entry while a panic unwinds.
    fn running(&self) -> MutexGuard<'_, BTreeMap<u64, Activity>> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Activities {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `kind` of work began. It counts until the returned guard drops.
    #[must_use = "the activity ends as soon as the guard is dropped"]
    pub fn begin(&self, kind: ActivityKind, label: impl Into<String>) -> ActivityGuard {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let activity = Activity {
            kind,
            label: label.into(),
            started_at: Utc::now(),
        };
        self.inner.running().insert(id, activity);
        ActivityGuard {
            id,
            inner: Arc::clone(&self.inner),
        }
    }

    /// Whether nothing is running.
    pub fn is_idle(&self) -> bool {
        self.inner.running().is_empty()
    }

    /// What is running, oldest first.
    pub fn snapshot(&self) -> Vec<Activity> {
        self.inner.running().values().cloned().collect()
    }
}

/// Keeps one activity counted. Dropping it ends the activity.
#[derive(Debug)]
pub struct ActivityGuard {
    id: u64,
    inner: Arc<Inner>,
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.inner.running().remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_activity_counts_until_its_guard_drops() {
        let activities = Activities::new();
        assert!(activities.is_idle());

        let guard = activities.begin(ActivityKind::AgentTurn, "session:1");
        assert!(!activities.is_idle());

        drop(guard);
        assert!(activities.is_idle());
    }

    #[test]
    fn clones_share_one_registry() {
        let activities = Activities::new();
        let _guard = activities
            .clone()
            .begin(ActivityKind::AgentTurn, "session:1");
        assert!(!activities.is_idle());
    }

    #[test]
    fn the_snapshot_lists_running_work_oldest_first() {
        let activities = Activities::new();
        let first = activities.begin(ActivityKind::AgentTurn, "session:1");
        let _second = activities.begin(ActivityKind::AgentTurn, "elysia:7");
        let _third = activities.begin(ActivityKind::AgentTurn, "session:2");
        drop(first);

        let labels: Vec<String> = activities
            .snapshot()
            .into_iter()
            .map(|activity| activity.label)
            .collect();
        assert_eq!(labels, ["elysia:7", "session:2"]);
    }

    /// A turn that panics must not stay counted forever, or the deployment would never update.
    #[test]
    fn a_panic_that_unwinds_through_the_guard_ends_the_activity() {
        let activities = Activities::new();
        let registry = activities.clone();
        let outcome = std::panic::catch_unwind(move || {
            let _guard = registry.begin(ActivityKind::AgentTurn, "session:1");
            panic!("the turn failed");
        });

        assert!(outcome.is_err());
        assert!(activities.is_idle());
    }
}
