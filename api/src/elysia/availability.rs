// Copyright © 2026 Jalapeno Labs

//! When Elysia may start work on her own: weekly windows in UTC.
//!
//! The server holds no time zones (see CLAUDE.md, "Time is UTC on the server"), so a window
//! is a range of minutes in the UTC week, where Monday 00:00 UTC is minute 0. The frontend
//! converts the viewer's local hours into these ranges when they are saved, and back when
//! they are shown. A local schedule therefore moves by an hour against UTC when daylight
//! saving starts or ends, until it is saved again; the settings page says so.
//!
//! No windows means any time. Windows only hold back work Elysia starts herself: a person's
//! message to her is answered whatever they say.

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};

/// Minutes in a week, the exclusive upper bound of a window's end.
pub const MINUTES_PER_WEEK: u16 = 7 * 24 * 60;

/// How many windows a schedule may hold: one or two per day with room to spare.
pub const MAX_WINDOWS: usize = 28;

/// One weekly range of UTC minutes, `start` inclusive and `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AvailabilityWindow {
    pub start_minute: u16,
    pub end_minute: u16,
}

/// Why a schedule was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("a schedule holds at most {MAX_WINDOWS} windows")]
    TooManyWindows,
    #[error("a window must start before it ends, within the {MINUTES_PER_WEEK} minutes of a week")]
    InvalidWindow,
}

/// Checks a schedule a person sent. A window crossing the end of the week is sent as two.
///
/// # Errors
/// Returns [`ScheduleError`] for too many windows or a window that is empty, reversed, or
/// past the end of the week.
pub fn validate(windows: &[AvailabilityWindow]) -> Result<(), ScheduleError> {
    if windows.len() > MAX_WINDOWS {
        return Err(ScheduleError::TooManyWindows);
    }
    let all_valid = windows.iter().all(|window| {
        window.start_minute < window.end_minute && window.end_minute <= MINUTES_PER_WEEK
    });
    if !all_valid {
        return Err(ScheduleError::InvalidWindow);
    }
    Ok(())
}

/// The minute of the UTC week `instant` falls in.
pub fn minute_of_week(instant: DateTime<Utc>) -> u16 {
    let days =
        u16::try_from(instant.weekday().num_days_from_monday()).expect("a weekday is below 7");
    let hours = u16::try_from(instant.hour()).expect("an hour is below 24");
    let minutes = u16::try_from(instant.minute()).expect("a minute is below 60");
    days * 24 * 60 + hours * 60 + minutes
}

/// Whether Elysia may start work on her own at `instant`.
pub fn is_available(windows: &[AvailabilityWindow], instant: DateTime<Utc>) -> bool {
    if windows.is_empty() {
        return true;
    }
    let minute = minute_of_week(instant);
    windows
        .iter()
        .any(|window| (window.start_minute..window.end_minute).contains(&minute))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        // 2026-10-19 is a Monday.
        Utc.with_ymd_and_hms(2026, 10, 19 + day, hour, minute, 0)
            .single()
            .expect("a valid instant")
    }

    #[test]
    fn the_week_starts_at_monday_midnight_utc() {
        assert_eq!(minute_of_week(at(0, 0, 0)), 0);
        assert_eq!(minute_of_week(at(0, 9, 30)), 570);
        assert_eq!(minute_of_week(at(6, 23, 59)), MINUTES_PER_WEEK - 1);
    }

    #[test]
    fn no_windows_means_any_time() {
        assert!(is_available(&[], at(3, 3, 0)));
    }

    #[test]
    fn a_window_holds_its_start_and_not_its_end() {
        // Monday 09:00 to 17:00 UTC.
        let windows = [AvailabilityWindow {
            start_minute: 540,
            end_minute: 1020,
        }];

        assert!(is_available(&windows, at(0, 9, 0)));
        assert!(is_available(&windows, at(0, 16, 59)));
        assert!(!is_available(&windows, at(0, 17, 0)));
        assert!(!is_available(&windows, at(1, 10, 0)), "Tuesday is outside");
    }

    #[test]
    fn empty_reversed_and_overlong_windows_are_refused() {
        let window = |start_minute, end_minute| AvailabilityWindow {
            start_minute,
            end_minute,
        };

        assert_eq!(
            validate(&[window(10, 10)]),
            Err(ScheduleError::InvalidWindow)
        );
        assert_eq!(
            validate(&[window(20, 10)]),
            Err(ScheduleError::InvalidWindow)
        );
        assert_eq!(
            validate(&[window(0, MINUTES_PER_WEEK + 1)]),
            Err(ScheduleError::InvalidWindow)
        );
        assert_eq!(validate(&[window(0, MINUTES_PER_WEEK)]), Ok(()));
        assert_eq!(
            validate(&[window(0, 1); MAX_WINDOWS + 1]),
            Err(ScheduleError::TooManyWindows)
        );
    }
}
