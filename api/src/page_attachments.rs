// Copyright © 2026 Jalapeno Labs

//! What may be attached to a knowledge base page, and how a file is named and served. The rows
//! themselves are [`crate::models::page_attachment`], and the routes `/api/v1/pages/{id}/attachments`.
//! See `docs/knowledge.md`, Pages, Attachments.
//!
//! Everything here is a policy that must not vary per deployment, so it is constants in code, and
//! clients read them from `GET /api/v1/page-attachments` rather than keeping a copy.
//!
//! Elysium serves these files on its own origin, and whoever uploads one chooses every byte, so
//! nothing the browser says about a file is believed:
//!
//! - **The type comes from the extension**, through [`FILE_TYPES`], which is a list of what is
//!   allowed. Anything that would run in a browser is either off it or, for SVG, is served as bytes.
//! - **Only images display.** An image is shown only when its first bytes prove it is that image
//!   ([`FileType::require_signature`]). Every other file is sent with `Content-Disposition:
//!   attachment`, so a browser saves it and never renders it. `/api`'s sandboxing Content Security
//!   Policy and `nosniff` (`crate::middleware::security_headers`) stand behind that.

use axum::http::HeaderValue;
use image::ImageFormat;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use uuid::Uuid;

use crate::errors::ApiError;

/// The largest file a page takes: 50 MB, room for a datasheet or a slide deck.
///
/// `client_max_body_size` in `nginx/locations.conf` must be at least this. A file this size spends
/// more than the API's request timeout reaching a slow provider, which is why its route has a
/// deadline of its own (`crate::middleware::request_timeout`).
pub const MAX_ATTACHMENT_BYTES: u64 = 50_000_000;

/// The longest a name may be, in characters, as `page_attachments.file_name` allows.
const MAX_NAME_CHARACTERS: usize = 200;

/// How many of a file's first bytes prove what image it is: enough for the longest signature among
/// [`FILE_TYPES`], WebP's twelve.
pub const SIGNATURE_BYTES: usize = 16;

/// Where attachments sit in their storage location, as `database-backups/` holds the backups.
const STORAGE_DIRECTORY: &str = "page-attachments";

/// One kind of file a page takes.
#[derive(Debug, PartialEq, Eq)]
pub struct FileType {
    /// Lowercase, without its dot.
    extension: &'static str,
    /// What it is served as.
    content_type: &'static str,
    /// Set for an image a browser displays. Its bytes must be this format.
    image: Option<ImageFormat>,
}

impl FileType {
    const fn image(
        extension: &'static str,
        content_type: &'static str,
        format: ImageFormat,
    ) -> Self {
        Self {
            extension,
            content_type,
            image: Some(format),
        }
    }

    const fn file(extension: &'static str, content_type: &'static str) -> Self {
        Self {
            extension,
            content_type,
            image: None,
        }
    }

    pub const fn content_type(&self) -> &'static str {
        self.content_type
    }

    /// Refuses a file named as an image whose `head`, its first bytes, is not that image.
    ///
    /// # Errors
    /// `400` with the code `attachment_not_an_image`.
    pub fn require_signature(&self, head: &[u8]) -> Result<(), ApiError> {
        let Some(format) = self.image else {
            return Ok(());
        };
        if image::guess_format(head).is_ok_and(|found| found == format) {
            return Ok(());
        }
        Err(ApiError::CodedBadRequest {
            code: "attachment_not_an_image",
            message: format!(
                "this file is not the .{} image its name says",
                self.extension
            ),
        })
    }
}

/// Everything a page takes. A kind of file joins this list deliberately: anything else is refused,
/// which is why HTML, scripts, and programs are not here.
///
/// SVG is here as a file to download, since diagrams come in it, but it is a document that can
/// carry scripts, so it is served as plain bytes and never as an image.
const FILE_TYPES: &[FileType] = &[
    FileType::image("png", "image/png", ImageFormat::Png),
    FileType::image("jpg", "image/jpeg", ImageFormat::Jpeg),
    FileType::image("jpeg", "image/jpeg", ImageFormat::Jpeg),
    FileType::image("gif", "image/gif", ImageFormat::Gif),
    FileType::image("webp", "image/webp", ImageFormat::WebP),
    FileType::file("svg", "application/octet-stream"),
    FileType::file("pdf", "application/pdf"),
    FileType::file("txt", "text/plain; charset=utf-8"),
    FileType::file("log", "text/plain; charset=utf-8"),
    FileType::file("md", "text/markdown; charset=utf-8"),
    FileType::file("csv", "text/csv; charset=utf-8"),
    FileType::file("tsv", "text/tab-separated-values; charset=utf-8"),
    FileType::file("json", "application/json"),
    FileType::file("yaml", "application/yaml"),
    FileType::file("yml", "application/yaml"),
    FileType::file("doc", "application/msword"),
    FileType::file(
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    FileType::file("xls", "application/vnd.ms-excel"),
    FileType::file(
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    FileType::file("ppt", "application/vnd.ms-powerpoint"),
    FileType::file(
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    FileType::file("odt", "application/vnd.oasis.opendocument.text"),
    FileType::file("ods", "application/vnd.oasis.opendocument.spreadsheet"),
    FileType::file("odp", "application/vnd.oasis.opendocument.presentation"),
    FileType::file("zip", "application/zip"),
    FileType::file("gz", "application/gzip"),
    FileType::file("tar", "application/x-tar"),
    FileType::file("7z", "application/x-7z-compressed"),
];

/// Every extension a page takes, in the order [`FILE_TYPES`] lists them.
pub fn accepted_extensions() -> Vec<&'static str> {
    FILE_TYPES.iter().map(|kind| kind.extension).collect()
}

/// Whether a file served as `content_type` displays in a page. A type dropped from [`FILE_TYPES`]
/// later falls to a download, which is the safe way to be wrong.
pub fn displays_inline(content_type: &str) -> bool {
    FILE_TYPES
        .iter()
        .any(|kind| kind.image.is_some() && kind.content_type == content_type)
}

/// A file's name once it is safe to keep, with the kind of file it names.
///
/// What arrives is a browser's, or an agent's, to choose. Any path is dropped, control characters
/// and the invisible ones that reorder text go, so `exe.gnp` cannot read as something else, and
/// runs of spaces become one. It stays in the alphabet it was written in, since people read it.
#[derive(Debug)]
pub struct AttachmentName {
    name: String,
    file_type: &'static FileType,
}

impl AttachmentName {
    /// The name `raw` stands for.
    ///
    /// # Errors
    /// `400` with the code `attachment_name` when no name is left, and `attachment_type` when its
    /// extension is not one a page takes.
    pub fn parse(raw: &str) -> Result<Self, ApiError> {
        let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
        let kept: String = base
            .chars()
            .filter(|&character| is_kept(character))
            .collect();
        let name = kept.split_whitespace().collect::<Vec<_>>().join(" ");

        let (stem, extension) = name.rsplit_once('.').unwrap_or((&name, ""));
        let stem = stem.trim_matches([' ', '.']);
        if stem.is_empty() {
            return Err(ApiError::CodedBadRequest {
                code: "attachment_name",
                message: "a file needs a name and an extension to be attached".to_owned(),
            });
        }

        let extension = extension.to_lowercase();
        let Some(file_type) = FILE_TYPES.iter().find(|kind| kind.extension == extension) else {
            return Err(ApiError::CodedBadRequest {
                code: "attachment_type",
                message: format!("pages do not take .{extension} files"),
            });
        };

        // The extension stays, so a shortened name still says what it is.
        let room = MAX_NAME_CHARACTERS - extension.chars().count() - 1;
        let stem: String = stem.chars().take(room).collect();
        Ok(Self {
            name: format!("{}.{extension}", stem.trim_end()),
            file_type,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }

    pub const fn file_type(&self) -> &'static FileType {
        self.file_type
    }
}

/// Whether `character` may stay in a name. Rust has no table of invisible characters, so the ones
/// that matter are named: the marks and overrides that change the direction text reads in.
fn is_kept(character: char) -> bool {
    !character.is_control()
        && !matches!(character, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// `file_name` as one plain segment of a path: letters and digits of the English alphabet, with
/// hyphens for anything else and its extension in lowercase.
///
/// It stands in the address a page's Markdown carries, and in the provider's key, and as the name a
/// browser falls back to. Keeping it this plain means nothing needs encoding in any of them, so a
/// person or agent reading a page's source reads a name. The attachment's id is what finds it.
pub fn plain_name(file_name: &str) -> String {
    let (stem, extension) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    let mut plain = String::with_capacity(stem.len());
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() {
            plain.push(character);
        } else if !plain.is_empty() && !plain.ends_with('-') {
            plain.push('-');
        }
    }
    let plain = plain.trim_end_matches('-');
    let plain = if plain.is_empty() { "file" } else { plain };
    format!("{plain}.{}", extension.to_ascii_lowercase())
}

/// Where attachment `id` of page `page_id` is kept, relative to its location's directory. Named for
/// people too: anyone looking in the bucket can tell what a file is, without Elysium.
pub fn storage_path(page_id: Uuid, id: Uuid, file_name: &str) -> String {
    format!(
        "{STORAGE_DIRECTORY}/{page_id}/{id}/{}",
        plain_name(file_name)
    )
}

/// The address an attachment is served at, which a page's Markdown carries. Its last segment is
/// there for whoever reads the address; the route finds the file by its id.
pub fn address(page_id: Uuid, id: Uuid, file_name: &str) -> String {
    format!(
        "/api/v1/pages/{page_id}/attachments/{id}/{}",
        plain_name(file_name)
    )
}

/// The `Content-Disposition` a file is served with: `inline` for a page to display it, and
/// `attachment` for a browser to save it and never render it. The plain name is what an old browser
/// falls back to, and `filename*` keeps the real one in any alphabet.
pub fn content_disposition(file_name: &str, is_inline: bool) -> HeaderValue {
    let disposition = if is_inline { "inline" } else { "attachment" };
    let plain = plain_name(file_name);
    let encoded = utf8_percent_encode(file_name, NON_ALPHANUMERIC);
    let value = format!("{disposition}; filename=\"{plain}\"; filename*=UTF-8''{encoded}");
    HeaderValue::from_str(&value).unwrap_or(HeaderValue::from_static("attachment"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal_code(raw: &str) -> &'static str {
        match AttachmentName::parse(raw) {
            Err(ApiError::CodedBadRequest { code, .. }) => code,
            other => panic!("{raw} should have been refused, not {other:?}"),
        }
    }

    #[test]
    fn a_name_keeps_only_what_a_name_is() {
        let cases = [
            ("report.pdf", "report.pdf"),
            ("Q3 Report  (final).PDF", "Q3 Report (final).pdf"),
            ("../../etc/passwd.txt", "passwd.txt"),
            ("C:\\Users\\Alex\\plan.docx", "plan.docx"),
            ("line\r\nbreak.txt", "linebreak.txt"),
            ("quiet\u{202e}gnp.exe.txt", "quietgnp.exe.txt"),
            ("  spaced  .png", "spaced.png"),
            ("設計図.png", "設計図.png"),
        ];
        for (raw, expected) in cases {
            let name = AttachmentName::parse(raw).expect("a name");
            assert_eq!(name.as_str(), expected, "{raw}");
        }
    }

    #[test]
    fn a_long_name_keeps_its_extension() {
        let name = AttachmentName::parse(&format!("{}.pdf", "a".repeat(500))).expect("a name");

        assert_eq!(name.as_str().chars().count(), MAX_NAME_CHARACTERS);
        assert_eq!(
            name.as_str()
                .rsplit_once('.')
                .map(|(_stem, extension)| extension),
            Some("pdf")
        );
    }

    #[test]
    fn nothing_left_of_a_name_is_refused() {
        for raw in ["", "   ", ".png", "...", "dir/", "\u{202e}"] {
            assert_eq!(refusal_code(raw), "attachment_name", "{raw:?}");
        }
    }

    #[test]
    fn what_could_run_is_refused() {
        for raw in [
            "page.html",
            "page.HTM",
            "page.xhtml",
            "script.js",
            "script.mjs",
            "tool.exe",
            "tool.sh",
            "photo.png.html",
            "noextension",
        ] {
            assert_eq!(refusal_code(raw), "attachment_type", "{raw:?}");
        }
    }

    #[test]
    fn an_image_must_be_the_image_it_says() {
        let png = AttachmentName::parse("photo.png").expect("a name");

        png.file_type()
            .require_signature(b"\x89PNG\r\n\x1a\n\0\0\0\0")
            .expect("a PNG");
        for head in [&b"<html><script>"[..], b"GIF89a", b""] {
            let refused = png.file_type().require_signature(head);
            assert!(
                matches!(
                    refused,
                    Err(ApiError::CodedBadRequest {
                        code: "attachment_not_an_image",
                        ..
                    })
                ),
                "{refused:?}"
            );
        }

        let webp = AttachmentName::parse("photo.webp").expect("a name");
        webp.file_type()
            .require_signature(b"RIFF\0\0\0\0WEBPVP8 ")
            .expect("a WebP");
    }

    #[test]
    fn only_images_display() {
        for name in ["a.png", "a.jpg", "a.jpeg", "a.gif", "a.webp"] {
            let parsed = AttachmentName::parse(name).expect("a name");
            assert!(displays_inline(parsed.file_type().content_type()), "{name}");
        }
        for name in ["a.svg", "a.pdf", "a.txt", "a.md", "a.json", "a.zip"] {
            let parsed = AttachmentName::parse(name).expect("a name");
            assert!(
                !displays_inline(parsed.file_type().content_type()),
                "{name}"
            );
        }
        assert!(!displays_inline("text/html"));
    }

    #[test]
    fn svg_is_served_as_bytes() {
        let name = AttachmentName::parse("diagram.svg").expect("a name");

        assert_eq!(name.file_type().content_type(), "application/octet-stream");
    }

    #[test]
    fn every_extension_is_written_as_names_are_compared() {
        for extension in accepted_extensions() {
            assert_eq!(extension, extension.to_lowercase());
            assert!(
                extension
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
            );
        }
    }

    #[test]
    fn a_plain_name_needs_no_encoding_anywhere() {
        let cases = [
            ("Q3 Report (final).pdf", "Q3-Report-final.pdf"),
            ("設計図.png", "file.png"),
            ("設計図 v2.PNG", "v2.png"),
            ("a--b__c.txt", "a-b-c.txt"),
        ];
        for (name, expected) in cases {
            assert_eq!(plain_name(name), expected, "{name}");
        }
    }

    /// nginx refuses a body past its own limit with a page of HTML, before the API can say why.
    #[test]
    fn nginx_lets_the_largest_file_through() {
        let locations = include_str!("../../nginx/locations.conf");
        let (_before, block) = locations
            .split_once("/attachments$")
            .expect("the route has a location of its own");
        let megabytes: u64 = block
            .split_once("client_max_body_size ")
            .and_then(|(_before, rest)| rest.split_once("m;"))
            .and_then(|(limit, _after)| limit.parse().ok())
            .expect("the location sets a limit in megabytes");

        assert!(megabytes * 1024 * 1024 >= MAX_ATTACHMENT_BYTES);
    }

    #[test]
    fn a_disposition_says_which_and_keeps_the_real_name() {
        assert_eq!(
            content_disposition("Q3 Report.pdf", false)
                .to_str()
                .expect("ascii"),
            "attachment; filename=\"Q3-Report.pdf\"; filename*=UTF-8''Q3%20Report%2Epdf"
        );
        assert_eq!(
            content_disposition("図.png", true).to_str().expect("ascii"),
            "inline; filename=\"file.png\"; filename*=UTF-8''%E5%9B%B3%2Epng"
        );
    }

    #[test]
    fn a_name_cannot_break_out_of_its_header() {
        let name =
            AttachmentName::parse("a\"; filename=\"evil.html\r\nX-Hurt: 1.txt").expect("a name");
        let header = content_disposition(name.as_str(), false);

        assert!(
            header
                .to_str()
                .expect("ascii")
                .starts_with("attachment; filename=\"a-filename-evil-htmlX-Hurt-1.txt\"")
        );
    }
}
