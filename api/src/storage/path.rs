// Copyright © 2026 Jalapeno Labs

//! Paths inside a storage location, checked before they reach a provider.
//!
//! Every file operation names its target relative to the location's directory
//! (`path_prefix`). A [`RelativePath`] is plain names separated by single slashes, so no
//! path can climb out of that directory or smuggle in a separator a provider reads
//! differently. The empty path is the location's directory itself.

use super::StorageError;

/// The longest object key, prefix included, in bytes. S3 and Google Cloud cap keys at
/// 1,024 bytes; Bunny allows 6,000 characters, but one bound for every provider keeps a
/// path that works on one location valid on any other.
const MAX_KEY_BYTES: usize = 1024;

/// A validated path relative to a location's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativePath(String);

impl RelativePath {
    /// Checks `value` as a path inside a location; the empty string is the location itself.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] for a leading or doubled slash, a trailing slash,
    /// a `.` or `..` segment, a backslash or control character, or more than 1,024 bytes.
    pub fn parse(value: &str) -> Result<Self, StorageError> {
        if value.is_empty() {
            return Ok(Self(String::new()));
        }
        if value.len() > MAX_KEY_BYTES {
            return Err(StorageError::Invalid(format!(
                "a path must be {MAX_KEY_BYTES} bytes or fewer"
            )));
        }

        let is_plain = value.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment
                    .chars()
                    .any(|character| character.is_control() || character == '\\')
        });
        if !is_plain {
            return Err(StorageError::Invalid(format!(
                "\"{}\" is not a plain path: use names separated by single slashes, without a \
                 leading or trailing slash, . or .., backslashes, or control characters",
                value.escape_debug()
            )));
        }
        Ok(Self(value.to_owned()))
    }

    /// Checks `value` as a path naming a file or directory, which the location itself is not.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] for the empty path, and for anything
    /// [`RelativePath::parse`] refuses.
    pub fn parse_named(value: &str) -> Result<Self, StorageError> {
        let path = Self::parse(value)?;
        if path.is_root() {
            return Err(StorageError::Invalid(
                "a path must name a file inside the location".to_owned(),
            ));
        }
        Ok(path)
    }

    pub const fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Whether this is the location's own directory.
    pub const fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The relative path of `name` directly inside this directory.
    pub fn child(&self, name: &str) -> String {
        if self.is_root() {
            return name.to_owned();
        }
        format!("{}/{name}", self.0)
    }

    /// The full key inside the zone or bucket: `prefix`, which is stored without
    /// surrounding slashes, joined to this path.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] when the joined key is longer than 1,024 bytes.
    pub fn within(&self, prefix: &str) -> Result<String, StorageError> {
        let key = match (prefix.is_empty(), self.is_root()) {
            (true, _) => self.0.clone(),
            (false, true) => prefix.to_owned(),
            (false, false) => format!("{prefix}/{}", self.0),
        };
        if key.len() > MAX_KEY_BYTES {
            return Err(StorageError::Invalid(format!(
                "the path and the location's directory together must be {MAX_KEY_BYTES} bytes or fewer"
            )));
        }
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_are_accepted() {
        for accepted in [
            "",
            "report.pdf",
            "builds/2026/app.tar.gz",
            "team docs/Q3 plan.md",
        ] {
            let path = RelativePath::parse(accepted).expect(accepted);
            assert_eq!(path.as_str(), accepted);
        }
        assert_eq!(
            RelativePath::parse("a/..b/c.")
                .expect("dots inside names")
                .as_str(),
            "a/..b/c."
        );
    }

    #[test]
    fn traversal_separators_and_control_characters_are_refused() {
        for refused in [
            "/etc/passwd",
            "builds/",
            "builds//app",
            "../secrets",
            "builds/./app",
            "builds/..",
            ".",
            "a\\b",
            "tab\there",
            "new\nline",
            "nul\0",
        ] {
            let error = RelativePath::parse(refused).expect_err(refused);
            assert!(matches!(error, StorageError::Invalid(_)), "{refused}");
        }
    }

    #[test]
    fn length_is_bounded_in_bytes() {
        RelativePath::parse(&"a".repeat(1024)).expect("1,024 bytes fit");
        RelativePath::parse(&"a".repeat(1025)).expect_err("1,025 bytes do not");
        // 342 three-byte characters are 1,026 bytes, though only 342 characters.
        RelativePath::parse(&"€".repeat(342)).expect_err("the bound counts bytes");
    }

    #[test]
    fn named_paths_refuse_the_location_itself() {
        RelativePath::parse_named("").expect_err("the root names no file");
        RelativePath::parse_named("a.txt").expect("a file");
    }

    #[test]
    fn keys_join_the_prefix_with_one_slash() {
        let file = RelativePath::parse("builds/app.tar.gz").expect("path");
        let root = RelativePath::parse("").expect("root");

        assert_eq!(file.within("").expect("key"), "builds/app.tar.gz");
        assert_eq!(
            file.within("elysium/artifacts").expect("key"),
            "elysium/artifacts/builds/app.tar.gz"
        );
        assert_eq!(root.within("").expect("key"), "");
        assert_eq!(root.within("elysium").expect("key"), "elysium");
    }

    #[test]
    fn joined_keys_are_bounded_too() {
        let path = RelativePath::parse(&"a".repeat(600)).expect("path");
        path.within(&"p".repeat(423))
            .expect("600 + 1 + 423 = 1,024 bytes");
        path.within(&"p".repeat(424)).expect_err("1,025 bytes");
    }

    #[test]
    fn children_are_named_relative_to_the_location() {
        let root = RelativePath::parse("").expect("root");
        let directory = RelativePath::parse("builds").expect("path");
        assert_eq!(root.child("app"), "app");
        assert_eq!(directory.child("app"), "builds/app");
    }
}
