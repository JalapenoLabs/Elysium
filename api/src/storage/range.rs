// Copyright © 2026 Jalapeno Labs

//! Byte ranges of a stored file, so a player seeks inside a long recording by asking for the
//! part it needs rather than downloading it whole (RFC 9110, Range Requests).
//!
//! [`ByteRange`] is what a browser's `Range` header asks for, and what [`super::Storage`]
//! forwards to the provider. [`ContentRange`] is what a provider's `Content-Range` says it sent.
//! Only one range per request is understood: a browser's player asks for one at a time, and
//! a server may answer a request for several with the whole file.

use std::fmt;

/// One range of a file's bytes, as a `Range: bytes=` header names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteRange {
    /// From byte `start` to byte `end`, both included, or to the file's end when `end` is
    /// `None`, which is how a player asks to play on from a point.
    From { start: u64, end: Option<u64> },
    /// The file's last `length` bytes, where some containers keep their index.
    Last { length: u64 },
}

/// The bytes of a file one range covers, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

impl Span {
    /// How many bytes it covers.
    pub const fn length(self) -> u64 {
        self.end - self.start + 1
    }
}

impl ByteRange {
    /// Reads a `Range` header naming one range of bytes.
    ///
    /// Anything else (several ranges, a unit other than bytes, a range that ends before it
    /// starts, or text that is not a range) reads as `None`, and the file is answered whole,
    /// as RFC 9110 lets a server do with a `Range` it does not take.
    pub fn parse(header: &str) -> Option<Self> {
        let spec = header.trim().strip_prefix("bytes=")?.trim();
        let (start, end) = spec.split_once('-')?;
        let (start, end) = (start.trim(), end.trim());
        if start.is_empty() {
            let length = end.parse().ok()?;
            return Some(Self::Last { length });
        }
        let start = start.parse().ok()?;
        if end.is_empty() {
            return Some(Self::From { start, end: None });
        }
        let end: u64 = end.parse().ok()?;
        if end < start {
            return None;
        }
        Some(Self::From {
            start,
            end: Some(end),
        })
    }

    /// The bytes it covers of a file `total` bytes long, or `None` when it covers none of them,
    /// which is answered `416 Range Not Satisfiable`. An end past the file's is the file's end.
    pub fn within(self, total: u64) -> Option<Span> {
        let last = total.checked_sub(1)?;
        match self {
            Self::From { start, end } => (start <= last).then(|| Span {
                start,
                end: end.map_or(last, |end| end.min(last)),
            }),
            Self::Last { length } => (length > 0).then(|| Span {
                start: total.saturating_sub(length),
                end: last,
            }),
        }
    }
}

/// The `Range` header that asks for it.
impl fmt::Display for ByteRange {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::From { start, end: None } => write!(formatter, "bytes={start}-"),
            Self::From {
                start,
                end: Some(end),
            } => write!(formatter, "bytes={start}-{end}"),
            Self::Last { length } => write!(formatter, "bytes=-{length}"),
        }
    }
}

/// What a provider's `Content-Range` says of a part it sent: which bytes, and how long the whole
/// file is, when it says (`*` when it does not).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub span: Span,
    pub total: Option<u64>,
}

impl ContentRange {
    /// Reads `bytes <start>-<end>/<total>`, the header that comes with a `206`.
    pub fn parse(header: &str) -> Option<Self> {
        let (span, total) = header.trim().strip_prefix("bytes ")?.split_once('/')?;
        let (start, end) = span.split_once('-')?;
        let span = Span {
            start: start.trim().parse().ok()?,
            end: end.trim().parse().ok()?,
        };
        if span.end < span.start {
            return None;
        }
        // `*` says the provider does not know the whole file's length.
        let total = match total.trim() {
            "*" => None,
            total => Some(total.parse::<u64>().ok()?),
        };
        if total.is_some_and(|total| span.end >= total) {
            return None;
        }
        Some(Self { span, total })
    }

    /// Reads `bytes */<total>`, the file's length a `416` carries, `None` when it gives none.
    pub fn unsatisfied_total(header: &str) -> Option<u64> {
        header.trim().strip_prefix("bytes */")?.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_player_asks_for_one_range_in_any_of_its_three_forms() {
        assert_eq!(
            ByteRange::parse("bytes=0-"),
            Some(ByteRange::From {
                start: 0,
                end: None
            })
        );
        assert_eq!(
            ByteRange::parse("bytes=100-199"),
            Some(ByteRange::From {
                start: 100,
                end: Some(199)
            })
        );
        assert_eq!(
            ByteRange::parse(" bytes=-500 "),
            Some(ByteRange::Last { length: 500 })
        );
    }

    #[test]
    fn anything_but_one_range_of_bytes_is_answered_whole() {
        for header in [
            "bytes=0-99,200-299",
            "items=0-9",
            "bytes=200-100",
            "bytes=-",
            "bytes=abc-",
            "bytes 0-99",
            "",
        ] {
            assert_eq!(ByteRange::parse(header), None, "{header:?}");
        }
    }

    #[test]
    fn a_range_is_its_header_again() {
        for header in ["bytes=0-", "bytes=100-199", "bytes=-500"] {
            let range = ByteRange::parse(header).expect("a range");
            assert_eq!(range.to_string(), header);
        }
    }

    #[test]
    fn a_range_covers_only_bytes_the_file_has() {
        let open = ByteRange::From {
            start: 10,
            end: None,
        };
        assert_eq!(open.within(100), Some(Span { start: 10, end: 99 }));
        let past_the_end = ByteRange::From {
            start: 90,
            end: Some(1_000),
        };
        assert_eq!(past_the_end.within(100), Some(Span { start: 90, end: 99 }));
        let suffix = ByteRange::Last { length: 30 };
        assert_eq!(suffix.within(100), Some(Span { start: 70, end: 99 }));
        let longer_than_the_file = ByteRange::Last { length: 300 };
        assert_eq!(
            longer_than_the_file.within(100),
            Some(Span { start: 0, end: 99 })
        );
        assert_eq!(Span { start: 70, end: 99 }.length(), 30);
    }

    #[test]
    fn a_range_past_the_file_or_of_nothing_covers_none_of_it() {
        let after = ByteRange::From {
            start: 100,
            end: None,
        };
        assert_eq!(after.within(100), None);
        assert_eq!(ByteRange::Last { length: 0 }.within(100), None);
        let any = ByteRange::From {
            start: 0,
            end: None,
        };
        assert_eq!(any.within(0), None, "an empty file has no byte to send");
    }

    #[test]
    fn a_providers_content_range_names_the_part_and_the_whole() {
        assert_eq!(
            ContentRange::parse("bytes 100-199/1000"),
            Some(ContentRange {
                span: Span {
                    start: 100,
                    end: 199
                },
                total: Some(1_000),
            })
        );
        assert_eq!(
            ContentRange::parse("bytes 0-9/*"),
            Some(ContentRange {
                span: Span { start: 0, end: 9 },
                total: None,
            })
        );
        for header in [
            "bytes 9-0/10",
            "bytes 0-10/10",
            "bytes */10",
            "0-9/10",
            "bytes 0-9",
        ] {
            assert_eq!(ContentRange::parse(header), None, "{header:?}");
        }
        assert_eq!(ContentRange::unsatisfied_total("bytes */1000"), Some(1_000));
        assert_eq!(ContentRange::unsatisfied_total("bytes */*"), None);
    }
}
