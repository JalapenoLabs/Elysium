// Copyright © 2026 Jalapeno Labs

//! When the nightly backup is due, as a pure function of the clock and tonight's attempts.
//!
//! Every night has one slot, [`BACKUP_TIME`] in UTC. From the slot on, the backup is due until
//! one succeeds, whatever started it: an admin's Back up now after the slot counts as
//! tonight's. A scheduled attempt that fails is retried [`RETRY_AFTER`] later, up to
//! [`ATTEMPTS_PER_NIGHT`] in all, and then the night is given up and reported. Deciding from
//! the recorded attempts, rather than from a timer, means a process that was down at the slot
//! backs up as soon as it starts, and one that restarts never backs up twice.

use chrono::{DateTime, NaiveTime, TimeDelta, Utc};

use crate::models::database_backup::{DatabaseBackup, DatabaseBackupStatus, DatabaseBackupTrigger};

/// The time of day, in UTC, the nightly backup is due. 08:00 UTC is 02:00 or 03:00 in US
/// Central time, where Elysium's deployments are used, so the dump runs while nobody is
/// working. The server holds no time zones; a deployment elsewhere changes this constant.
pub const BACKUP_TIME: NaiveTime = NaiveTime::from_hms_opt(8, 0, 0).expect("08:00 is a time");

/// How long after a failed scheduled attempt the next one starts. Long enough for a provider
/// outage or a full disk to clear, short enough that every retry lands the same night.
pub const RETRY_AFTER: TimeDelta = TimeDelta::hours(1);

/// How many scheduled attempts a night gets before it is given up and reported. Three spaced an
/// hour apart ride out a passing outage without reporting it, and still report a lasting one
/// before the working day starts.
pub const ATTEMPTS_PER_NIGHT: usize = 3;

/// Where tonight's backup stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Back up now.
    Due,
    /// A backup is running.
    Running,
    /// Tonight's backup is stored.
    Done,
    /// A scheduled attempt failed; the next one starts at this time.
    RetryAt(DateTime<Utc>),
    /// Every scheduled attempt failed; the next one is tomorrow's.
    GaveUp,
}

/// The latest slot at or before `now`: today's [`BACKUP_TIME`] once it has passed, otherwise
/// yesterday's.
pub fn latest_slot(now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now.date_naive().and_time(BACKUP_TIME).and_utc();
    if today <= now {
        return today;
    }
    today - TimeDelta::days(1)
}

/// The first slot after `now`.
pub fn next_slot(now: DateTime<Utc>) -> DateTime<Utc> {
    latest_slot(now) + TimeDelta::days(1)
}

/// Where tonight's backup stands at `now`, from `attempts`, every backup that began at or
/// after [`latest_slot`].
pub fn decide(now: DateTime<Utc>, attempts: &[DatabaseBackup]) -> Decision {
    let mut failed_on_schedule = 0;
    let mut last_failure: Option<DateTime<Utc>> = None;
    for attempt in attempts {
        match attempt.status {
            DatabaseBackupStatus::Running => return Decision::Running,
            DatabaseBackupStatus::Succeeded => return Decision::Done,
            DatabaseBackupStatus::Failed if attempt.trigger == DatabaseBackupTrigger::Schedule => {
                failed_on_schedule += 1;
                let finished_at = attempt.finished_at.unwrap_or(attempt.started_at);
                last_failure = last_failure.max(Some(finished_at));
            }
            // An admin's or the deployment's failure was seen by whoever started it, so it
            // neither spends one of the night's attempts nor delays the next.
            DatabaseBackupStatus::Failed => {}
        }
    }

    if failed_on_schedule >= ATTEMPTS_PER_NIGHT {
        return Decision::GaveUp;
    }
    match last_failure {
        Some(failed_at) if failed_at + RETRY_AFTER > now => {
            Decision::RetryAt(failed_at + RETRY_AFTER)
        }
        _ => Decision::Due,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use uuid::Uuid;

    use super::*;
    use crate::models::user::SYSTEM_USER_ID;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 3, hour, minute, 0)
            .single()
            .expect("a valid time")
    }

    fn attempt(
        trigger: DatabaseBackupTrigger,
        status: DatabaseBackupStatus,
        started_at: DateTime<Utc>,
    ) -> DatabaseBackup {
        let finished = status != DatabaseBackupStatus::Running;
        DatabaseBackup {
            id: Uuid::now_v7(),
            trigger,
            status,
            storage_location_id: Some(Uuid::now_v7()),
            started_at,
            finished_at: finished.then(|| started_at + TimeDelta::minutes(2)),
            size_bytes: (status == DatabaseBackupStatus::Succeeded).then_some(1024),
            error: (status == DatabaseBackupStatus::Failed).then(|| "refused".to_owned()),
            created_by: SYSTEM_USER_ID,
            claimed_at: Some(started_at),
        }
    }

    fn scheduled_failure(started_at: DateTime<Utc>) -> DatabaseBackup {
        attempt(
            DatabaseBackupTrigger::Schedule,
            DatabaseBackupStatus::Failed,
            started_at,
        )
    }

    #[test]
    fn the_slot_is_todays_once_it_has_passed_and_yesterdays_before() {
        assert_eq!(latest_slot(at(8, 0)), at(8, 0));
        assert_eq!(latest_slot(at(23, 59)), at(8, 0));
        assert_eq!(latest_slot(at(7, 59)), at(8, 0) - TimeDelta::days(1));
        assert_eq!(next_slot(at(7, 59)), at(8, 0));
        assert_eq!(next_slot(at(8, 0)), at(8, 0) + TimeDelta::days(1));
    }

    #[test]
    fn a_night_without_attempts_is_due() {
        assert_eq!(decide(at(8, 0), &[]), Decision::Due);
    }

    #[test]
    fn any_success_since_the_slot_is_tonights_backup() {
        let by_an_admin = attempt(
            DatabaseBackupTrigger::Admin,
            DatabaseBackupStatus::Succeeded,
            at(9, 0),
        );
        assert_eq!(decide(at(10, 0), &[by_an_admin]), Decision::Done);
    }

    #[test]
    fn nothing_starts_while_a_backup_runs() {
        let running = attempt(
            DatabaseBackupTrigger::Deployment,
            DatabaseBackupStatus::Running,
            at(8, 30),
        );
        assert_eq!(decide(at(9, 0), &[running]), Decision::Running);
    }

    #[test]
    fn a_failed_attempt_is_retried_an_hour_after_it_ended() {
        let failed = scheduled_failure(at(8, 0));
        let retry_at = at(8, 2) + RETRY_AFTER;
        assert_eq!(
            decide(at(8, 30), std::slice::from_ref(&failed)),
            Decision::RetryAt(retry_at)
        );
        assert_eq!(decide(retry_at, &[failed]), Decision::Due);
    }

    #[test]
    fn the_night_is_given_up_after_its_last_scheduled_attempt() {
        let failures = [
            scheduled_failure(at(8, 0)),
            scheduled_failure(at(9, 5)),
            scheduled_failure(at(10, 10)),
        ];
        assert_eq!(decide(at(12, 0), &failures), Decision::GaveUp);
    }

    /// The person who asked for a backup saw it fail, so it spends none of the night's
    /// attempts.
    #[test]
    fn an_admins_failure_neither_spends_an_attempt_nor_delays_the_next() {
        let failures = [
            scheduled_failure(at(8, 0)),
            scheduled_failure(at(9, 5)),
            attempt(
                DatabaseBackupTrigger::Admin,
                DatabaseBackupStatus::Failed,
                at(10, 0),
            ),
        ];
        assert_eq!(decide(at(10, 30), &failures), Decision::Due);
    }
}
