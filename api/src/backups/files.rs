// Copyright © 2026 Jalapeno Labs

//! Where a backup's files go in its storage location, and which are old enough to remove.
//!
//! Every file sits directly in [`DIRECTORY`] under the location's own directory, named
//! `<started>-<database>.dump`, with `<started>` the backup's start to the second, such as
//! `database-backups/20261103T080000Z-elysium.dump`. One directory of plain files, rather than a
//! directory per backup, because storage deletes files alone: Bunny removes a directory with
//! everything in it and S3 has no directories at all, so retention deletes file by file and
//! leaves nothing empty behind. A name sorts by time, so the newest backup lists last.
//!
//! Storage locations are shared with Studio and agents, so retention only ever touches a file
//! in [`DIRECTORY`] whose whole name has that shape.

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};

use crate::storage::{Entry, EntryKind};

/// The directory, inside the location's own, every backup's files are written to.
pub const DIRECTORY: &str = "database-backups";

/// How many days a backup is kept. A month of nightly backups means a mistake noticed weeks
/// later can still be undone, and costs little: a dump is a compressed copy of the databases.
/// Older ones are removed after each backup that succeeds.
pub const RETENTION_DAYS: i64 = 30;

/// How a backup's start is written in its files' names: sortable, with no character a path
/// refuses.
const STARTED_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// What every backup file's name ends with: `pg_dump`'s custom format, which `pg_restore` reads.
const EXTENSION: &str = ".dump";

/// The path, relative to the location's directory, of `database`'s dump in the backup that
/// started at `started_at`.
pub fn dump_path(started_at: DateTime<Utc>, database: &str) -> String {
    format!(
        "{DIRECTORY}/{}-{database}{EXTENSION}",
        started_at.format(STARTED_FORMAT)
    )
}

/// When the backup a file belongs to started, or `None` for a file that is not a backup's.
fn started_at_of(entry: &Entry) -> Option<DateTime<Utc>> {
    if entry.kind != EntryKind::File {
        return None;
    }
    let name = entry
        .path
        .strip_prefix(DIRECTORY)?
        .strip_prefix('/')?
        .strip_suffix(EXTENSION)?;
    let (started, database) = name.split_once('-')?;
    if database.is_empty() || database.contains('/') {
        return None;
    }
    NaiveDateTime::parse_from_str(started, STARTED_FORMAT)
        .ok()
        .map(|started| started.and_utc())
}

/// The paths among `entries`, a listing of [`DIRECTORY`], of every backup file older than
/// [`RETENTION_DAYS`] at `now`.
pub fn expired(entries: &[Entry], now: DateTime<Utc>) -> Vec<String> {
    let cutoff = now - TimeDelta::days(RETENTION_DAYS);
    let mut paths = Vec::new();
    for entry in entries {
        if started_at_of(entry).is_some_and(|started_at| started_at < cutoff) {
            paths.push(entry.path.clone());
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn file(path: &str) -> Entry {
        Entry {
            path: path.to_owned(),
            kind: EntryKind::File,
            size_bytes: Some(10),
            modified_at: None,
        }
    }

    fn november(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, 8, 0, 0)
            .single()
            .expect("a valid time")
    }

    #[test]
    fn a_dump_is_named_by_its_backups_start_and_its_database() {
        let started_at = Utc
            .with_ymd_and_hms(2026, 11, 3, 8, 0, 41)
            .single()
            .expect("a valid time");
        assert_eq!(
            dump_path(started_at, "elysium"),
            "database-backups/20261103T080041Z-elysium.dump"
        );
    }

    #[test]
    fn a_backup_past_retention_expires_and_a_recent_one_stays() {
        let now = november(30);
        let old = dump_path(november(30) - TimeDelta::days(RETENTION_DAYS + 1), "kratos");
        let recent = dump_path(november(29), "kratos");
        let entries = [file(&old), file(&recent)];
        assert_eq!(expired(&entries, now), [old]);
    }

    /// Locations are shared, so retention never removes a file it did not write.
    #[test]
    fn only_files_shaped_like_a_backups_ever_expire() {
        let now = november(30);
        let entries = [
            file("database-backups/notes.txt"),
            file("database-backups/20200101T000000Z.dump"),
            file("database-backups/20200101T000000Z-.dump"),
            file("database-backups/2020-01-01-elysium.dump"),
            file("studio/20200101T000000Z-elysium.dump"),
            Entry {
                path: "database-backups/20200101T000000Z-elysium.dump".to_owned(),
                kind: EntryKind::Directory,
                size_bytes: None,
                modified_at: None,
            },
        ];
        assert!(expired(&entries, now).is_empty());
    }

    #[test]
    fn a_database_name_with_a_hyphen_is_still_a_backups() {
        let now = november(30);
        let old = dump_path(
            november(1) - TimeDelta::days(RETENTION_DAYS),
            "elysium-work",
        );
        assert_eq!(expired(&[file(&old)], now), [old]);
    }
}
