// Copyright © 2026 Jalapeno Labs

//! The meeting attachment workflow: a file a person attached to a meeting, stored and read. See
//! `docs/meetings.md`, Attachments and frames, and `docs/engine.md`, Workflows and steps.
//!
//! A file arrives as an upload's parts, staged in the workspace (`crate::meeting_uploads`), and
//! finishing the upload records it on the meeting and starts its run of this workflow
//! ([`workflow`]), started by the person who attached it:
//!
//! 1. [`StoreAttachment`] streams the parts, joined, to the storage location it was attached to,
//!    records where it is kept, and removes the parts.
//! 2. [`ReadAttachmentText`] reads the text of a file whose kind holds any
//!    (`crate::meeting_attachments::TextReader`) and indexes it: with the meeting's own document,
//!    and with every excerpt it is pinned in (`super::attachments::refresh_pinned`).
//!
//! # Reading text
//!
//! Plain text is read as it streams from storage. A PDF is read by `pdftotext` and an Office or
//! OpenDocument file by a short Python program, each in the workspace container, as a meeting's
//! media is cut there (`super::media`): a file someone chose is read in a container of its own
//! rather than by a parser inside the engine, and the engine's memory stays flat whatever its
//! size. What is read is cut to `TEXT_MAX_CHARACTERS`. A file that cannot be read is recorded as
//! `unreadable` and the step still succeeds: the file itself is kept and downloads, and nothing a
//! retry could change would read it.
//!
//! # What it logs
//!
//! Ids, sizes, and how reading went, never a file's name or a word of its text: every member reads
//! a run's log.

use std::sync::Arc;

use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent_models::AgentKind;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::meeting_attachments::{self, READ_TEXT_MAX_BYTES, TEXT_MAX_CHARACTERS, TextReader};
use crate::meeting_uploads::staging::Staging;
use crate::meeting_uploads::{FileKind, part_count};
use crate::models::meeting;
use crate::models::meeting_attachment::{self, MeetingAttachment, TextState};
use crate::models::meeting_upload;
use crate::models::storage_location;
use crate::realtime::{EventBus, ServerEvent};
use crate::runner::WorkspacePath;
use crate::runner::files::FileError;
use crate::runner::workspace::{AGENT_USER, SCRIPT_TIMEOUT, WorkspaceContainer};
use crate::storage::Storage;
use crate::workflows::{self, Done, Start, Step, StepContext, StepFailure, StepLog, Workflow};

/// What a run of the workflow is about, as `workflow_runs.source_kind` names it: the attachment.
pub const SOURCE_KIND: &str = "meeting_attachment";

/// The workflow's name, as `workflow_runs.workflow` names it.
pub const NAME: &str = "meeting_attachment";

/// Reads a PDF's text into a file. `$1` is the PDF, `$2` the text.
const PDF_SCRIPT: &str = r#"
set -euo pipefail
pdftotext -enc UTF-8 -q -- "$1" "$2"
"#;

/// Reads an Office Open XML or OpenDocument file's text into a file: every paragraph, heading,
/// and spreadsheet string, one a line, in the order the file keeps them. `$1` is the file, `$2`
/// the text, `$3` the most characters to write.
///
/// A document is a zip of XML parts. Only the parts that hold what someone wrote are read (a
/// document's body and notes, a deck's slides then their notes in order, a workbook's strings, an
/// OpenDocument's content), each at most 64 MiB unpacked, so a zip that unpacks to far more than
/// it holds reads nothing past that. Python's own XML parser expands no external entities, and the
/// expat it runs on refuses amplifying ones.
const OFFICE_SCRIPT: &str = r#"
set -euo pipefail
python3 -I - "$1" "$2" "$3" <<'PYTHON'
import re
import sys
import zipfile
import xml.etree.ElementTree as tree

source, target, limit = sys.argv[1], sys.argv[2], int(sys.argv[3])
PART_MAX_BYTES = 64 * 1024 * 1024
# Word's, DrawingML's and OpenDocument's paragraphs and headings, and a workbook's strings.
PARAGRAPHS = {"p", "h", "si"}


def numbered(names, pattern):
    found = []
    for name in names:
        match = re.fullmatch(pattern, name)
        if match:
            found.append((int(match.group(1)), name))
    return [name for _number, name in sorted(found)]


def parts_of(names):
    if "word/document.xml" in names:
        notes = [name for name in ("word/footnotes.xml", "word/endnotes.xml") if name in names]
        return ["word/document.xml"] + notes
    if "ppt/presentation.xml" in names:
        slides = numbered(names, r"ppt/slides/slide(\d+)\.xml")
        notes = numbered(names, r"ppt/notesSlides/notesSlide(\d+)\.xml")
        return slides + notes
    if "xl/workbook.xml" in names:
        return [name for name in ("xl/sharedStrings.xml",) if name in names]
    if "content.xml" in names:
        return ["content.xml"]
    return []


def paragraphs(element, found):
    local = element.tag.rpartition("}")[2]
    if local in PARAGRAPHS:
        text = " ".join("".join(element.itertext()).split())
        if text:
            found.append(text)
        return
    for child in element:
        paragraphs(child, found)


written = 0
with zipfile.ZipFile(source) as archive, open(target, "w", encoding="utf-8") as output:
    names = archive.namelist()
    for name in parts_of(names):
        if archive.getinfo(name).file_size > PART_MAX_BYTES:
            continue
        found = []
        paragraphs(tree.fromstring(archive.read(name)), found)
        for text in found:
            if written >= limit:
                sys.exit(0)
            line = text[: limit - written]
            output.write(line + "\n")
            written += len(line) + 1
PYTHON
"#;

/// A file a run stores and reads: the attachment, and the upload whose parts it arrived in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFile {
    pub attachment_id: Uuid,
    /// The upload it arrived in; `None` for a file already stored.
    pub upload_id: Option<Uuid>,
}

/// The meeting attachment workflow: a file stored, then read.
pub fn workflow() -> Workflow<AttachmentFile> {
    workflows::declare(NAME)
        .first::<StoreAttachment>()
        .then::<ReadAttachmentText>()
        .build()
}

/// The run that stores and reads `file`, attached by `created_by`.
pub fn run_for(file: AttachmentFile, created_by: Uuid) -> crate::models::workflow_run::NewRun {
    workflow().new_run(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: file.attachment_id.to_string(),
        input: file,
        created_by,
    })
}

/// What the steps are built from.
#[derive(Clone)]
pub struct AttachmentDeps {
    pub database: Pool,
    pub events: EventBus,
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    pub workspace: WorkspaceContainer,
    pub staging: Staging,
}

struct Inner {
    database: Pool,
    events: EventBus,
    cipher: Arc<Cipher>,
    storage: Storage,
    workspace: WorkspaceContainer,
    staging: Staging,
}

impl Inner {
    async fn connection(
        &self,
    ) -> Result<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>, StepFailure>
    {
        self.database.get().await.map_err(|error| {
            StepFailure::retryable(format!("No database connection available: {error}"))
        })
    }

    /// Attachment `id`, or a failure for good when it is gone, which no retry brings back.
    async fn attachment(&self, id: Uuid) -> Result<MeetingAttachment, StepFailure> {
        let mut connection = self.connection().await?;
        match meeting_attachment::find_by_id(&mut connection, id).await {
            Ok(attachment) => Ok(attachment),
            Err(diesel::result::Error::NotFound) => {
                Err(StepFailure::permanent("The attachment is gone"))
            }
            Err(error) => Err(StepFailure::retryable(error)),
        }
    }

    /// Tells every client that `attachment`'s meeting changed, so its page lists it as it now is.
    async fn announce(&self, attachment: &MeetingAttachment) -> Result<(), StepFailure> {
        let mut connection = self.connection().await?;
        let changed = meeting::find(&mut connection, attachment.meeting_id)
            .await
            .map_err(StepFailure::retryable)?;
        self.events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state.clone(),
            created_by: changed.created_by,
            entry_id: None,
        });
        Ok(())
    }
}

impl std::fmt::Debug for Inner {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("AttachmentSteps").finish_non_exhaustive()
    }
}

/// Stores a file's parts where it was attached. See the module's docs.
#[derive(Debug, Clone)]
pub struct StoreAttachment {
    inner: Arc<Inner>,
}

impl StoreAttachment {
    pub fn new(deps: AttachmentDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                cipher: deps.cipher,
                storage: deps.storage,
                workspace: deps.workspace,
                staging: deps.staging,
            }),
        }
    }

    async fn store(
        &self,
        input: AttachmentFile,
        log: &StepLog,
    ) -> Result<Done<AttachmentFile>, StepFailure> {
        let attachment = self.inner.attachment(input.attachment_id).await?;
        if attachment.stored_at.is_some() {
            log.line("Stored already");
            return Ok(stored(input, "Stored already"));
        }
        let Some(upload_id) = input.upload_id else {
            return Err(StepFailure::permanent(
                "The attachment names no upload to store it from",
            ));
        };
        let mut connection = self.inner.connection().await?;
        let staged = meeting_upload::find_owned(&mut connection, upload_id, attachment.created_by)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => {
                    StepFailure::permanent("The upload the file arrived in is gone")
                }
                other => StepFailure::retryable(other),
            })?;
        let Some(file) = staged.file(FileKind::Attachment) else {
            return Err(StepFailure::permanent("The upload holds no attached file"));
        };
        let size_bytes = file.size_bytes();
        let parts = part_count(size_bytes, staged.upload.part_bytes());
        if staged.upload.parts_removed_at.is_some() {
            return Err(StepFailure::permanent(
                "The file's parts are no longer in the workspace; attach it again",
            ));
        }
        let location = storage_location::find(&mut connection, attachment.storage_location_id)
            .await
            .map_err(StepFailure::retryable)?;
        // The file may take a while to store; no connection is held while it does.
        drop(connection);

        let access_key = location.access_key(&self.inner.cipher).map_err(|error| {
            StepFailure::permanent(format!("The location's key cannot be decrypted: {error}"))
        })?;
        let path = meeting_attachments::storage_path(
            attachment.meeting_id,
            attachment.id,
            &attachment.file_name,
        );
        let joined = self
            .inner
            .staging
            .read((upload_id, FileKind::Attachment), parts);
        self.inner
            .storage
            .upload(
                &location,
                &access_key,
                &path,
                joined,
                size_bytes,
                Some(&attachment.content_type),
            )
            .await
            .map_err(StepFailure::retryable)?;
        log.line(&format!(
            "Stored {size_bytes} bytes from {parts} parts at {path}"
        ));

        let mut connection = self.inner.connection().await?;
        meeting_attachment::record_stored(&mut connection, attachment.id, &path, Utc::now())
            .await
            .map_err(StepFailure::retryable)?;
        // Once stored, the parts are no longer needed; one left behind is the sweep's to remove.
        match self.inner.staging.remove(upload_id).await {
            Ok(()) => {
                meeting_upload::record_parts_removed(&mut connection, upload_id)
                    .await
                    .map_err(StepFailure::retryable)?;
            }
            Err(error) => log.line(&format!(
                "The parts could not be removed yet, so the sweep will: {error}"
            )),
        }
        drop(connection);
        self.inner.announce(&attachment).await?;
        Ok(stored(input, "Stored"))
    }
}

/// The step's answer once the file is stored: what the read takes.
fn stored(input: AttachmentFile, summary: &str) -> Done<AttachmentFile> {
    Done::Succeeded {
        output: input,
        summary: Some(summary.to_owned()),
    }
}

impl Step for StoreAttachment {
    const NAME: &'static str = "store_attachment";
    /// Moves bytes; no model of Claude's runs.
    const AGENT_KIND: Option<AgentKind> = None;
    type Input = AttachmentFile;
    type Output = AttachmentFile;

    async fn run(
        &self,
        input: AttachmentFile,
        context: StepContext,
    ) -> Result<Done<AttachmentFile>, StepFailure> {
        self.store(input, &context.log).await
    }
}

/// Reads a stored file's text and indexes it. See the module's docs.
#[derive(Debug, Clone)]
pub struct ReadAttachmentText {
    inner: Arc<Inner>,
}

impl ReadAttachmentText {
    pub fn new(deps: AttachmentDeps) -> Self {
        Self {
            inner: StoreAttachment::new(deps).inner,
        }
    }

    async fn read(
        &self,
        input: AttachmentFile,
        log: &StepLog,
    ) -> Result<Done<AttachmentFile>, StepFailure> {
        let attachment = self.inner.attachment(input.attachment_id).await?;
        if attachment.text_state() != Some(TextState::Pending) {
            return Ok(Done::Skipped {
                reason: "Nothing to read".to_owned(),
            });
        }
        let Some(path) = attachment.storage_path.clone() else {
            return Ok(Done::Skipped {
                reason: "Not stored, so nothing to read".to_owned(),
            });
        };
        let reader = meeting_attachments::reader_of(&attachment.file_name);
        let size_bytes = u64::try_from(attachment.size_bytes).unwrap_or(u64::MAX);
        let read = match reader {
            None => Read::Nothing,
            Some(_) if size_bytes > READ_TEXT_MAX_BYTES => {
                log.line(&format!(
                    "{size_bytes} bytes is more than a file whose text is read, {READ_TEXT_MAX_BYTES}"
                ));
                Read::Nothing
            }
            Some(reader) => self.read_text(&attachment, &path, reader, log).await?,
        };
        let (state, text) = match &read {
            Read::Text(text) => (TextState::Read, Some(text.as_str())),
            Read::Nothing => (TextState::None, None),
            Read::Unreadable => (TextState::Unreadable, None),
        };

        let mut connection = self.inner.connection().await?;
        connection
            .transaction(async |connection| {
                let current = meeting::lock(connection, attachment.meeting_id).await?;
                meeting_attachment::record_text(connection, attachment.id, state, text).await?;
                super::attachments::refresh_pinned(connection, &current, attachment.id).await
            })
            .await
            .map_err(StepFailure::retryable)?;
        drop(connection);
        self.inner.announce(&attachment).await?;
        let summary = match read {
            Read::Text(text) => format!("Read {} characters", text.chars().count()),
            Read::Nothing => "No text to read".to_owned(),
            Read::Unreadable => "Its text could not be read".to_owned(),
        };
        log.line(&summary);
        Ok(Done::Succeeded {
            output: input,
            summary: Some(summary),
        })
    }

    /// What `reader` reads of `attachment`, kept at `path`. A provider or workspace that does not
    /// answer is retried; a file that does not read is [`Read::Unreadable`].
    async fn read_text(
        &self,
        attachment: &MeetingAttachment,
        path: &str,
        reader: TextReader,
        log: &StepLog,
    ) -> Result<Read, StepFailure> {
        let mut connection = self.inner.connection().await?;
        let location = storage_location::find(&mut connection, attachment.storage_location_id)
            .await
            .map_err(StepFailure::retryable)?;
        drop(connection);
        let access_key = location.access_key(&self.inner.cipher).map_err(|error| {
            StepFailure::permanent(format!("The location's key cannot be decrypted: {error}"))
        })?;
        let download = self
            .inner
            .storage
            .download(&location, &access_key, path)
            .await
            .map_err(StepFailure::retryable)?;
        let script = match reader {
            TextReader::Plain => {
                let bytes = read_up_to(download.body, TEXT_MAX_CHARACTERS * 4)
                    .await
                    .map_err(StepFailure::retryable)?;
                return Ok(Read::from_bytes(&bytes));
            }
            TextReader::Pdf => PDF_SCRIPT,
            TextReader::Office => OFFICE_SCRIPT,
        };

        let directory = scratch_directory(attachment.id);
        let source = format!("file.{}", meeting_attachments::extension_of(&attachment.file_name));
        let written = self
            .inner
            .workspace
            .write_file(&directory, &source, download.body)
            .await;
        let read = match written {
            Ok(_) => self.run_reader(&directory, &source, script, log).await,
            Err(error) => Err(StepFailure::retryable(error)),
        };
        // Removed however the read went, so a file someone chose never stays in the workspace.
        if let Err(error) = self.inner.workspace.remove_directory(&directory).await {
            log.line(&format!("The workspace copy could not be removed: {error}"));
        }
        read
    }

    /// Runs `script`, a reader, on `source` in `directory`, and reads back the text it wrote.
    async fn run_reader(
        &self,
        directory: &WorkspacePath,
        source: &str,
        script: &str,
        log: &StepLog,
    ) -> Result<Read, StepFailure> {
        let absolute = directory.absolute();
        let limit = TEXT_MAX_CHARACTERS.to_string();
        let output = self
            .inner
            .workspace
            .run_script(
                AGENT_USER,
                script,
                &[
                    &format!("{absolute}/{source}"),
                    &format!("{absolute}/text.txt"),
                    &limit,
                ],
                Vec::new(),
                SCRIPT_TIMEOUT,
            )
            .await
            .map_err(|error| StepFailure::retryable(FileError::from(error)))?;
        if output.exit_code != Some(0) {
            // What the reader said can quote the file; the log keeps only that it failed.
            log.line(&format!(
                "The reader stopped with exit code {:?}",
                output.exit_code
            ));
            return Ok(Read::Unreadable);
        }
        let text = self
            .inner
            .workspace
            .read_file(directory, "text.txt")
            .await
            .map_err(StepFailure::retryable)?;
        let bytes = read_up_to(text.body, TEXT_MAX_CHARACTERS * 4)
            .await
            .map_err(StepFailure::retryable)?;
        Ok(Read::from_bytes(&bytes))
    }
}

impl Step for ReadAttachmentText {
    const NAME: &'static str = "read_attachment_text";
    /// `pdftotext` and a zip of XML; no model of Claude's runs.
    const AGENT_KIND: Option<AgentKind> = None;
    type Input = AttachmentFile;
    type Output = AttachmentFile;

    async fn run(
        &self,
        input: AttachmentFile,
        context: StepContext,
    ) -> Result<Done<AttachmentFile>, StepFailure> {
        self.read(input, &context.log).await
    }
}

/// What reading a file found.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Read {
    /// Its text, cut to [`TEXT_MAX_CHARACTERS`].
    Text(String),
    /// Nothing to read: a kind that holds no text, or a file too large to read.
    Nothing,
    /// It did not read.
    Unreadable,
}

impl Read {
    /// `bytes` as text: UTF-8, any invalid sequence and every NUL left out, cut to
    /// [`TEXT_MAX_CHARACTERS`] and trimmed.
    fn from_bytes(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let text: String = text
            .chars()
            .filter(|character| *character != '\0' && *character != char::REPLACEMENT_CHARACTER)
            .take(TEXT_MAX_CHARACTERS)
            .collect();
        Self::Text(text.trim().to_owned())
    }
}

/// The first `limit` bytes of `body`, reading no further.
async fn read_up_to<Chunks, ChunkError>(body: Chunks, limit: usize) -> Result<Vec<u8>, ChunkError>
where
    Chunks: futures_util::Stream<Item = Result<bytes::Bytes, ChunkError>>,
{
    let mut chunks = std::pin::pin!(body);
    let mut read = Vec::new();
    while read.len() < limit {
        match chunks.next().await {
            Some(chunk) => {
                let chunk = chunk?;
                let room = limit - read.len();
                read.extend_from_slice(&chunk[..chunk.len().min(room)]);
            }
            None => break,
        }
    }
    Ok(read)
}

/// The workspace directory one attachment is read in.
fn scratch_directory(attachment_id: Uuid) -> WorkspacePath {
    WorkspacePath::new(format!(".elysium/attachments/{attachment_id}"))
        .expect("an attachment's directory is a plain relative path")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_utf8_without_nul_or_broken_bytes_and_bounded() {
        assert_eq!(
            Read::from_bytes(b"  Ship\0 Atlas \xff\xfeon Friday\n"),
            Read::Text("Ship Atlas on Friday".to_owned())
        );
        let Read::Text(long) = Read::from_bytes("a".repeat(TEXT_MAX_CHARACTERS + 10).as_bytes())
        else {
            panic!("text reads as text");
        };
        assert_eq!(long.chars().count(), TEXT_MAX_CHARACTERS);
    }

    #[tokio::test]
    async fn a_body_is_read_no_further_than_asked() {
        let chunks = futures_util::stream::iter([
            Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"abcd")),
            Ok(bytes::Bytes::from_static(b"efgh")),
            Err(std::io::Error::other("never read")),
        ]);
        assert_eq!(read_up_to(chunks, 6).await.expect("reads"), b"abcdef");
    }

    #[test]
    fn the_workflow_stores_then_reads() {
        let outline = workflow().outline();
        assert_eq!(outline.name(), NAME);
        assert_eq!(outline.steps().len(), 2);
    }
}
