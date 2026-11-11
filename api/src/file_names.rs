// Copyright © 2026 Jalapeno Labs

//! A file's name as a browser or an agent gives it, made safe to keep and to show.
//!
//! Whoever sends a file chooses its name, so nothing in it is trusted. [`clean`] drops any path,
//! control characters, and the invisible characters that reorder text (so `exe.gnp` cannot read
//! as something else), and folds runs of whitespace into one space. The name stays in the
//! alphabet it was written in, since people read it. Page attachments and meeting uploads both
//! name their files this way, each deciding for itself which extensions it takes.

/// A name once it is safe to keep: its stem and its lowercase extension, which may be empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanName {
    stem: String,
    extension: String,
}

impl CleanName {
    /// The extension in lowercase, without its dot; empty when the name has none.
    pub fn extension(&self) -> &str {
        &self.extension
    }

    /// The whole name, at most `max_characters` long, shortening the stem so the extension stays
    /// and a shortened name still says what it is.
    pub fn into_name(self, max_characters: usize) -> String {
        let room = max_characters.saturating_sub(self.extension.chars().count() + 1);
        let stem: String = self.stem.chars().take(room).collect();
        format!("{}.{}", stem.trim_end(), self.extension)
    }
}

/// The name `raw` stands for, or `None` when nothing of a name is left once it is cleaned.
pub fn clean(raw: &str) -> Option<CleanName> {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let kept: String = base
        .chars()
        .filter(|&character| is_kept(character))
        .collect();
    let name = kept.split_whitespace().collect::<Vec<_>>().join(" ");

    let (stem, extension) = name.rsplit_once('.').unwrap_or((&name, ""));
    let stem = stem.trim_matches([' ', '.']);
    if stem.is_empty() {
        return None;
    }
    Some(CleanName {
        stem: stem.to_owned(),
        extension: extension.to_lowercase(),
    })
}

/// Whether `character` may stay in a name. Rust has no table of invisible characters, so the ones
/// that matter are named: the marks and overrides that change the direction text reads in.
fn is_kept(character: char) -> bool {
    !character.is_control()
        && !matches!(character, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_keeps_only_what_a_name_is() {
        let cases = [
            ("report.pdf", "report", "pdf"),
            ("Q3 Report  (final).PDF", "Q3 Report (final)", "pdf"),
            ("../../etc/passwd.txt", "passwd", "txt"),
            ("C:\\Users\\Alex\\standup.MP4", "standup", "mp4"),
            ("quiet\u{202e}4pm.exe.vtt", "quiet4pm.exe", "vtt"),
            ("noextension", "noextension", ""),
        ];
        for (raw, stem, extension) in cases {
            let cleaned = clean(raw).expect("a name");
            assert_eq!(cleaned.extension(), extension, "{raw}");
            assert_eq!(
                cleaned.into_name(200),
                format!("{stem}.{extension}"),
                "{raw}"
            );
        }
    }

    #[test]
    fn nothing_left_of_a_name_is_none() {
        for raw in ["", "   ", ".png", "...", "dir/", "\u{202e}"] {
            assert_eq!(clean(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn a_shortened_name_keeps_its_extension() {
        let name = clean(&format!("{}.mov", "a".repeat(500)))
            .expect("a name")
            .into_name(200);

        assert_eq!(name.chars().count(), 200);
        assert_eq!(
            name.rsplit_once('.').map(|(_stem, extension)| extension),
            Some("mov")
        );
    }
}
