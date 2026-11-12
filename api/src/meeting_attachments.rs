// Copyright © 2026 Jalapeno Labs

//! What a meeting keeps beside its transcript: files people attach to it, and stills of its video
//! pinned to it, how each is named, kept, and served, and how a file's text is read. The rows are
//! [`crate::models::meeting_attachment`], what a meeting does with them is
//! [`crate::meetings::attachments`], and the routes are under `/api/v1/meetings/{id}/attachments`.
//! See `docs/meetings.md`, Attachments and frames.
//!
//! Everything here but the largest file is a policy that must not vary per deployment, so it is
//! constants in code; the largest file is an admin's setting (`workspace_settings`), 1 GB unless
//! they choose another. Clients read all of it from `GET /api/v1/meeting-attachments`.
//!
//! # Any file
//!
//! A meeting takes any file, since what a meeting was about can be anything: a deck, a spec, a
//! spreadsheet, a build. Nothing the browser says about a file is believed, and nothing but a
//! proven image ever renders on Elysium's origin:
//!
//! - **The type comes from the extension**, through [`KNOWN_TYPES`]; any other file is served as
//!   bytes (`application/octet-stream`).
//! - **Only images display.** An image file is taken only when its first bytes prove it is the
//!   image its name says, as on pages (`crate::page_attachments`), and a frame is re-encoded from
//!   its pixels by Elysium itself (`crate::images::compress_frame`). Everything else is sent with
//!   `Content-Disposition: attachment`, so a browser saves it and never renders it, with `/api`'s
//!   sandboxing Content Security Policy and `nosniff` behind that (`docs/security.md`).
//!
//! That is why a meeting can take what a page refuses, such as HTML or a program: a page's files
//! are what its text links to and shows, where a meeting's are kept with it, read for search, and
//! downloaded.

use image::ImageFormat;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::file_names;
use crate::page_attachments;

/// The most an admin may let a meeting take: what a storage provider takes in one upload, since a
/// file is kept whole (`crate::storage::MAX_UPLOAD_BYTES`).
pub const MAX_BYTES_CEILING: u64 = crate::storage::MAX_UPLOAD_BYTES;

/// The most files and frames one meeting holds. Nothing needs a limit to work; it bounds what a
/// meeting's page lists and what segmenting's frames and an agent's loop can pile on.
pub const MAX_PER_MEETING: i64 = 500;

/// The most of a file's text kept for search, in characters: a long specification, about 130
/// pages. What is past it is not searched, which the file's row does not need to say, since the
/// file itself is a download away.
pub const TEXT_MAX_CHARACTERS: usize = 200_000;

/// The largest file whose text is read. Past it, a file is almost always media or an archive,
/// and reading a document that large would hold the workspace for minutes.
pub const READ_TEXT_MAX_BYTES: u64 = 200_000_000;

/// The longest a caption may be, in characters, as `meeting_attachments.caption` allows.
pub const MAX_CAPTION_CHARACTERS: usize = 1000;

/// The longest a name may be, in characters, as `meeting_attachments.file_name` allows.
const MAX_NAME_CHARACTERS: usize = 200;

/// How many of a file's first bytes prove what image it is, as on pages.
pub const SIGNATURE_BYTES: usize = page_attachments::SIGNATURE_BYTES;

/// Where a meeting's files sit in their storage location, beside `page-attachments/`.
const STORAGE_DIRECTORY: &str = "meeting-attachments";

/// What a file's text is read with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextReader {
    /// Text as it is, read as UTF-8.
    Plain,
    /// A PDF, through `pdftotext` in the workspace.
    Pdf,
    /// An Office Open XML or `OpenDocument` file, a zip of XML, read in the workspace.
    Office,
}

/// A kind of file Elysium knows by its extension.
#[derive(Debug, PartialEq, Eq)]
pub struct KnownType {
    /// Lowercase, without its dot.
    extension: &'static str,
    content_type: &'static str,
    /// Set for an image a browser displays. Its bytes must be this format.
    image: Option<ImageFormat>,
    /// How its text is read; `None` for a kind that holds none to read.
    reader: Option<TextReader>,
}

impl KnownType {
    const fn image(
        extension: &'static str,
        content_type: &'static str,
        format: ImageFormat,
    ) -> Self {
        Self {
            extension,
            content_type,
            image: Some(format),
            reader: None,
        }
    }

    const fn read(extension: &'static str, content_type: &'static str, reader: TextReader) -> Self {
        Self {
            extension,
            content_type,
            image: None,
            reader: Some(reader),
        }
    }

    const fn bytes(extension: &'static str, content_type: &'static str) -> Self {
        Self {
            extension,
            content_type,
            image: None,
            reader: None,
        }
    }
}

/// Every kind of file whose type is known by its extension. Any other is served as bytes, and its
/// text is not read. SVG and HTML are known so their text is not mistaken for plain text, and are
/// served as bytes, since either can carry a script.
const KNOWN_TYPES: &[KnownType] = &[
    KnownType::image("png", "image/png", ImageFormat::Png),
    KnownType::image("jpg", "image/jpeg", ImageFormat::Jpeg),
    KnownType::image("jpeg", "image/jpeg", ImageFormat::Jpeg),
    KnownType::image("gif", "image/gif", ImageFormat::Gif),
    KnownType::image("webp", "image/webp", ImageFormat::WebP),
    KnownType::bytes("svg", "application/octet-stream"),
    KnownType::bytes("html", "application/octet-stream"),
    KnownType::bytes("htm", "application/octet-stream"),
    KnownType::read("pdf", "application/pdf", TextReader::Pdf),
    KnownType::read("txt", "text/plain; charset=utf-8", TextReader::Plain),
    KnownType::read("log", "text/plain; charset=utf-8", TextReader::Plain),
    KnownType::read("md", "text/markdown; charset=utf-8", TextReader::Plain),
    KnownType::read("csv", "text/csv; charset=utf-8", TextReader::Plain),
    KnownType::read(
        "tsv",
        "text/tab-separated-values; charset=utf-8",
        TextReader::Plain,
    ),
    KnownType::read("json", "application/json", TextReader::Plain),
    KnownType::read("yaml", "application/yaml", TextReader::Plain),
    KnownType::read("yml", "application/yaml", TextReader::Plain),
    KnownType::read("vtt", "text/vtt; charset=utf-8", TextReader::Plain),
    KnownType::read("srt", "application/x-subrip", TextReader::Plain),
    KnownType::bytes("doc", "application/msword"),
    KnownType::read(
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        TextReader::Office,
    ),
    KnownType::bytes("xls", "application/vnd.ms-excel"),
    KnownType::read(
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        TextReader::Office,
    ),
    KnownType::bytes("ppt", "application/vnd.ms-powerpoint"),
    KnownType::read(
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        TextReader::Office,
    ),
    KnownType::read(
        "odt",
        "application/vnd.oasis.opendocument.text",
        TextReader::Office,
    ),
    KnownType::read(
        "ods",
        "application/vnd.oasis.opendocument.spreadsheet",
        TextReader::Office,
    ),
    KnownType::read(
        "odp",
        "application/vnd.oasis.opendocument.presentation",
        TextReader::Office,
    ),
    KnownType::bytes("zip", "application/zip"),
    KnownType::bytes("gz", "application/gzip"),
    KnownType::bytes("tar", "application/x-tar"),
    KnownType::bytes("7z", "application/x-7z-compressed"),
    KnownType::bytes("mp4", "video/mp4"),
    KnownType::bytes("mov", "video/quicktime"),
    KnownType::bytes("webm", "video/webm"),
    KnownType::bytes("mp3", "audio/mpeg"),
    KnownType::bytes("wav", "audio/wav"),
    KnownType::bytes("m4a", "audio/mp4"),
];

/// What a file is served as when its kind is not known.
const UNKNOWN_TYPE: &str = "application/octet-stream";

/// The type a stored file named `file_name` is served as.
pub fn content_type_of(file_name: &str) -> &'static str {
    known_type_of(file_name).map_or(UNKNOWN_TYPE, |known| known.content_type)
}

/// How a stored file named `file_name` has its text read, if it does.
pub fn reader_of(file_name: &str) -> Option<TextReader> {
    known_type_of(file_name).and_then(|known| known.reader)
}

/// Whether a file served as `content_type` displays in a page: a proven image, as on pages.
pub fn displays_inline(content_type: &str) -> bool {
    page_attachments::displays_inline(content_type)
}

/// The extension of `file_name`, in lowercase, or `bin` for a name with none: what a copy of it is
/// named by in the workspace, where a reader opens it.
pub fn extension_of(file_name: &str) -> String {
    file_name.rsplit_once('.').map_or_else(
        || "bin".to_owned(),
        |(_stem, extension)| extension.to_ascii_lowercase(),
    )
}

fn known_type_of(file_name: &str) -> Option<&'static KnownType> {
    let (_stem, extension) = file_name.rsplit_once('.')?;
    KNOWN_TYPES
        .iter()
        .find(|known| known.extension == extension)
}

/// A file's name once it is safe to keep (`crate::file_names`), with the kind of file it names.
#[derive(Debug)]
pub struct FileName {
    name: String,
    known: Option<&'static KnownType>,
}

impl FileName {
    /// The name `raw` stands for.
    ///
    /// # Errors
    /// `400` with the code `meeting_attachment_name` when no name is left.
    pub fn parse(raw: &str) -> Result<Self, ApiError> {
        let Some(cleaned) = file_names::clean(raw) else {
            return Err(ApiError::CodedBadRequest {
                code: "meeting_attachment_name",
                message: "a file needs a name to be attached".to_owned(),
            });
        };
        let name = cleaned.into_name(MAX_NAME_CHARACTERS);
        let known = known_type_of(&name);
        Ok(Self { name, known })
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// Refuses a file named as an image whose `head`, its first bytes, is not that image, so only a
    /// proven image ever displays.
    ///
    /// # Errors
    /// `400` with the code `meeting_attachment_not_an_image`.
    pub fn require_signature(&self, head: &[u8]) -> Result<(), ApiError> {
        let Some(format) = self.known.and_then(|known| known.image) else {
            return Ok(());
        };
        if image::guess_format(head).is_ok_and(|found| found == format) {
            return Ok(());
        }
        Err(ApiError::CodedBadRequest {
            code: "meeting_attachment_not_an_image",
            message: format!("{} is not the image its name says", self.name),
        })
    }
}

/// Where attachment `id` of meeting `meeting_id` is kept, relative to its location's directory.
/// Named for people too: anyone looking in the bucket can tell what a file is, without Elysium.
pub fn storage_path(meeting_id: Uuid, id: Uuid, file_name: &str) -> String {
    format!(
        "{STORAGE_DIRECTORY}/{meeting_id}/{id}/{}",
        page_attachments::plain_name(file_name)
    )
}

/// Where a frame segmenting pinned from capture `capture_id` at `capture_ms` into it is kept: named
/// by its moment rather than its row, so an attempt after one cut short keeps the same picture at
/// the same place and leaves no copy that no row names.
pub fn frame_path(meeting_id: Uuid, capture_id: Uuid, capture_ms: i64) -> String {
    format!("{STORAGE_DIRECTORY}/{meeting_id}/frames/{capture_id}-{capture_ms}.webp")
}

/// The address an attachment is served at. Its last segment is there for whoever reads the
/// address; the route finds the file by its id.
pub fn address(meeting_id: Uuid, id: Uuid, file_name: &str) -> String {
    format!(
        "/api/v1/meetings/{meeting_id}/attachments/{id}/{}",
        page_attachments::plain_name(file_name)
    )
}

/// What a frame of a capture's video at `capture_ms` into it is called: `Frame at 12m04s.webp`,
/// so a frame downloaded still says where it came from.
pub fn frame_name(capture_ms: i64) -> String {
    let seconds = capture_ms.max(0) / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        return format!("Frame at {hours}h{minutes:02}m{seconds:02}s.webp");
    }
    format!("Frame at {minutes}m{seconds:02}s.webp")
}

/// `caption` trimmed, and refused when nothing is left or it is too long.
///
/// # Errors
/// `400` with the code `meeting_frame_caption`.
pub fn caption(caption: &str) -> Result<String, ApiError> {
    let trimmed = caption.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_CAPTION_CHARACTERS {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_frame_caption",
            message: format!(
                "a frame's caption says what it shows, in 1 to {MAX_CAPTION_CHARACTERS} characters"
            ),
        });
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal_code(result: Result<impl std::fmt::Debug, ApiError>) -> &'static str {
        match result {
            Err(ApiError::CodedBadRequest { code, .. }) => code,
            other => panic!("should have been refused, not {other:?}"),
        }
    }

    #[test]
    fn any_file_is_taken_and_only_a_known_type_is_served_as_itself() {
        for (raw, name, content_type) in [
            (
                "Q3 Deck (final).PPTX",
                "Q3 Deck (final).pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            (
                "../../etc/notes.md",
                "notes.md",
                "text/markdown; charset=utf-8",
            ),
            ("build.exe", "build.exe", "application/octet-stream"),
            ("page.html", "page.html", "application/octet-stream"),
            ("diagram.svg", "diagram.svg", "application/octet-stream"),
            ("LICENSE", "LICENSE", "application/octet-stream"),
        ] {
            let parsed = FileName::parse(raw).expect("a name");
            assert_eq!(parsed.as_str(), name, "{raw}");
            assert_eq!(content_type_of(parsed.as_str()), content_type, "{raw}");
        }
        for raw in ["", "   ", ".png", "dir/"] {
            assert_eq!(
                refusal_code(FileName::parse(raw)),
                "meeting_attachment_name",
                "{raw:?}"
            );
        }
    }

    #[test]
    fn nothing_but_a_proven_image_displays() {
        let png = FileName::parse("slide.png").expect("a name");
        png.require_signature(b"\x89PNG\r\n\x1a\n\0\0\0\0")
            .expect("a PNG");
        assert_eq!(
            refusal_code(png.require_signature(b"<html><script>")),
            "meeting_attachment_not_an_image"
        );
        // A file no image is named as needs no proof, and never displays.
        let page = FileName::parse("page.html").expect("a name");
        page.require_signature(b"<html><script>")
            .expect("taken as bytes");
        assert!(!displays_inline(content_type_of(page.as_str())));
        assert!(displays_inline(content_type_of(png.as_str())));
        assert!(displays_inline("image/webp"));
    }

    #[test]
    fn a_files_text_is_read_by_its_kind() {
        assert_eq!(reader_of("spec.pdf"), Some(TextReader::Pdf));
        assert_eq!(reader_of("plan.docx"), Some(TextReader::Office));
        assert_eq!(reader_of("deck.odp"), Some(TextReader::Office));
        assert_eq!(reader_of("data.csv"), Some(TextReader::Plain));
        for name in [
            "slide.png",
            "demo.mp4",
            "page.html",
            "build.exe",
            "LICENSE",
            "old.doc",
        ] {
            assert_eq!(reader_of(name), None, "{name}");
        }
    }

    #[test]
    fn every_extension_is_written_as_names_are_compared() {
        for known in KNOWN_TYPES {
            assert!(
                known
                    .extension
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit()),
                "{}",
                known.extension
            );
            assert!(known.image.is_none() || known.reader.is_none());
        }
    }

    /// The admin's setting starts where the design says, 1 GB, and the database holds it beneath
    /// the ceiling as this module does.
    #[test]
    fn the_largest_file_starts_at_a_gigabyte_and_never_passes_the_ceiling() {
        let migration = include_str!(
            "../migrations/2026-11-12-010000-0250_attach_files_and_frames_to_meetings/up.sql"
        );
        assert!(migration.contains("DEFAULT 1000000000"));
        assert!(migration.contains(&format!("BETWEEN 1 AND {MAX_BYTES_CEILING}")));
    }

    #[test]
    fn a_frame_is_named_for_where_it_was_taken() {
        assert_eq!(frame_name(724_500), "Frame at 12m04s.webp");
        assert_eq!(frame_name(3_725_000), "Frame at 1h02m05s.webp");
        assert_eq!(frame_name(-5), "Frame at 0m00s.webp");
    }

    #[test]
    fn paths_and_addresses_name_the_meeting_and_the_file() {
        let meeting = Uuid::nil();
        let id = Uuid::from_u128(7);
        assert_eq!(
            storage_path(meeting, id, "Q3 Deck.pptx"),
            "meeting-attachments/00000000-0000-0000-0000-000000000000/\
             00000000-0000-0000-0000-000000000007/Q3-Deck.pptx"
        );
        assert_eq!(
            address(meeting, id, "Frame at 12m04s.webp"),
            "/api/v1/meetings/00000000-0000-0000-0000-000000000000/attachments/\
             00000000-0000-0000-0000-000000000007/Frame-at-12m04s.webp"
        );
    }

    #[test]
    fn a_caption_is_trimmed_and_bounded() {
        assert_eq!(
            caption("  The Q3 roadmap  ").expect("a caption"),
            "The Q3 roadmap"
        );
        assert_eq!(refusal_code(caption("   ")), "meeting_frame_caption");
        assert_eq!(
            refusal_code(caption(&"a".repeat(MAX_CAPTION_CHARACTERS + 1))),
            "meeting_frame_caption"
        );
    }
}
