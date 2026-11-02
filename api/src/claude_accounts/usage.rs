// Copyright © 2026 Jalapeno Labs

//! Reading `/api/oauth/usage`, and deciding when an account's usage should keep a turn off it.
//!
//! The endpoint reports each window as `{ "utilization": ..., "resets_at": ... }`. The weekly
//! figure prefers the OAuth-apps bucket, which is the one Claude Code draws on, and falls back to
//! the overall week. Utilization has been reported both as a fraction (0 to 1) and as a
//! percentage (0 to 100); one payload uses one convention, so any value of 1 or more marks the
//! whole payload as percentages.

use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{Level, event};

use crate::models::claude_account::{UsageWindow, UsageWindows};

/// The usage, as a percentage of a window, at which a turn no longer starts on an account.
///
/// Just short of 100: an account this close to its limit would give out within moments of a
/// turn starting, and the runner would hand the turn on mid-way. Starting it on the next
/// credential is cheaper. A turn already running is not affected; the runner's own failover
/// handles a limit reached mid-turn.
pub const SKIP_AT_UTILIZATION: f64 = 99.0;

const FIVE_HOUR_KEYS: &[&str] = &["five_hour"];
const SEVEN_DAY_KEYS: &[&str] = &["seven_day_oauth_apps", "seven_day"];

/// The windows in a usage answer. A window the answer lacks, or reports without a utilization,
/// is `None`.
pub fn parse(body: &Value) -> UsageWindows {
    let five_hour = read_window(body, FIVE_HOUR_KEYS);
    let seven_day = read_window(body, SEVEN_DAY_KEYS);
    let is_percent_scale = [five_hour, seven_day]
        .iter()
        .flatten()
        .any(|(utilization, _resets_at)| *utilization >= 1.0);
    let scale = if is_percent_scale { 1.0 } else { 100.0 };
    let to_window = |(utilization, resets_at): (f64, Option<DateTime<Utc>>)| UsageWindow {
        utilization: utilization * scale,
        resets_at,
    };
    UsageWindows {
        five_hour: five_hour.map(to_window),
        seven_day: seven_day.map(to_window),
    }
}

fn read_window(body: &Value, keys: &[&str]) -> Option<(f64, Option<DateTime<Utc>>)> {
    let Some(bucket) = keys
        .iter()
        .find_map(|key| body.get(*key).filter(|bucket| bucket.is_object()))
    else {
        event!(
            name: "claude_accounts.usage.window_missing",
            Level::DEBUG,
            usage.window = keys.first().copied().unwrap_or_default(),
            "the usage answer has no such window",
        );
        return None;
    };
    let utilization = bucket.get("utilization")?.as_f64()?;
    let resets_at = bucket
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|instant| instant.with_timezone(&Utc));
    Some((utilization, resets_at))
}

/// When an account's usage keeps turns off it, the time the blocking window resets: the latest
/// reset among windows at or past [`SKIP_AT_UTILIZATION`] that have not reset yet. `None` means
/// the account may take a turn. A spent window with no known reset blocks for an hour past
/// `now`, so a stale reading cannot block forever: the keeper reads usage again long before then.
pub fn blocked_until(usage: &UsageWindows, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    [usage.five_hour, usage.seven_day]
        .into_iter()
        .flatten()
        .filter(|window| window.utilization >= SKIP_AT_UTILIZATION)
        .map(|window| {
            window
                .resets_at
                .unwrap_or_else(|| now + chrono::TimeDelta::hours(1))
        })
        .filter(|resets_at| *resets_at > now)
        .max()
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;

    use super::*;

    #[test]
    fn percentages_are_read_as_they_are() {
        let usage = parse(&json!({
            "five_hour": { "utilization": 37.0, "resets_at": "2026-11-03T01:00:00+00:00" },
            "seven_day": { "utilization": 12.5, "resets_at": "2026-11-08T00:00:00Z" },
        }));

        let five_hour = usage.five_hour.expect("five-hour window");
        assert!((five_hour.utilization - 37.0).abs() < f64::EPSILON);
        assert_eq!(
            five_hour.resets_at.map(|instant| instant.to_rfc3339()),
            Some("2026-11-03T01:00:00+00:00".to_owned())
        );
        let seven_day = usage.seven_day.expect("seven-day window");
        assert!((seven_day.utilization - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn fractions_are_scaled_to_percentages() {
        let usage = parse(&json!({
            "five_hour": { "utilization": 0.25, "resets_at": null },
            "seven_day": { "utilization": 0.5 },
        }));

        assert!((usage.five_hour.expect("present").utilization - 25.0).abs() < 1e-9);
        assert!((usage.seven_day.expect("present").utilization - 50.0).abs() < 1e-9);
        assert_eq!(usage.five_hour.expect("present").resets_at, None);
    }

    #[test]
    fn the_weekly_window_prefers_the_oauth_apps_bucket() {
        let usage = parse(&json!({
            "seven_day": { "utilization": 90.0 },
            "seven_day_oauth_apps": { "utilization": 10.0 },
        }));

        assert!((usage.seven_day.expect("present").utilization - 10.0).abs() < f64::EPSILON);
        assert_eq!(usage.five_hour, None);
    }

    #[test]
    fn an_answer_without_windows_reports_none() {
        assert_eq!(parse(&json!({})), UsageWindows::default());
        assert_eq!(
            parse(&json!({ "five_hour": { "resets_at": "2026-11-03T01:00:00Z" } })),
            UsageWindows::default()
        );
    }

    #[test]
    fn a_spent_window_blocks_until_it_resets_and_no_longer() {
        let now = Utc::now();
        let reset = now + TimeDelta::hours(2);
        let spent = UsageWindows {
            five_hour: Some(UsageWindow {
                utilization: 100.0,
                resets_at: Some(reset),
            }),
            seven_day: Some(UsageWindow {
                utilization: 40.0,
                resets_at: Some(now + TimeDelta::days(3)),
            }),
        };

        assert_eq!(blocked_until(&spent, now), Some(reset));
        assert_eq!(
            blocked_until(&spent, reset + TimeDelta::seconds(1)),
            None,
            "a window that has reset no longer blocks, even before usage is read again"
        );
    }

    #[test]
    fn usage_below_the_threshold_never_blocks() {
        let now = Utc::now();
        let busy = UsageWindows {
            five_hour: Some(UsageWindow {
                utilization: SKIP_AT_UTILIZATION - 0.5,
                resets_at: Some(now + TimeDelta::hours(1)),
            }),
            seven_day: None,
        };
        assert_eq!(blocked_until(&busy, now), None);
        assert_eq!(blocked_until(&UsageWindows::default(), now), None);
    }

    #[test]
    fn the_later_reset_wins_when_both_windows_are_spent() {
        let now = Utc::now();
        let weekly_reset = now + TimeDelta::days(2);
        let both = UsageWindows {
            five_hour: Some(UsageWindow {
                utilization: 100.0,
                resets_at: Some(now + TimeDelta::hours(1)),
            }),
            seven_day: Some(UsageWindow {
                utilization: 100.0,
                resets_at: Some(weekly_reset),
            }),
        };
        assert_eq!(blocked_until(&both, now), Some(weekly_reset));
    }
}
