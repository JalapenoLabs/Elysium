// Copyright © 2026 Jalapeno Labs

//! The fingerprint that tells one problem from another, whoever hits it.
//!
//! A fingerprint hashes where the problem is (its category, such as
//! `connector:jira:check:sign_in`) with its redacted error, normalized so that what varies from
//! one occurrence to the next does not count: case, runs of whitespace, and every number (a
//! status code stays, as `#`, but not which second a timeout took). The version and the browser
//! are left out, so the same problem keeps one issue across releases until it is closed.
//!
//! Matching is exact: two reports join one issue only when their fingerprints are equal, never
//! because their errors look alike.

use sha2::{Digest, Sha256};

/// How many characters of the normalized error count. An error that differs only past this is
/// the same problem, and a very long error cannot make hashing expensive.
const NORMALIZED_MAX_CHARACTERS: usize = 1000;

/// How many hex characters of the hash a fingerprint keeps: 64 bits, plenty to keep problems
/// apart, short enough to read and to search GitHub for. `issue_reports` checks the shape.
const FINGERPRINT_HEX_CHARACTERS: usize = 16;

/// Changes when the normalization does, so a fingerprint computed one way never matches one
/// computed another.
const FINGERPRINT_VERSION: &str = "elysium-issue-report-v1";

/// The fingerprint of the problem in `category` whose error, already redacted, is
/// `redacted_error`: 16 lower-case hex characters.
pub fn fingerprint(category: &str, redacted_error: &str) -> String {
    let normalized = normalize(redacted_error);
    let digest = Sha256::digest(format!("{FINGERPRINT_VERSION}\n{category}\n{normalized}"));
    let mut hex = hex::encode(digest);
    hex.truncate(FINGERPRINT_HEX_CHARACTERS);
    hex
}

/// `error` lower-cased, every run of digits as `#`, and every run of whitespace as one space.
fn normalize(error: &str) -> String {
    let mut normalized = String::with_capacity(error.len());
    let mut in_digits = false;
    let mut in_space = false;
    for character in error.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_digit() {
            if !in_digits {
                normalized.push('#');
            }
            in_digits = true;
            in_space = false;
            continue;
        }
        in_digits = false;
        if character.is_whitespace() {
            if !in_space {
                normalized.push(' ');
            }
            in_space = true;
            continue;
        }
        in_space = false;
        normalized.push(character);
    }
    normalized.chars().take(NORMALIZED_MAX_CHARACTERS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_is_sixteen_lower_case_hex_characters() {
        let value = fingerprint("connector:jira:sync", "GitHub: Bad credentials");
        assert_eq!(value.len(), FINGERPRINT_HEX_CHARACTERS);
        assert!(
            value
                .chars()
                .all(|character| matches!(character, '0'..='9' | 'a'..='f'))
        );
    }

    #[test]
    fn case_spacing_and_numbers_do_not_change_the_problem() {
        let first = fingerprint("connector:slack:sync", "Timed out after 30.2s (attempt 2)");
        let second = fingerprint(
            "connector:slack:sync",
            "  timed  OUT after 31.07s\n(attempt 3) ",
        );
        assert_eq!(first, second);
    }

    #[test]
    fn a_different_error_or_place_is_a_different_problem() {
        let base = fingerprint("connector:jira:check:sign_in", "Jira refused the token");
        assert_ne!(
            base,
            fingerprint("connector:jira:check:sign_in", "Jira could not be reached")
        );
        assert_ne!(
            base,
            fingerprint(
                "connector:confluence:check:sign_in",
                "Jira refused the token"
            )
        );
    }

    #[test]
    fn only_the_start_of_a_very_long_error_counts() {
        let start = "x".repeat(NORMALIZED_MAX_CHARACTERS);
        assert_eq!(
            fingerprint("app:test", &format!("{start} and then one ending")),
            fingerprint("app:test", &format!("{start} and then another")),
        );
    }

    #[test]
    fn fingerprints_are_stable_across_builds() {
        // Every issue already filed is matched by this value. A change here splits each problem
        // into an old issue and a new one, so it is made deliberately, with FINGERPRINT_VERSION.
        // `printf 'elysium-issue-report-v1\nconnector:github:sync\ngithub: bad credentials' |
        // sha256sum | cut -c1-16`
        assert_eq!(
            fingerprint("connector:github:sync", "GitHub: Bad credentials"),
            "2cd8ee68cdf0b0ad"
        );
        assert_eq!(normalize(" A  b\t1234 c "), "a b # c");
    }
}
