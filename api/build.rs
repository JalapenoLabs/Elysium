// Copyright © 2026 Jalapeno Labs

//! Captures git and toolchain metadata into environment variables for `/api/version`.
//!
//! Everything is best-effort: a checkout with no commits, a Docker context without
//! `.git`, or a machine without `git` all produce an empty [`GitInfo`] rather than a
//! failed build.

#[path = "src/git_info.rs"]
mod git_info;

use std::path::Path;
use std::process::Command;

use git_info::{GitCommit, GitInfo};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// How many commits `/api/version` reports. Enough to identify what a deployment
/// contains without turning the response into a changelog.
const GIT_HISTORY_DEPTH: usize = 20;

/// Field separator for `git log --format`. A control character cannot appear in a
/// hash, author, date, or subject, so splitting on it is unambiguous.
const GIT_LOG_FIELD_SEPARATOR: char = '\u{1f}';

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("cargo always sets CARGO_MANIFEST_DIR");
    let repository = Path::new(&manifest_dir);

    // Rebuild when HEAD moves or any branch ref is written so the embedded history
    // stays honest. Both paths exist from `git init` onward; `refs` is a directory,
    // which cargo scans recursively, so every commit to a loose ref triggers a rerun.
    for tracked in [".git/HEAD", ".git/refs"] {
        let path = repository.join("..").join(tracked);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    // `embed_migrations!` compiles the SQL in. Cargo only watches paths a build script
    // names once it names any, so the directory has to be listed explicitly.
    println!("cargo:rerun-if-changed=migrations");

    let git_info = read_git_info(repository);
    let git_info_json = serde_json::to_string(&git_info).expect("GitInfo serializes to JSON");
    println!("cargo:rustc-env=ELYSIUM_GIT_INFO={git_info_json}");

    let built_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC 3339 formatting of the current time cannot fail");
    println!("cargo:rustc-env=ELYSIUM_BUILT_AT={built_at}");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let rustc_version = run_command(Command::new(rustc).arg("--version")).unwrap_or_default();
    println!("cargo:rustc-env=ELYSIUM_RUSTC_VERSION={rustc_version}");

    let profile = std::env::var("PROFILE").unwrap_or_default();
    println!("cargo:rustc-env=ELYSIUM_BUILD_PROFILE={profile}");
}

/// Reads branch and recent history from the checkout containing `directory`.
fn read_git_info(directory: &Path) -> GitInfo {
    let branch = run_command(Command::new("git").current_dir(directory).args([
        "rev-parse",
        "--abbrev-ref",
        "HEAD",
    ]));

    let log_format = ["%H", "%h", "%an", "%cI", "%s"].join(&GIT_LOG_FIELD_SEPARATOR.to_string());

    let history = run_command(Command::new("git").current_dir(directory).args([
        "log",
        "-n",
        &GIT_HISTORY_DEPTH.to_string(),
        &format!("--format={log_format}"),
    ]));

    let Some((branch, history)) = branch.zip(history) else {
        println!(
            "cargo:warning=git metadata unavailable; /api/version will report an empty history"
        );
        return GitInfo::default();
    };

    let history: Vec<GitCommit> = history.lines().filter_map(parse_log_line).collect();

    GitInfo {
        branch,
        commit: history.first().cloned(),
        history,
    }
}

/// Parses one `git log` line produced with the separator-joined format above.
fn parse_log_line(line: &str) -> Option<GitCommit> {
    let mut fields = line.split(GIT_LOG_FIELD_SEPARATOR);

    Some(GitCommit {
        hash: fields.next()?.to_owned(),
        short_hash: fields.next()?.to_owned(),
        author: fields.next()?.to_owned(),
        date: fields.next()?.to_owned(),
        subject: fields.next()?.to_owned(),
    })
}

/// Runs `command`, returning trimmed stdout only when it exited successfully.
fn run_command(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
