// Copyright © 2026 Jalapeno Labs

//! An initiative's progress: how many of its items are resolved, out of how many count.
//!
//! Total counts every member that is `inbox`, `open`, or `resolved`; a dismissed or
//! deleted item counts toward neither. An item in two initiatives counts fully toward
//! both, since each initiative counts only its own members.
//!
//! Progress is shown as resolved and total together, with a burnup chart of both over
//! time, so added scope shows as scope rather than as a bar moving backwards. The chart is
//! drawn from when each item joined and left the initiative, and when it was resolved,
//! dismissed, or deleted. An item keeps only its latest resolution, so one resolved,
//! reopened, and resolved again shows resolved from the second time on.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// One span an item spent in an initiative, with the moments that decide how it counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_field_names,
    reason = "each is a moment, named like the column it comes from"
)]
pub struct Membership {
    pub joined_at: DateTime<Utc>,
    /// `None` while the item is still a member.
    pub left_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub dismissed_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Resolved items out of the total that count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub resolved: usize,
    pub total: usize,
}

/// Progress as it stood at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BurnupPoint {
    pub at: DateTime<Utc>,
    pub resolved: usize,
    pub total: usize,
}

/// Whether `moment` had happened by `instant`.
fn happened_by(moment: Option<DateTime<Utc>>, instant: DateTime<Utc>) -> bool {
    moment.is_some_and(|moment| moment <= instant)
}

impl Membership {
    /// How this span counted at `instant`: `None` when it did not count at all, otherwise
    /// whether the item was resolved.
    fn counted_at(&self, instant: DateTime<Utc>) -> Option<bool> {
        let is_member = self.joined_at <= instant && !happened_by(self.left_at, instant);
        let is_dropped =
            happened_by(self.dismissed_at, instant) || happened_by(self.deleted_at, instant);
        if !is_member || is_dropped {
            return None;
        }
        Some(happened_by(self.resolved_at, instant))
    }
}

/// Progress at `instant`.
pub fn at(memberships: &[Membership], instant: DateTime<Utc>) -> Progress {
    let mut progress = Progress::default();
    for membership in memberships {
        let Some(is_resolved) = membership.counted_at(instant) else {
            continue;
        };
        progress.total += 1;
        if is_resolved {
            progress.resolved += 1;
        }
    }
    progress
}

/// Progress from `since` to `now`: a point at `since`, one at every moment either count
/// changed, and one at `now`.
///
/// Progress is recounted at each moment something happened, which is quadratic in the
/// number of members. Initiatives hold tens to hundreds of items, so this stays well under
/// a millisecond and keeps the counting rule in one place, [`Membership::counted_at`].
pub fn burnup(
    memberships: &[Membership],
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Vec<BurnupPoint> {
    let mut moments: Vec<DateTime<Utc>> = Vec::new();
    for membership in memberships {
        moments.push(membership.joined_at);
        moments.extend(membership.left_at);
        moments.extend(membership.resolved_at);
        moments.extend(membership.dismissed_at);
        moments.extend(membership.deleted_at);
    }
    moments.retain(|moment| since < *moment && *moment < now);
    moments.sort_unstable();
    moments.dedup();

    let mut points: Vec<BurnupPoint> = Vec::new();
    for moment in std::iter::once(since).chain(moments) {
        let progress = at(memberships, moment);
        let is_unchanged = points.last().is_some_and(|previous| {
            previous.resolved == progress.resolved && previous.total == progress.total
        });
        if is_unchanged {
            continue;
        }
        points.push(BurnupPoint {
            at: moment,
            resolved: progress.resolved,
            total: progress.total,
        });
    }

    let current = at(memberships, now);
    points.push(BurnupPoint {
        at: now,
        resolved: current.resolved,
        total: current.total,
    });
    points
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn day(number: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-01T09:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
            + TimeDelta::days(number)
    }

    fn joined(on: i64) -> Membership {
        Membership {
            joined_at: day(on),
            left_at: None,
            resolved_at: None,
            dismissed_at: None,
            deleted_at: None,
        }
    }

    fn counts(points: &[BurnupPoint]) -> Vec<(DateTime<Utc>, usize, usize)> {
        points
            .iter()
            .map(|point| (point.at, point.resolved, point.total))
            .collect()
    }

    #[test]
    fn dismissed_deleted_and_departed_items_count_toward_neither() {
        let memberships = [
            joined(0),
            Membership {
                resolved_at: Some(day(1)),
                ..joined(0)
            },
            Membership {
                dismissed_at: Some(day(2)),
                ..joined(0)
            },
            Membership {
                deleted_at: Some(day(2)),
                ..joined(0)
            },
            Membership {
                left_at: Some(day(2)),
                ..joined(0)
            },
        ];

        assert_eq!(
            at(&memberships, day(1)),
            Progress {
                resolved: 1,
                total: 5
            }
        );
        assert_eq!(
            at(&memberships, day(3)),
            Progress {
                resolved: 1,
                total: 2
            }
        );
    }

    #[test]
    fn an_item_resolved_before_joining_counts_as_resolved_from_the_moment_it_joins() {
        let late_joiner = [Membership {
            resolved_at: Some(day(1)),
            ..joined(3)
        }];

        assert_eq!(at(&late_joiner, day(2)), Progress::default());
        assert_eq!(
            at(&late_joiner, day(3)),
            Progress {
                resolved: 1,
                total: 1
            }
        );
    }

    #[test]
    fn a_rejoined_item_counts_once_per_span_it_is_in() {
        let spans = [
            Membership {
                left_at: Some(day(2)),
                ..joined(0)
            },
            joined(4),
        ];

        assert_eq!(at(&spans, day(1)).total, 1);
        assert_eq!(at(&spans, day(3)).total, 0);
        assert_eq!(at(&spans, day(5)).total, 1);
    }

    #[test]
    fn burnup_shows_added_scope_as_scope_and_ends_now() {
        let memberships = [
            Membership {
                resolved_at: Some(day(2)),
                ..joined(0)
            },
            joined(0),
            // Scope added on day 3, then resolved on day 4.
            Membership {
                resolved_at: Some(day(4)),
                ..joined(3)
            },
            // Dismissed the same day it was resolved elsewhere: one point for the moment.
            Membership {
                dismissed_at: Some(day(4)),
                ..joined(1)
            },
        ];

        let points = burnup(&memberships, day(0), day(6));

        assert_eq!(
            counts(&points),
            [
                (day(0), 0, 2),
                (day(1), 0, 3),
                (day(2), 1, 3),
                (day(3), 1, 4),
                (day(4), 2, 3),
                (day(6), 2, 3),
            ]
        );
    }

    #[test]
    fn moments_that_change_nothing_add_no_point() {
        let memberships = [
            // Joined and dismissed the same moment: never counted.
            Membership {
                dismissed_at: Some(day(1)),
                ..joined(1)
            },
            joined(0),
        ];

        let points = burnup(&memberships, day(0), day(2));

        assert_eq!(counts(&points), [(day(0), 0, 1), (day(2), 0, 1)]);
    }

    #[test]
    fn an_empty_initiative_draws_a_flat_line_from_creation() {
        let points = burnup(&[], day(0), day(3));

        assert_eq!(counts(&points), [(day(0), 0, 0), (day(3), 0, 0)]);
    }
}
