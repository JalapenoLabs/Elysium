// Copyright © 2026 Jalapeno Labs

//! Files in the workspace, read and written for Elysium's own code: tools that move files
//! between a session's directory and storage, and Studio collecting what an agent delivered.
//!
//! The API never mounts the workspace volume. Every operation is a `docker exec` as the agent
//! account, so a file Elysium writes belongs to the agent, and a symbolic link an agent left
//! can only ever lead somewhere else inside the workspace container, never into the API's.
//! Each operation is confined to a root, a session's directory: the path is resolved inside
//! the container and refused unless it stays under the root, and a write never follows a
//! link at its target.
//!
//! Bytes stream both ways. A read reports the file's size before its first byte, as storage
//! providers need, and its body fails if fewer or more bytes arrive. A write lands in a
//! partial file renamed into place once complete, so a failed write never leaves half a file
//! under the real name.

use std::pin::Pin;
use std::task::{Context, Poll};

use bollard::container::LogOutput;
use bollard::exec::StartExecResults;
use bollard::models::ExecConfig;
use bytes::Bytes;
use futures_util::stream::BoxStream;
use futures_util::{Stream, StreamExt};
use tokio::io::AsyncWriteExt;

use super::WorkspacePath;
use super::workspace::{
    AGENT_USER, CONTAINER_NAME, SCRIPT_TIMEOUT, WorkspaceContainer, WorkspaceError,
};

/// Exit statuses the scripts below use to say why they refused.
const EXIT_MISSING: i64 = 66;
const EXIT_OUTSIDE: i64 = 65;

/// `$1` is the root, `$2` the path under it. Prints the size, a newline, then the bytes.
const READ_SCRIPT: &str = r#"
root=$(realpath -m -- "$1")
resolved=$(realpath -e -- "$1/$2" 2>/dev/null) || exit 66
case "$resolved" in "$root"/*) ;; *) exit 65 ;; esac
[ -f "$resolved" ] || exit 66
stat -c %s -- "$resolved" || exit 70
exec cat -- "$resolved"
"#;

/// `$1` is the root, `$2` the path under it; the bytes arrive on standard input. Prints the
/// size written.
const WRITE_SCRIPT: &str = r#"
root=$(realpath -m -- "$1")
target="$1/$2"
directory=$(dirname -- "$target")
mkdir -p -- "$directory" || exit 70
resolved=$(realpath -e -- "$directory") || exit 70
case "$resolved/" in "$root"/*) ;; *) exit 65 ;; esac
[ ! -L "$target" ] || exit 65
partial="$resolved/.$(basename -- "$target").elysium-partial"
cat > "$partial" || { rm -f -- "$partial"; exit 70; }
mv -f -- "$partial" "$resolved/$(basename -- "$target")" || exit 70
stat -c %s -- "$resolved/$(basename -- "$target")"
"#;

/// `$1` is the root, `$2` a directory under it. Prints `size`, `sha256`, and the path relative
/// to that directory for every regular file in it, each field followed by a NUL. A directory
/// that does not exist lists nothing.
const LIST_SCRIPT: &str = r#"
root=$(realpath -m -- "$1")
directory=$(realpath -e -- "$1/$2" 2>/dev/null) || exit 0
case "$directory/" in "$root"/*) ;; *) exit 65 ;; esac
cd -- "$directory" || exit 70
find . -type f ! -name '*.elysium-partial' -printf '%P\0' | while IFS= read -r -d '' file; do
  printf '%s\0%s\0%s\0' "$(stat -c %s -- "$file")" "$(sha256sum -- "$file" | cut -d' ' -f1)" "$file"
done
"#;

/// `$1` is a directory to remove, with everything in it.
const REMOVE_SCRIPT: &str = r#"rm -rf -- "$1""#;

/// Why a file operation was refused.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("{0} does not exist or is not a file")]
    NotFound(String),
    #[error("{0} leads outside the directory it must stay in")]
    Outside(String),
    #[error("{0}")]
    Failed(String),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
}

/// A file being read: its size, then its bytes.
pub struct WorkspaceFile {
    pub content_length: u64,
    pub body: BoxStream<'static, Result<Bytes, FileError>>,
}

impl std::fmt::Debug for WorkspaceFile {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkspaceFile")
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

/// One file a listing found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// Relative to the listed directory, with `/` between components.
    pub path: String,
    pub size_bytes: u64,
    /// Lowercase hex.
    pub sha256: String,
}

impl WorkspaceContainer {
    /// Starts reading `path` under `root`.
    ///
    /// # Errors
    /// [`FileError::NotFound`] or [`FileError::Outside`] before any byte is read; the body
    /// fails if the file changes length while it streams.
    pub async fn read_file(
        &self,
        root: &WorkspacePath,
        path: &str,
    ) -> Result<WorkspaceFile, FileError> {
        let root = root.absolute();
        let (mut output, exec_id) = self
            .start(vec![READ_SCRIPT.to_owned(), root, path.to_owned()])
            .await?;

        // The size line comes first; anything before its newline is the size.
        let mut head: Vec<u8> = Vec::new();
        let newline = loop {
            if let Some(newline) = head.iter().position(|byte| *byte == b'\n') {
                break newline;
            }
            match output.next().await {
                Some(Ok(LogOutput::StdOut { message })) => head.extend_from_slice(&message),
                Some(Ok(_other_stream)) => {}
                Some(Err(error)) => return Err(WorkspaceError::from(error).into()),
                None => {
                    let exit = self.exit_code(&exec_id).await?;
                    return Err(refusal(exit, path));
                }
            }
        };
        let content_length: u64 = std::str::from_utf8(&head[..newline])
            .ok()
            .and_then(|size| size.trim().parse().ok())
            .ok_or_else(|| FileError::Failed(format!("could not read the size of {path}")))?;
        let first = Bytes::copy_from_slice(&head[newline + 1..]);

        let remainder = output.filter_map(|chunk| async move {
            match chunk {
                Ok(LogOutput::StdOut { message }) => Some(Ok(message)),
                Ok(_other_stream) => None,
                Err(error) => Some(Err(FileError::Workspace(error.into()))),
            }
        });
        let body = futures_util::stream::once(async move { Ok(first) })
            .chain(remainder)
            .filter(|chunk| {
                let empty = matches!(chunk, Ok(bytes) if bytes.is_empty());
                async move { !empty }
            });
        Ok(WorkspaceFile {
            content_length,
            body: ExactLength::new(body.boxed(), content_length, path.to_owned()).boxed(),
        })
    }

    /// Writes `body` to `path` under `root`, creating its directories, and returns the size
    /// written.
    ///
    /// # Errors
    /// [`FileError::Outside`] for a path that leaves `root` or ends in a link, and
    /// [`FileError::Failed`] when the body fails or the write does not complete.
    pub async fn write_file<Chunks, ChunkError>(
        &self,
        root: &WorkspacePath,
        path: &str,
        mut body: Chunks,
    ) -> Result<u64, FileError>
    where
        Chunks: Stream<Item = Result<Bytes, ChunkError>> + Unpin + Send,
        ChunkError: std::fmt::Display,
    {
        let exec = self
            .docker()
            .create_exec(
                CONTAINER_NAME,
                ExecConfig {
                    cmd: Some(script_argv(vec![
                        WRITE_SCRIPT.to_owned(),
                        root.absolute(),
                        path.to_owned(),
                    ])),
                    user: Some(AGENT_USER.to_owned()),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .map_err(WorkspaceError::from)?;
        let StartExecResults::Attached {
            mut output,
            mut input,
        } = self
            .docker()
            .start_exec(&exec.id, None)
            .await
            .map_err(WorkspaceError::from)?
        else {
            return Err(WorkspaceError::Detached.into());
        };

        let writing = async {
            while let Some(chunk) = body.next().await {
                let chunk = chunk.map_err(|error| {
                    FileError::Failed(format!("the file's source failed: {error}"))
                })?;
                input.write_all(&chunk).await.map_err(|error| {
                    FileError::Failed(format!("could not write {path}: {error}"))
                })?;
            }
            input
                .shutdown()
                .await
                .map_err(|error| FileError::Failed(format!("could not finish {path}: {error}")))
        };
        let reading = async {
            let mut printed = Vec::new();
            while let Some(chunk) = output.next().await {
                if let LogOutput::StdOut { message } = chunk.map_err(WorkspaceError::from)? {
                    printed.extend_from_slice(&message);
                }
            }
            Ok::<_, FileError>(printed)
        };
        let (written, printed) = tokio::join!(writing, reading);
        written?;
        let printed = printed?;

        let exit = self.exit_code(&exec.id).await?;
        if exit != Some(0) {
            return Err(refusal(exit, path));
        }
        String::from_utf8_lossy(&printed)
            .trim()
            .parse()
            .map_err(|_unparsed| FileError::Failed(format!("could not confirm {path} was written")))
    }

    /// Every regular file under `directory` in `root`, with its size and hash.
    ///
    /// # Errors
    /// [`FileError::Outside`] for a directory that leaves `root`.
    pub async fn list_files(
        &self,
        root: &WorkspacePath,
        directory: &str,
    ) -> Result<Vec<ListedFile>, FileError> {
        let (mut output, exec_id) = self
            .start(vec![
                LIST_SCRIPT.to_owned(),
                root.absolute(),
                directory.to_owned(),
            ])
            .await?;
        let mut printed = Vec::new();
        while let Some(chunk) = output.next().await {
            if let LogOutput::StdOut { message } = chunk.map_err(WorkspaceError::from)? {
                printed.extend_from_slice(&message);
            }
        }
        let exit = self.exit_code(&exec_id).await?;
        if exit != Some(0) {
            return Err(refusal(exit, directory));
        }
        Ok(parse_listing(&printed))
    }

    /// Removes `directory` and everything in it. One that does not exist is not an error.
    ///
    /// # Errors
    /// Propagates Docker's refusal, or the removal's own failure.
    pub async fn remove_directory(&self, directory: &WorkspacePath) -> Result<(), FileError> {
        let output = self
            .run_script(
                AGENT_USER,
                REMOVE_SCRIPT,
                &[&directory.absolute()],
                Vec::new(),
                SCRIPT_TIMEOUT,
            )
            .await?;
        if output.exit_code != Some(0) {
            return Err(FileError::Failed(format!(
                "could not remove {}: {}",
                directory.absolute(),
                output.output.trim()
            )));
        }
        Ok(())
    }

    async fn start(
        &self,
        script_and_arguments: Vec<String>,
    ) -> Result<
        (
            Pin<Box<dyn Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send>>,
            String,
        ),
        FileError,
    > {
        let exec = self
            .docker()
            .create_exec(
                CONTAINER_NAME,
                ExecConfig {
                    cmd: Some(script_argv(script_and_arguments)),
                    user: Some(AGENT_USER.to_owned()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .map_err(WorkspaceError::from)?;
        let StartExecResults::Attached { output, .. } = self
            .docker()
            .start_exec(&exec.id, None)
            .await
            .map_err(WorkspaceError::from)?
        else {
            return Err(WorkspaceError::Detached.into());
        };
        Ok((output, exec.id))
    }

    async fn exit_code(&self, exec_id: &str) -> Result<Option<i64>, FileError> {
        let inspected = self
            .docker()
            .inspect_exec(exec_id)
            .await
            .map_err(WorkspaceError::from)?;
        Ok(inspected.exit_code)
    }
}

/// `bash -c SCRIPT name ARGUMENTS...`, so arguments never become shell text.
fn script_argv(script_and_arguments: Vec<String>) -> Vec<String> {
    let mut parts = script_and_arguments.into_iter();
    let script = parts.next().unwrap_or_default();
    let mut argv = vec![
        "bash".to_owned(),
        "-c".to_owned(),
        script,
        "elysium-files".to_owned(),
    ];
    argv.extend(parts);
    argv
}

fn refusal(exit: Option<i64>, path: &str) -> FileError {
    match exit {
        Some(EXIT_MISSING) => FileError::NotFound(path.to_owned()),
        Some(EXIT_OUTSIDE) => FileError::Outside(path.to_owned()),
        Some(code) => FileError::Failed(format!("{path}: the workspace exited with status {code}")),
        None => FileError::Failed(format!("{path}: the workspace did not report how it ended")),
    }
}

/// Reads the listing's NUL-separated triples. A triple that does not parse is skipped, which
/// only a file renamed mid-listing could cause.
fn parse_listing(printed: &[u8]) -> Vec<ListedFile> {
    let fields: Vec<&[u8]> = printed.split(|byte| *byte == 0).collect();
    let mut files = Vec::with_capacity(fields.len() / 3);
    for [size, sha256, path] in fields.as_chunks::<3>().0 {
        let (Ok(size), Ok(sha256), Ok(path)) = (
            std::str::from_utf8(size),
            std::str::from_utf8(sha256),
            std::str::from_utf8(path),
        ) else {
            continue;
        };
        let Ok(size_bytes) = size.parse() else {
            continue;
        };
        files.push(ListedFile {
            path: path.to_owned(),
            size_bytes,
            sha256: sha256.to_owned(),
        });
    }
    files
}

/// Ends a body with an error when it carries more or fewer bytes than were announced.
struct ExactLength {
    inner: BoxStream<'static, Result<Bytes, FileError>>,
    expected: u64,
    seen: u64,
    path: String,
    finished: bool,
}

impl ExactLength {
    fn new(
        inner: BoxStream<'static, Result<Bytes, FileError>>,
        expected: u64,
        path: String,
    ) -> Self {
        Self {
            inner,
            expected,
            seen: 0,
            path,
            finished: false,
        }
    }
}

impl Stream for ExactLength {
    type Item = Result<Bytes, FileError>;

    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `cx`"
    )]
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        match self.inner.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.seen += chunk.len() as u64;
                if self.seen > self.expected {
                    self.finished = true;
                    let error = format!("{} grew while it was read", self.path);
                    return Poll::Ready(Some(Err(FileError::Failed(error))));
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(None) => {
                self.finished = true;
                if self.seen == self.expected {
                    return Poll::Ready(None);
                }
                let error = format!(
                    "{} ended after {} of {} bytes",
                    self.path, self.seen, self.expected
                );
                Poll::Ready(Some(Err(FileError::Failed(error))))
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_reads_nul_separated_triples() {
        let printed = b"12\x00abc\x00model.glb\x005\x00def\x00renders/front view.png\x00";
        assert_eq!(
            parse_listing(printed),
            vec![
                ListedFile {
                    path: "model.glb".to_owned(),
                    size_bytes: 12,
                    sha256: "abc".to_owned()
                },
                ListedFile {
                    path: "renders/front view.png".to_owned(),
                    size_bytes: 5,
                    sha256: "def".to_owned()
                },
            ]
        );
        assert!(parse_listing(b"").is_empty());
    }

    #[tokio::test]
    async fn a_body_shorter_or_longer_than_announced_fails() {
        let chunks = |parts: &[&'static [u8]]| {
            futures_util::stream::iter(
                parts
                    .iter()
                    .map(|part| Ok::<_, FileError>(Bytes::from_static(part)))
                    .collect::<Vec<_>>(),
            )
            .boxed()
        };

        let exact: Vec<_> = ExactLength::new(chunks(&[b"ab", b"c"]), 3, "f".to_owned())
            .collect()
            .await;
        assert!(exact.iter().all(Result::is_ok));

        let short: Vec<_> = ExactLength::new(chunks(&[b"ab"]), 3, "f".to_owned())
            .collect()
            .await;
        assert!(
            matches!(short.last(), Some(Err(FileError::Failed(message))) if message.contains("2 of 3"))
        );

        let long: Vec<_> = ExactLength::new(chunks(&[b"abcd"]), 3, "f".to_owned())
            .collect()
            .await;
        assert!(
            matches!(long.last(), Some(Err(FileError::Failed(message))) if message.contains("grew"))
        );
    }
}
