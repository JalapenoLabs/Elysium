// Copyright © 2026 Jalapeno Labs

//! Which environment variable keys Elysium refuses, and why.
//!
//! Environment variables are handed to every coding session's satellite thread, on top of
//! what Elysium and the satellite set themselves. Some keys cannot mean anything useful
//! there: Elysium sets them, the satellite refuses them, or the satellite overwrites them.
//! Accepting one would store a variable that silently never reaches the agent, or, worse,
//! one that fails a thread at creation. [`key_refusal`] is the one gate for all of them, so
//! the routes that store variables and the code that opens threads agree on the rules.
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
    /// The satellite refuses every `ARSOX_*` key: it holds the satellite's own secret.
    ReservedForSatellite,
    /// Shaped like a model provider's credential, which the satellite refuses because its
    /// LLM proxy presents the credential on the agent's behalf.
    ProviderCredential,
    /// The satellite sets this key after every declared variable, so a stored value would
    /// never reach the agent.
    OverriddenBySatellite,
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
            Self::ReservedForSatellite => {
                "is reserved for the satellite: no ARSOX_ variable is ever passed to an agent"
            }
            Self::ProviderCredential => {
                "is shaped like a model provider credential, which the satellite's LLM proxy \
                 presents on the agent's behalf"
            }
            Self::OverriddenBySatellite => {
                "is set by the satellite for every thread, so a value stored here would never \
                 reach the agent"
            }
        }
    }
}

/// How a rule recognizes the keys it refuses.
#[derive(Debug)]
enum KeyPattern {
    Exact(&'static str),
    Prefix(&'static str),
    /// See [`is_provider_credential`].
    ProviderCredential,
}

impl KeyPattern {
    fn matches(&self, key: &str) -> bool {
        match self {
            Self::Exact(refused) => key == *refused,
            Self::Prefix(prefix) => key.starts_with(prefix),
            Self::ProviderCredential => is_provider_credential(key),
        }
    }
}

#[derive(Debug)]
struct KeyRule {
    pattern: KeyPattern,
    refusal: KeyRefusal,
}

/// Every refused key, in the order they are checked.
const REFUSED_KEYS: [KeyRule; 14] = [
    // Elysium hands a thread its GitHub token through these. `GIT_CONFIG_` covers
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
    // Refused by the satellite when a thread is created (`declared_key_refusal` in
    // arsox-satellite's `crates/arsox-satellite/src/harness/spawn.rs`).
    KeyRule {
        pattern: KeyPattern::Prefix("ARSOX_"),
        refusal: KeyRefusal::ReservedForSatellite,
    },
    KeyRule {
        pattern: KeyPattern::ProviderCredential,
        refusal: KeyRefusal::ProviderCredential,
    },
    // Set by the satellite after everything a thread declares (`environment_for` in the same
    // file): the exec broker's `PATH`, the egress proxy in both cases, since tools disagree
    // about which spelling they read, and the LLM proxy's base URLs.
    KeyRule {
        pattern: KeyPattern::Exact("PATH"),
        refusal: KeyRefusal::OverriddenBySatellite,
    },
    KeyRule {
        pattern: KeyPattern::Exact("HTTP_PROXY"),
        refusal: KeyRefusal::OverriddenBySatellite,
    },
    KeyRule {
        pattern: KeyPattern::Exact("http_proxy"),
        refusal: KeyRefusal::OverriddenBySatellite,
    },
    KeyRule {
        pattern: KeyPattern::Exact("HTTPS_PROXY"),
        refusal: KeyRefusal::OverriddenBySatellite,
    },
    KeyRule {
        pattern: KeyPattern::Exact("https_proxy"),
        refusal: KeyRefusal::OverriddenBySatellite,
    },
    KeyRule {
        pattern: KeyPattern::Exact("NO_PROXY"),
        refusal: KeyRefusal::OverriddenBySatellite,
    },
    KeyRule {
        pattern: KeyPattern::Exact("no_proxy"),
        refusal: KeyRefusal::OverriddenBySatellite,
    },
    KeyRule {
        pattern: KeyPattern::Exact("ANTHROPIC_BASE_URL"),
        refusal: KeyRefusal::OverriddenBySatellite,
    },
    KeyRule {
        pattern: KeyPattern::Exact("OPENAI_BASE_URL"),
        refusal: KeyRefusal::OverriddenBySatellite,
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

/// Whether a key is shaped like a model provider's credential.
///
/// A mirror of `is_provider_credential` in arsox-satellite's
/// `crates/arsox-satellite/src/harness/spawn.rs`, which refuses a thread declaring such a key:
/// the satellite's LLM proxy holds the provider credential, so an agent never needs one and
/// must never be handed one. Matched on shape rather than by name, as the satellite does, so
/// the two agree on keys neither has listed. Keep the two lists in step.
fn is_provider_credential(key: &str) -> bool {
    const VENDORS: [&str; 6] = [
        "ANTHROPIC_",
        "OPENAI_",
        "AWS_",
        "AZURE_",
        "GOOGLE_",
        "DEEPSEEK_",
    ];
    const SECRETS: [&str; 5] = [
        "API_KEY",
        "AUTH_TOKEN",
        "ACCESS_KEY",
        "SECRET",
        "CREDENTIALS",
    ];

    VENDORS.iter().any(|vendor| key.starts_with(vendor))
        && SECRETS.iter().any(|secret| key.contains(secret))
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
            "ANTHROPIC_MODEL",
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
    fn keys_the_satellite_refuses_are_refused_here_too() {
        assert_eq!(
            key_refusal("ARSOX_SECRET"),
            Some(KeyRefusal::ReservedForSatellite)
        );
        for key in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "OPENAI_API_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "AZURE_CLIENT_SECRET",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "DEEPSEEK_API_KEY",
        ] {
            assert_eq!(
                key_refusal(key),
                Some(KeyRefusal::ProviderCredential),
                "{key}"
            );
        }
    }

    #[test]
    fn keys_the_satellite_overrides_are_refused_in_either_case_it_sets() {
        for key in [
            "PATH",
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "NO_PROXY",
            "no_proxy",
            "ANTHROPIC_BASE_URL",
            "OPENAI_BASE_URL",
        ] {
            assert_eq!(
                key_refusal(key),
                Some(KeyRefusal::OverriddenBySatellite),
                "{key}"
            );
        }
    }
}
