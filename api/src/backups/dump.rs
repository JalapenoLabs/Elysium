// Copyright © 2026 Jalapeno Labs

//! Dumps one database with the Postgres server's own `pg_dump`.
//!
//! The dump runs inside the `postgres` container (`docker exec`, through the filtered Docker
//! proxy, which allows exec), exactly as the updater dumps before an update: `pg_dump --format
//! custom`, as the image's superuser over its local socket. The server's own binary is always
//! the server's version, so a dump never fails on a client older than the server, and the API
//! image carries no Postgres client to keep in step with `compose.yml`. The custom format is
//! compressed, and `pg_restore` restores it.
//!
//! The dump streams into a file the API holds, since storage providers need a file's length
//! before its first byte.

use std::path::Path;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::StartExecResults;
use bollard::models::ExecConfig;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

/// The Postgres container `compose.yml` names. The API reaches it by this name, as it reaches
/// nginx's.
pub const POSTGRES_CONTAINER: &str = "elysium-postgres";

/// The account the dump runs as: the image's own, which owns the cluster.
const POSTGRES_USER: &str = "postgres";

/// Runs `pg_dump` on `$1` as the superuser the image was started with. `POSTGRES_USER` is set
/// in the container's environment by `compose.yml`; the database name is an argument, never
/// shell text.
const DUMP_SCRIPT: &str = r#"exec pg_dump --username "$POSTGRES_USER" --format custom -- "$1""#;

/// How much of what `pg_dump` printed to stderr a failure keeps, from its end, where the
/// error is.
const STDERR_TAIL: usize = 1_000;

/// Why a dump failed.
#[derive(Debug, thiserror::Error)]
pub enum DumpError {
    #[error("Docker: {0}")]
    Docker(#[from] bollard::errors::Error),
    #[error("could not write the dump: {0}")]
    Write(#[from] std::io::Error),
    #[error("the dump's exec did not attach its output")]
    Detached,
    #[error("pg_dump exited with status {status}: {stderr}")]
    Exited { status: i64, stderr: String },
    #[error("pg_dump did not report how it ended")]
    NoExitStatus,
}

/// What dumps a database: Postgres's own `pg_dump` in its container, or, in tests, a fake that
/// writes a known body or fails.
#[derive(Debug, Clone)]
pub enum Dumper {
    /// `docker exec` in `container`, normally [`POSTGRES_CONTAINER`].
    Docker { docker: Docker, container: String },
    /// Writes `fake dump of <database>`, or fails for the database named.
    #[cfg(test)]
    Fake { failing: Option<String> },
}

impl Dumper {
    /// Dumps `database` into a new file at `into`, and returns its size in bytes.
    ///
    /// # Errors
    /// [`DumpError::Exited`] with what `pg_dump` printed when it fails, such as a database that
    /// does not exist; otherwise Docker's refusal or a failed write.
    pub async fn dump(&self, database: &str, into: &Path) -> Result<u64, DumpError> {
        match self {
            Self::Docker { docker, container } => {
                dump_in_container(docker, container, database, into).await
            }
            #[cfg(test)]
            Self::Fake { failing } => {
                if failing.as_deref() == Some(database) {
                    return Err(DumpError::Exited {
                        status: 1,
                        stderr: format!("pg_dump: error: database \"{database}\" does not exist"),
                    });
                }
                let body = format!("fake dump of {database}");
                tokio::fs::write(into, &body).await?;
                Ok(body.len() as u64)
            }
        }
    }
}

async fn dump_in_container(
    docker: &Docker,
    container: &str,
    database: &str,
    into: &Path,
) -> Result<u64, DumpError> {
    let exec = docker
        .create_exec(
            container,
            ExecConfig {
                cmd: Some(vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    DUMP_SCRIPT.to_owned(),
                    "pg_dump".to_owned(),
                    database.to_owned(),
                ]),
                user: Some(POSTGRES_USER.to_owned()),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await?;
    let StartExecResults::Attached { mut output, .. } = docker.start_exec(&exec.id, None).await?
    else {
        return Err(DumpError::Detached);
    };

    let mut file = tokio::fs::File::create(into).await?;
    let mut written: u64 = 0;
    let mut stderr: Vec<u8> = Vec::new();
    while let Some(chunk) = output.next().await {
        match chunk? {
            LogOutput::StdOut { message } => {
                file.write_all(&message).await?;
                written += message.len() as u64;
            }
            LogOutput::StdErr { message } => stderr.extend_from_slice(&message),
            LogOutput::Console { .. } | LogOutput::StdIn { .. } => {}
        }
    }
    // Every byte is on disk before the upload reads the file's length.
    file.sync_all().await?;

    let inspected = docker.inspect_exec(&exec.id).await?;
    match inspected.exit_code {
        Some(0) => Ok(written),
        Some(status) => Err(DumpError::Exited {
            status,
            stderr: tail(&String::from_utf8_lossy(&stderr)),
        }),
        None => Err(DumpError::NoExitStatus),
    }
}

/// The end of `text`, trimmed, at most [`STDERR_TAIL`] characters.
fn tail(text: &str) -> String {
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    trimmed
        .chars()
        .skip(count.saturating_sub(STDERR_TAIL))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_keeps_the_end_of_what_pg_dump_printed() {
        let long = format!("{}pg_dump: error: the end", "noise ".repeat(500));
        let kept = tail(&long);
        assert_eq!(kept.chars().count(), STDERR_TAIL);
        assert!(kept.ends_with("pg_dump: error: the end"));
        assert_eq!(tail("  short\n"), "short");
    }

    /// Opt-in: dumps every database of a running stack's Postgres through a Docker proxy with
    /// `EXEC=1`, as the API does, into `BACKUPS_TEST_OUTPUT`. Restoring those dumps by the steps
    /// in `docs/infrastructure.md` is how a restore is tested against a scratch stack.
    #[tokio::test]
    #[ignore = "needs BACKUPS_TEST_DOCKER_URL, BACKUPS_TEST_POSTGRES_CONTAINER, BACKUPS_TEST_DATABASES"]
    async fn a_running_stacks_databases_dump_through_docker() {
        // `verify-migrations.sh` runs every ignored test; this one needs a stack it may not have.
        let (Ok(url), Ok(container), Ok(databases)) = (
            std::env::var("BACKUPS_TEST_DOCKER_URL"),
            std::env::var("BACKUPS_TEST_POSTGRES_CONTAINER"),
            std::env::var("BACKUPS_TEST_DATABASES"),
        ) else {
            eprintln!(
                "skipped: set BACKUPS_TEST_DOCKER_URL, BACKUPS_TEST_POSTGRES_CONTAINER, and \
                 BACKUPS_TEST_DATABASES to dump a running stack"
            );
            return;
        };
        let output = std::env::var("BACKUPS_TEST_OUTPUT")
            .map_or_else(|_unset| std::env::temp_dir(), std::path::PathBuf::from);
        let docker =
            Docker::connect_with_http(&url, 600, bollard::API_DEFAULT_VERSION).expect("a client");
        let dumper = Dumper::Docker { docker, container };

        for database in databases.split(',') {
            let into = output.join(format!("{database}.dump"));
            let size = dumper.dump(database, &into).await.expect("a dump");
            let written = std::fs::read(&into).expect("the dump reads");
            assert_eq!(written.len() as u64, size);
            // Every custom-format archive opens with this signature.
            assert!(
                written.starts_with(b"PGDMP"),
                "{database} is a custom-format dump"
            );
        }

        let refused = dumper
            .dump("no_such_database", &output.join("missing.dump"))
            .await;
        let Err(DumpError::Exited { stderr, .. }) = refused else {
            panic!("a database that does not exist fails the dump: {refused:?}");
        };
        assert!(stderr.contains("no_such_database"), "{stderr}");
    }
}
