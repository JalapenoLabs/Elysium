// Copyright © 2026 Jalapeno Labs

//! What a person may upload as a meeting, and how its files arrive. The rows are
//! [`crate::models::meeting_upload`], the routes `/api/v1/meeting-uploads`, and the step that
//! stores an upload is the meeting workflow's fetch (`crate::meetings::fetch`). See
//! `docs/meetings.md`, Uploads.
//!
//! Everything here is a policy that must not vary per deployment, so it is constants in code, and
//! the web app reads them from `GET /api/v1/meeting-uploads/policy` rather than keeping a copy.
//!
//! # Parts
//!
//! A recording can be gigabytes, which no single request carries reliably, so every file arrives
//! as numbered parts of [`PART_BYTES`] (the last one shorter), each its own request, in order. A
//! part already received is acknowledged again without being written, so a person whose
//! connection dropped resumes from the first part the API does not have, from the same page or
//! after a reload. Parts wait in the upload's staging directory ([`staging`]) until the meeting's
//! fetch streams them, joined, to the storage location the upload chose.
//!
//! # What is believed
//!
//! Nothing the browser says about a file. What it is comes from its extension, through
//! [`FILE_TYPES`], a list of what a meeting takes; a recording's first bytes must also be the
//! format its extension names, checked as its first part arrives, since the recording is later
//! played on Elysium's own origin. A transcript is read as text when the upload finishes
//! ([`transcript`]), and one that holds nothing said is refused then.

pub mod staging;
pub mod transcript;

use crate::errors::ApiError;
use crate::file_names;
use crate::storage::MAX_UPLOAD_BYTES;

/// What a capture made from an upload names as its source (`meeting_captures.source_kind`).
pub const SOURCE_KIND: &str = "upload";

/// How long an unfinished upload waits for its next part before the engine removes it and what of
/// it arrived: a week, long enough to come back to after a weekend, short enough that abandoned
/// gigabytes do not sit in the workspace for good.
pub const STALE_AFTER: chrono::TimeDelta = chrono::TimeDelta::days(7);

/// How many bytes each part of a file carries, but the last: 8 MiB.
///
/// Small enough that a part reaches the API well inside its deadline on a slow uplink (nginx
/// holds a part until it has arrived whole), and large enough that the largest recording is 640
/// parts. `client_max_body_size` on the parts' location in `nginx/locations.conf` must be at least
/// this. Each upload keeps the size it started with (`meeting_uploads.part_bytes`), so changing
/// this never breaks one under way.
pub const PART_BYTES: u32 = 8 * 1024 * 1024;

/// The largest recording: the most storage takes in one upload (5 GiB), since a recording is kept
/// as one file.
pub const MAX_RECORDING_BYTES: u64 = MAX_UPLOAD_BYTES;

/// The largest transcript: 10 MB. A day of speech is about a megabyte of WebVTT, and a transcript
/// is read whole when its upload finishes, so this bounds what that holds in memory.
pub const MAX_TRANSCRIPT_BYTES: u64 = 10_000_000;

/// The longest a file's name may be, in characters, as `meeting_upload_files.file_name` allows.
const MAX_NAME_CHARACTERS: usize = 200;

/// How many of a recording's first bytes prove its format: enough for the longest signature among
/// [`FILE_TYPES`], WAV's twelve.
pub const SIGNATURE_BYTES: usize = 12;

/// What one file of an upload is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Recording,
    Transcript,
}

impl FileKind {
    /// As `meeting_upload_files.kind` and the parts' route name it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Transcript => "transcript",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Recording, Self::Transcript]
            .into_iter()
            .find(|kind| kind.as_str() == text)
    }

    /// The largest file of this kind.
    pub const fn max_bytes(self) -> u64 {
        match self {
            Self::Recording => MAX_RECORDING_BYTES,
            Self::Transcript => MAX_TRANSCRIPT_BYTES,
        }
    }
}

/// How a transcript's text is laid out, which decides how it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptFormat {
    WebVtt,
    Srt,
    /// Plain text, one line said per line, with `Speaker:` and `[00:01:23]` read when there.
    Text,
    /// Text as Markdown writes it: the same, once headings, lists, quotes, and emphasis are read
    /// past.
    Markdown,
}

/// One kind of file a meeting takes.
#[derive(Debug, PartialEq, Eq)]
pub struct FileType {
    /// Lowercase, without its dot.
    extension: &'static str,
    /// What it is stored and served as.
    content_type: &'static str,
    kind: FileKind,
    /// A recording's first bytes, which must hold its format's signature; `None` for a transcript.
    signature: Option<Signature>,
    /// How a transcript is read; `None` for a recording.
    format: Option<TranscriptFormat>,
}

/// The first bytes a recording's format begins with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signature {
    /// An ID3 tag, or an MPEG audio frame's sync bits.
    Mp3,
    /// `RIFF`, its length, then `WAVE`.
    Wav,
    /// An ISO base media file: its first box is `ftyp`.
    Mp4,
    /// QuickTime: `ftyp` as MP4 has, or, in files older than it, one of the atoms they open with.
    QuickTime,
}

impl Signature {
    fn holds(self, head: &[u8]) -> bool {
        let first_box = head.get(4..8);
        match self {
            Self::Mp3 => {
                head.starts_with(b"ID3")
                    || matches!(head, [0xFF, second, ..] if second & 0xE0 == 0xE0)
            }
            Self::Wav => head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE"),
            Self::Mp4 => first_box == Some(b"ftyp"),
            Self::QuickTime => first_box.is_some_and(|name| {
                [
                    b"ftyp", b"moov", b"mdat", b"wide", b"free", b"skip", b"pnot",
                ]
                .iter()
                .any(|atom| name == atom.as_slice())
            }),
        }
    }
}

impl FileType {
    const fn recording(
        extension: &'static str,
        content_type: &'static str,
        signature: Signature,
    ) -> Self {
        Self {
            extension,
            content_type,
            kind: FileKind::Recording,
            signature: Some(signature),
            format: None,
        }
    }

    const fn transcript(
        extension: &'static str,
        content_type: &'static str,
        format: TranscriptFormat,
    ) -> Self {
        Self {
            extension,
            content_type,
            kind: FileKind::Transcript,
            signature: None,
            format: Some(format),
        }
    }

    pub const fn extension(&self) -> &'static str {
        self.extension
    }

    pub const fn content_type(&self) -> &'static str {
        self.content_type
    }

    /// How this transcript is read.
    ///
    /// # Panics
    /// Panics for a recording's type, which only a programming error asks of.
    pub fn format(&self) -> TranscriptFormat {
        self.format
            .expect("only a transcript's type has a format, and only a transcript is read")
    }

    /// Refuses a recording whose `head`, its first bytes, is not the format its name says.
    ///
    /// # Errors
    /// `400` with the code `meeting_upload_not_media`.
    pub fn require_signature(&self, head: &[u8]) -> Result<(), ApiError> {
        let Some(signature) = self.signature else {
            return Ok(());
        };
        if signature.holds(head) {
            return Ok(());
        }
        Err(ApiError::CodedBadRequest {
            code: "meeting_upload_not_media",
            message: format!(
                "this file is not the .{} recording its name says",
                self.extension
            ),
        })
    }
}

/// Everything a meeting takes. A kind of file joins this list deliberately; anything else is
/// refused.
const FILE_TYPES: &[FileType] = &[
    FileType::recording("mp3", "audio/mpeg", Signature::Mp3),
    FileType::recording("wav", "audio/wav", Signature::Wav),
    FileType::recording("mp4", "video/mp4", Signature::Mp4),
    FileType::recording("mov", "video/quicktime", Signature::QuickTime),
    FileType::transcript("vtt", "text/vtt; charset=utf-8", TranscriptFormat::WebVtt),
    FileType::transcript("srt", "application/x-subrip", TranscriptFormat::Srt),
    FileType::transcript("txt", "text/plain; charset=utf-8", TranscriptFormat::Text),
    FileType::transcript(
        "md",
        "text/markdown; charset=utf-8",
        TranscriptFormat::Markdown,
    ),
];

/// Every extension a meeting takes as `kind`, in the order [`FILE_TYPES`] lists them.
pub fn accepted_extensions(kind: FileKind) -> Vec<&'static str> {
    FILE_TYPES
        .iter()
        .filter(|file_type| file_type.kind == kind)
        .map(|file_type| file_type.extension)
        .collect()
}

/// The type a stored file of `kind` named `file_name` is, by its extension.
pub fn type_of(kind: FileKind, file_name: &str) -> Option<&'static FileType> {
    let extension = file_name
        .rsplit_once('.')
        .map_or("", |(_stem, extension)| extension);
    FILE_TYPES
        .iter()
        .find(|file_type| file_type.kind == kind && file_type.extension == extension)
}

/// The name `raw` stands for once it is safe to keep (`crate::file_names`), as a file of `kind`,
/// whose type [`type_of`] then reads from it.
///
/// # Errors
/// `400` with the code `meeting_upload_name` when no name is left, and `meeting_upload_type` when
/// its extension is not one a meeting takes as `kind`.
pub fn upload_name(raw: &str, kind: FileKind) -> Result<String, ApiError> {
    let Some(cleaned) = file_names::clean(raw) else {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_name",
            message: "a file needs a name and an extension to be uploaded".to_owned(),
        });
    };
    let extension = cleaned.extension();
    let is_taken = FILE_TYPES
        .iter()
        .any(|file_type| file_type.kind == kind && file_type.extension == extension);
    if !is_taken {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_upload_type",
            message: format!(
                "a meeting's {} cannot be a .{extension} file",
                kind.as_str()
            ),
        });
    }
    Ok(cleaned.into_name(MAX_NAME_CHARACTERS))
}

/// How many bytes part `index` of a file of `size_bytes` carries, in parts of `part_bytes`;
/// `None` past its last part.
pub fn part_length(size_bytes: u64, part_bytes: u32, index: u64) -> Option<u64> {
    let start = index.checked_mul(u64::from(part_bytes))?;
    if start >= size_bytes {
        return None;
    }
    Some((size_bytes - start).min(u64::from(part_bytes)))
}

/// How many parts a file of `size_bytes` arrives in.
pub fn part_count(size_bytes: u64, part_bytes: u32) -> u64 {
    size_bytes.div_ceil(u64::from(part_bytes))
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
    fn a_meeting_takes_the_four_recordings_and_four_transcripts_the_issue_names() {
        assert_eq!(
            accepted_extensions(FileKind::Recording),
            ["mp3", "wav", "mp4", "mov"]
        );
        assert_eq!(
            accepted_extensions(FileKind::Transcript),
            ["vtt", "srt", "txt", "md"]
        );
    }

    #[test]
    fn a_file_is_what_its_extension_says_and_only_as_its_own_kind() {
        let recording = upload_name("Weekly sync.MOV", FileKind::Recording).expect("a name");
        assert_eq!(recording, "Weekly sync.mov");
        let recording_type = type_of(FileKind::Recording, &recording).expect("a type");
        assert_eq!(recording_type.content_type(), "video/quicktime");

        let transcript = upload_name("notes.md", FileKind::Transcript).expect("a name");
        let transcript_type = type_of(FileKind::Transcript, &transcript).expect("a type");
        assert_eq!(transcript_type.format(), TranscriptFormat::Markdown);
        assert_eq!(type_of(FileKind::Recording, &transcript), None);

        for (raw, kind) in [
            ("notes.vtt", FileKind::Recording),
            ("call.mp4", FileKind::Transcript),
            ("call.webm", FileKind::Recording),
            ("page.html", FileKind::Transcript),
            ("noextension", FileKind::Recording),
        ] {
            assert_eq!(
                refusal_code(upload_name(raw, kind)),
                "meeting_upload_type",
                "{raw}"
            );
        }
        assert_eq!(
            refusal_code(upload_name(".mp3", FileKind::Recording)),
            "meeting_upload_name"
        );
    }

    #[test]
    fn a_recording_must_begin_as_its_format_does() {
        let cases: [(&str, &[u8]); 6] = [
            ("a.mp3", b"ID3\x04\0\0\0\0\0\0\0\0"),
            ("a.mp3", b"\xFF\xFB\x90\x64\0\0\0\0\0\0\0\0"),
            ("a.wav", b"RIFF\x24\x08\0\0WAVEfmt "),
            ("a.mp4", b"\0\0\0\x20ftypisom"),
            ("a.mov", b"\0\0\0\x14ftypqt  "),
            ("a.mov", b"\0\0\0\x08wide\0\0\0\0"),
        ];
        for (name, head) in cases {
            let file_type = type_of(FileKind::Recording, name).expect("a type");
            file_type.require_signature(head).expect(name);
        }

        let mp4 = type_of(FileKind::Recording, "a.mp4").expect("a type");
        for head in [&b"<html><script>alert"[..], b"RIFF\0\0\0\0WAVE", b""] {
            assert_eq!(
                refusal_code(mp4.require_signature(head)),
                "meeting_upload_not_media"
            );
        }
    }

    #[test]
    fn a_file_arrives_in_whole_parts_and_a_shorter_last_one() {
        assert_eq!(part_count(20, 8), 3);
        assert_eq!(part_count(16, 8), 2);
        assert_eq!(part_length(20, 8, 0), Some(8));
        assert_eq!(part_length(20, 8, 2), Some(4));
        assert_eq!(part_length(20, 8, 3), None);
        assert_eq!(part_length(16, 8, 2), None);
        assert_eq!(part_length(u64::MAX, 8, u64::MAX), None);
        // The largest recording is a bounded number of requests.
        assert_eq!(part_count(MAX_RECORDING_BYTES, PART_BYTES), 640);
    }

    /// nginx refuses a body past its own limit with a page of HTML, before the API can say why.
    #[test]
    fn nginx_lets_a_whole_part_through() {
        let locations = include_str!("../../../nginx/locations.conf");
        let (_before, block) = locations
            .split_once("/parts/[0-9]+$")
            .expect("the parts have a location of their own");
        let megabytes: u64 = block
            .split_once("client_max_body_size ")
            .and_then(|(_before, rest)| rest.split_once("m;"))
            .and_then(|(limit, _after)| limit.parse().ok())
            .expect("the location sets a limit in megabytes");

        assert!(megabytes * 1024 * 1024 >= u64::from(PART_BYTES));
    }
}
