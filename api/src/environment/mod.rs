// Copyright © 2026 Jalapeno Labs

//! Which environment variable keys Elysium refuses, and why.
//!
//! Environment variables are handed to every agent turn, beneath what Elysium sets itself
//! (`crate::runner`, `crate::sessions`). Some keys cannot mean anything useful there: Elysium
//! sets them for every turn, so a stored value would never reach the agent, or would change
//! which model credential a turn runs on behind the LLM settings' back. [`key_refusal`] is the
//! one gate for all of them, so the routes that store variables and the code that starts turns
//! agree on the rules.
//!
//! The rules are a lookup table, `REFUSED_KEYS`, matched top to bottom. Keys are matched
//! with their exact case, as processes see them: `PATH` and `Path` are different variables.
//! The frontend mirrors this table in `src/pages/Settings/Environment/environmentPresentation.ts`.

/// The longest key Elysium stores. Mirrored by `environment_variables_key_length`.
pub const KEY_MAX_CHARACTERS: usize = 128;

/// Why a key cannot be stored. Each reason completes the sentence "`KEY` ...".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRefusal {
    /// Not a name a process can carry, or longer than [`KEY_MAX_CHARACTERS`].
    Malformed,
    /// Elysium sets this key itself, for GitHub.
    ReservedForElysium,
    /// The runner sets this key for every turn: its configuration directory, or Elysium's
    /// own `ELYSIUM_*` wiring.
    ReservedForRunner,
    /// Decides which model credential, or which endpoint, a turn's CLI uses. Turns run on the
    /// credentials under Settings, LLMs, in priority order.
    ModelCredential,
}

impl KeyRefusal {
    /// The reason, worded to follow the key it refuses.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Malformed => {
                "is not a valid key: use a letter or underscore, then letters, digits, or \
                 underscores, up to 128 characters"
            }
            Self::ReservedForElysium => "is reserved for Elysium, which sets it for GitHub",
            Self::ReservedForRunner => {
                "is reserved for Elysium's runner, which sets it for every agent turn"
            }
            Self::ModelCredential => {
                "decides the model credential an agent runs on; add credentials under \
                 Settings, LLMs instead"
            }
        }
    }
}

/// How a rule recognizes the keys it refuses.
#[derive(Debug)]
enum KeyPattern {
    Exact(&'static str),
    Prefix(&'static str),
}

impl KeyPattern {
    fn matches(&self, key: &str) -> bool {
        match self {
            Self::Exact(refused) => key == *refused,
            Self::Prefix(prefix) => key.starts_with(prefix),
        }
    }
}

#[derive(Debug)]
struct KeyRule {
    pattern: KeyPattern,
    refusal: KeyRefusal,
}

/// Every refused key, in the order they are checked.
const REFUSED_KEYS: [KeyRule; 9] = [
    // Elysium hands a turn its GitHub token through these. `GIT_CONFIG_` covers
    // `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT`, and the numbered `GIT_CONFIG_KEY_<n>` and
    // `GIT_CONFIG_VALUE_<n>` git reads alongside it.
    KeyRule {
        pattern: KeyPattern::Exact("GH_TOKEN"),
        refusal: KeyRefusal::ReservedForElysium,
    },
    KeyRule {
        pattern: KeyPattern::Exact("GITHUB_TOKEN"),
        refusal: KeyRefusal::ReservedForElysium,
    },
    KeyRule {
        pattern: KeyPattern::Prefix("GIT_CONFIG_"),
        refusal: KeyRefusal::ReservedForElysium,
    },
    // The workspace image points Claude Code at the configuration repository's checkout, and
    // the turn script reads Elysium's own variables (`ELYSIUM_MCP_CONFIG`).
    KeyRule {
        pattern: KeyPattern::Exact("CLAUDE_CONFIG_DIR"),
        refusal: KeyRefusal::ReservedForRunner,
    },
    KeyRule {
        pattern: KeyPattern::Prefix("ELYSIUM_"),
        refusal: KeyRefusal::ReservedForRunner,
    },
    // What Claude Code reads to choose its credential and endpoint. The runner sets the
    // credential for every attempt and fails over between them; a stored one would override
    // that or be overridden by it.
    KeyRule {
        pattern: KeyPattern::Exact("CLAUDE_CODE_OAUTH_TOKEN"),
        refusal: KeyRefusal::ModelCredential,
    },
    KeyRule {
        pattern: KeyPattern::Exact("ANTHROPIC_API_KEY"),
        refusal: KeyRefusal::ModelCredential,
    },
    KeyRule {
        pattern: KeyPattern::Exact("ANTHROPIC_AUTH_TOKEN"),
        refusal: KeyRefusal::ModelCredential,
    },
    KeyRule {
        pattern: KeyPattern::Exact("ANTHROPIC_BASE_URL"),
        refusal: KeyRefusal::ModelCredential,
    },
];

/// Why `key` cannot be stored as an environment variable, or `None` when it can.
///
/// A malformed key is refused before any rule is consulted, so every rule only ever sees a
/// name a process could carry.
pub fn key_refusal(key: &str) -> Option<KeyRefusal> {
    if !is_well_formed(key) {
        return Some(KeyRefusal::Malformed);
    }

    REFUSED_KEYS
        .iter()
        .find(|rule| rule.pattern.matches(key))
        .map(|rule| rule.refusal)
}

/// A letter or underscore, then letters, digits, or underscores, up to
/// [`KEY_MAX_CHARACTERS`]: the names every shell accepts. Mirrored by the
/// `environment_variables_key_shape` constraint.
fn is_well_formed(key: &str) -> bool {
    let mut characters = key.chars();
    let Some(first) = characters.next() else {
        return false;
    };

    key.len() <= KEY_MAX_CHARACTERS
        && (first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_keys_are_accepted() {
        for key in [
            "NPM_TOKEN",
            "DATABASE_URL",
            "AWS_REGION",
            "AWS_SECRET_ACCESS_KEY",
            "OPENAI_API_KEY",
            "ANTHROPIC_MODEL",
            "PATH",
            "_PRIVATE",
            "CI",
            "Path",
            "GIT_AUTHOR_NAME",
        ] {
            assert_eq!(key_refusal(key), None, "{key} should be accepted");
        }
    }

    #[test]
    fn malformed_keys_are_refused_before_any_rule() {
        let too_long = "A".repeat(KEY_MAX_CHARACTERS + 1);
        for key in [
            "",
            "1PASSWORD",
            "HAS SPACE",
            "HAS-DASH",
            "KEY=VALUE",
            "CAFÉ",
            &too_long,
        ] {
            assert_eq!(
                key_refusal(key),
                Some(KeyRefusal::Malformed),
                "{key} should be malformed"
            );
        }
        assert_eq!(key_refusal(&"A".repeat(KEY_MAX_CHARACTERS)), None);
    }

    #[test]
    fn keys_elysium_sets_for_github_are_reserved() {
        for key in [
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_KEY_0",
        ] {
            assert_eq!(
                key_refusal(key),
                Some(KeyRefusal::ReservedForElysium),
                "{key}"
            );
        }
    }

    #[test]
    fn keys_the_runner_sets_are_reserved() {
        for key in [
            "CLAUDE_CONFIG_DIR",
            "ELYSIUM_MCP_CONFIG",
            "ELYSIUM_ANYTHING",
        ] {
            assert_eq!(
                key_refusal(key),
                Some(KeyRefusal::ReservedForRunner),
                "{key}"
            );
        }
    }

    #[test]
    fn keys_that_choose_the_model_credential_are_refused() {
        for key in [
            "CLAUDE_CODE_OAUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
        ] {
            assert_eq!(key_refusal(key), Some(KeyRefusal::ModelCredential), "{key}");
        }
    }
}
