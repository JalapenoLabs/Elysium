// Copyright © 2026 Jalapeno Labs

//! Where an upload's parts wait until its meeting's fetch stores them.
//!
//! In the workspace container, as a meeting's media is while it is worked on (`crate::meetings::
//! media`): `.elysium/uploads/<upload>/<kind>/<part>`, each part a file of its own, written through
//! `docker exec` as the agent account like every file Elysium puts there. Neither the API nor the
//! engine mounts the workspace, and both reach it, which is what lets the API take the parts while
//! the person waits and the engine store them on its own time. Parts are named by their number,
//! zero-padded, so a directory listing reads them in order.
//!
//! A part sent twice replaces itself. Only the parts its upload's row counts as received are ever
//! read (`crate::models::meeting_upload`), and a part is counted only once it was written whole,
//! so a part cut short is never read: the next attempt writes it again. Reading joins a file's
//! parts into one stream, part after part, and never holds more than a chunk of one in memory.
//!
//! Tests stage in a directory of their own instead, since no Docker daemon serves them; both read
//! and write through the same few calls.

use bytes::Bytes;
use futures_util::stream::{self, BoxStream};
use futures_util::{Stream, StreamExt, TryStreamExt};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::FileKind;
use crate::runner::WorkspacePath;
use crate::runner::files::FileError;
use crate::runner::workspace::WorkspaceContainer;

/// The workspace directory every upload stages under.
const STAGING_DIRECTORY: &str = ".elysium/uploads";

/// Why staging refused.
#[derive(Debug, thiserror::Error)]
pub enum StagingError {
    #[error(transparent)]
    Workspace(#[from] FileError),
    #[error("could not stage the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("the part arrived with {received} bytes where {expected} were sent")]
    Length { expected: u64, received: u64 },
}

/// Where uploads stage. Cloning is cheap.
#[derive(Debug, Clone)]
pub struct Staging {
    place: Place,
}

#[derive(Debug, Clone)]
enum Place {
    Workspace(WorkspaceContainer),
    /// A directory a test holds, standing in for the workspace's.
    #[cfg(test)]
    Directory(std::sync::Arc<std::path::PathBuf>),
}

impl Staging {
    /// Staging in `workspace`, as the API and the engine stage.
    pub const fn in_workspace(workspace: WorkspaceContainer) -> Self {
        Self {
            place: Place::Workspace(workspace),
        }
    }

    /// Staging in `directory`, for a test.
    #[cfg(test)]
    pub fn in_directory(directory: impl Into<std::path::PathBuf>) -> Self {
        Self {
            place: Place::Directory(std::sync::Arc::new(directory.into())),
        }
    }

    /// Writes part `index` of upload `upload_id`'s file of `kind` from `body`, which must carry
    /// exactly `length` bytes, replacing the part if it arrived before.
    ///
    /// # Errors
    /// [`StagingError::Length`] when the body carried more or fewer, and nothing is left under the
    /// part's name; otherwise the workspace's refusal.
    pub async fn write_part<Chunks, ChunkError>(
        &self,
        (upload_id, kind): (Uuid, FileKind),
        index: u64,
        length: u64,
        body: Chunks,
    ) -> Result<(), StagingError>
    where
        Chunks: Stream<Item = Result<Bytes, ChunkError>> + Send,
        ChunkError: std::fmt::Display,
    {
        let written = match &self.place {
            Place::Workspace(workspace) => {
                workspace
                    .write_file(&root(), &part_path(upload_id, kind, index), body)
                    .await?
            }
            #[cfg(test)]
            Place::Directory(directory) => {
                write_in_directory(directory, &part_path(upload_id, kind, index), body).await?
            }
        };
        if written != length {
            // Its name holds the wrong bytes, which nothing reads: only the parts an upload's row
            // counts as received are ever joined, and the next attempt replaces this one.
            return Err(StagingError::Length {
                expected: length,
                received: written,
            });
        }
        Ok(())
    }

    /// Upload `upload_id`'s file of `kind`, its `parts` parts joined in order, as one stream.
    pub fn read(
        &self,
        (upload_id, kind): (Uuid, FileKind),
        parts: u64,
    ) -> BoxStream<'static, Result<Bytes, StagingError>> {
        let staging = self.clone();
        stream::iter(0..parts)
            .then(move |index| {
                let staging = staging.clone();
                async move { staging.read_part(&part_path(upload_id, kind, index)).await }
            })
            .try_flatten()
            .boxed()
    }

    /// The SHA-256 of upload `upload_id`'s file of `kind`, its `parts` parts joined, as lowercase
    /// hex, read as it streams.
    ///
    /// # Errors
    /// The workspace's refusal, such as a part that is not there.
    pub async fn sha256(&self, file: (Uuid, FileKind), parts: u64) -> Result<String, StagingError> {
        let mut hasher = Sha256::new();
        let mut chunks = self.read(file, parts);
        while let Some(chunk) = chunks.next().await {
            hasher.update(chunk?);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    /// Removes everything upload `upload_id` staged. Nothing there is not an error.
    ///
    /// # Errors
    /// The workspace's refusal.
    pub async fn remove(&self, upload_id: Uuid) -> Result<(), StagingError> {
        match &self.place {
            Place::Workspace(workspace) => {
                workspace
                    .remove_directory(&upload_directory(upload_id))
                    .await?;
            }
            #[cfg(test)]
            Place::Directory(directory) => {
                match tokio::fs::remove_dir_all(directory.join(upload_id.to_string())).await {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(error.into());
                    }
                    _removed => {}
                }
            }
        }
        Ok(())
    }

    /// One part, as a stream of its bytes.
    async fn read_part(
        &self,
        path: &str,
    ) -> Result<BoxStream<'static, Result<Bytes, StagingError>>, StagingError> {
        match &self.place {
            Place::Workspace(workspace) => {
                let file = workspace.read_file(&root(), path).await?;
                Ok(file.body.map_err(StagingError::from).boxed())
            }
            #[cfg(test)]
            Place::Directory(directory) => {
                let file = tokio::fs::File::open(directory.join(path)).await?;
                Ok(tokio_util::io::ReaderStream::new(file)
                    .map_err(StagingError::from)
                    .boxed())
            }
        }
    }
}

/// The directory every upload stages under, as a root no path may leave.
fn root() -> WorkspacePath {
    WorkspacePath::new(STAGING_DIRECTORY).expect("the staging directory is a plain relative path")
}

/// The directory upload `upload_id` stages in.
fn upload_directory(upload_id: Uuid) -> WorkspacePath {
    WorkspacePath::new(format!("{STAGING_DIRECTORY}/{upload_id}"))
        .expect("an upload's directory is a plain relative path")
}

/// Where part `index` of an upload's file of `kind` stages, under [`root`]. Six digits hold the
/// most parts a file has, so the names sort as their numbers do.
fn part_path(upload_id: Uuid, kind: FileKind, index: u64) -> String {
    format!("{upload_id}/{}/{index:06}", kind.as_str())
}

/// A test's write: the body into a partial file, renamed into place once whole, as the
/// workspace's own write does.
#[cfg(test)]
async fn write_in_directory<Chunks, ChunkError>(
    directory: &std::path::Path,
    path: &str,
    body: Chunks,
) -> Result<u64, StagingError>
where
    Chunks: Stream<Item = Result<Bytes, ChunkError>> + Send,
    ChunkError: std::fmt::Display,
{
    use tokio::io::AsyncWriteExt;

    let target = directory.join(path);
    let parent = target.parent().expect("a part's path has a directory");
    tokio::fs::create_dir_all(parent).await?;
    let partial = target.with_extension("partial");
    let mut file = tokio::fs::File::create(&partial).await?;
    let mut written: u64 = 0;
    let mut body = std::pin::pin!(body);
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| std::io::Error::other(error.to_string()))?;
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
    }
    file.flush().await?;
    tokio::fs::rename(&partial, &target).await?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(bytes: &'static [u8]) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
        stream::iter(bytes.chunks(3).map(|chunk| Ok(Bytes::from_static(chunk))))
    }

    #[test]
    fn parts_sort_as_their_numbers_do() {
        let upload = Uuid::nil();
        assert_eq!(
            part_path(upload, FileKind::Recording, 7),
            "00000000-0000-0000-0000-000000000000/recording/000007"
        );
        assert!(
            part_path(upload, FileKind::Recording, 99)
                < part_path(upload, FileKind::Recording, 100)
        );
    }

    #[tokio::test]
    async fn a_files_parts_read_back_joined_in_order_and_hash_as_one() {
        let directory = tempfile::tempdir().expect("a directory");
        let staging = Staging::in_directory(directory.path());
        let file = (Uuid::now_v7(), FileKind::Recording);

        // Out of order, and one sent twice: what is read is each part once, in its place.
        staging
            .write_part(file, 1, 6, chunks(b"second"))
            .await
            .expect("stages");
        staging
            .write_part(file, 0, 5, chunks(b"first"))
            .await
            .expect("stages");
        staging
            .write_part(file, 1, 6, chunks(b"second"))
            .await
            .expect("stages again");

        let joined: Vec<Bytes> = staging.read(file, 2).try_collect().await.expect("reads");
        assert_eq!(joined.concat(), b"firstsecond");
        assert_eq!(
            staging.sha256(file, 2).await.expect("hashes"),
            hex::encode(Sha256::digest(b"firstsecond"))
        );

        staging.remove(file.0).await.expect("removes");
        assert!(staging.sha256(file, 2).await.is_err(), "nothing is left");
        staging
            .remove(file.0)
            .await
            .expect("removing nothing is fine");
    }

    #[tokio::test]
    async fn a_part_that_arrives_short_is_refused() {
        let directory = tempfile::tempdir().expect("a directory");
        let staging = Staging::in_directory(directory.path());
        let file = (Uuid::now_v7(), FileKind::Transcript);

        let short = staging.write_part(file, 0, 10, chunks(b"short")).await;

        assert!(matches!(
            short,
            Err(StagingError::Length {
                expected: 10,
                received: 5
            })
        ));
    }
}
