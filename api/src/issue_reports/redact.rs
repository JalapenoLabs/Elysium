// Copyright © 2026 Jalapeno Labs

//! Deterministic redaction of the text a problem report carries.
//!
//! Every error message, log line, and note in a report passes through [`Redactor::redact`]
//! before it is shown in the preview or posted, so nothing that names a person, an account, or a
//! secret leaves Elysium. The same text always redacts to the same result, which is what lets a
//! fingerprint of the redacted error match the same problem across people and deployments.
//!
//! Two kinds of rules run, in a fixed order:
//!
//! - **Known values.** Names cannot be told from other words by a pattern, so the workspace's
//!   own record of who its people are and which accounts they sign in as (`personal_values` in
//!   `crate::models::issue_report`) is redacted wherever an error repeats it, case-insensitively
//!   and on word boundaries: a person's name and each part of it, logins, account ids, sites.
//! - **Patterns.** URLs (their host when it names a tenant, and always their path, query, and
//!   credentials), email addresses, bearer and basic credentials, JWTs, tokens with a known
//!   prefix (GitHub, Slack, Anthropic, `OpenAI`, AWS, Google, Atlassian, GitLab), secrets written
//!   as `key=value`, UUIDs, Teams conversation ids, IPv4 and IPv6 addresses, home directories,
//!   Slack ids, long hex ids, long random-looking strings, long numbers, and `@` mentions.
//!
//! Each placeholder is a word in square brackets, such as `[email]`, which Markdown shows as
//! written and no later rule matches again. Redaction errs toward hiding: a word that happens to
//! be someone's name is hidden in every error, which costs a little context and never leaks.

use std::net::Ipv6Addr;
use std::str::FromStr;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use url::{Host, Url};

/// Hosts that name a service everyone uses, never a person or their organization, so a URL on
/// one keeps its host. Any other host is redacted, keeping only a tenant suffix below.
///
/// The compose services are here too: an error naming `kratos` or `hydra` says which part of
/// Elysium's own stack failed, and those names are the same in every deployment.
const PUBLIC_HOSTS: &[&str] = &[
    "localhost",
    "api",
    "docker-proxy",
    "hydra",
    "kratos",
    "nginx",
    "postgres",
    "redis",
    "stalwart",
    "workspace",
    "github.com",
    "api.github.com",
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
    "api.atlassian.com",
    "auth.atlassian.com",
    "id.atlassian.com",
    "api.fireflies.ai",
    "app.fireflies.ai",
    "outlook.office.com",
    "outlook.office365.com",
    "outlook.live.com",
    "login.microsoftonline.com",
    "login.live.com",
    "graph.microsoft.com",
    "teams.microsoft.com",
    "teams.cloud.microsoft",
    "teams.live.com",
    "mail.google.com",
    "accounts.google.com",
    "oauth2.googleapis.com",
    "gmail.googleapis.com",
    "www.googleapis.com",
    "storage.googleapis.com",
    "slack.com",
    "app.slack.com",
    "api.slack.com",
    "s3.amazonaws.com",
    "storage.bunnycdn.com",
    "api.anthropic.com",
    "console.anthropic.com",
    "claude.ai",
    "api.openai.com",
    "registry-1.docker.io",
    "ghcr.io",
];

/// Domains whose subdomains are each one customer: a Jira site, a YouTrack site, a Slack
/// workspace, a `SharePoint` tenant, an S3 bucket, a Bunny pull zone. The suffix says which service
/// failed, so it stays; the label in front of it names the customer, so it goes.
const TENANT_SUFFIXES: &[&str] = &[
    "atlassian.net",
    "youtrack.cloud",
    "slack.com",
    "sharepoint.com",
    "amazonaws.com",
    "b-cdn.net",
    "github.io",
];

/// Mail providers whose domain names no employer, so an address on one is redacted without
/// also redacting the provider's name everywhere.
const PUBLIC_MAIL_DOMAINS: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "outlook.com",
    "hotmail.com",
    "live.com",
    "msn.com",
    "yahoo.com",
    "icloud.com",
    "me.com",
    "proton.me",
    "protonmail.com",
    "aol.com",
    "fastmail.com",
];

/// Known values shorter than this are not redacted on their own: two letters match inside too
/// many errors to be worth hiding, and no name, login, or id is that short.
const KNOWN_VALUE_MIN_CHARACTERS: usize = 3;

/// A URL with any scheme. Square brackets end it, so a URL this module already rewrote, such
/// as `https://[host]/[path]`, is never matched again.
static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b[a-z][a-z0-9+.-]{1,15}://[^\s<>"'`\[\]]+"#).expect("a valid pattern")
});

/// A host on a tenant suffix written without a scheme, such as `acme.atlassian.net`.
static TENANT_HOST_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    let suffixes: Vec<String> = TENANT_SUFFIXES
        .iter()
        .map(|suffix| regex::escape(suffix))
        .collect();
    Regex::new(&format!(
        r"(?i)\b(?:[a-z0-9-]+\.)+(?:{})\b",
        suffixes.join("|")
    ))
    .expect("a valid pattern")
});

/// A Teams conversation id, such as `19:abc123@thread.v2`, which names one chat.
static TEAMS_CONVERSATION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b\d{1,2}:[a-z0-9_.=-]+@(?:thread\.(?:v2|skype|tacv2)|unq\.gbl\.spaces)\b")
        .expect("a valid pattern")
});

static EMAIL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9-]+(?:\.[a-z0-9-]+)*\.[a-z]{2,}\b")
        .expect("a valid pattern")
});

/// A credential after its scheme, as an `Authorization` header writes it.
static AUTHORIZATION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(bearer|basic|token)(\s+)([A-Za-z0-9._~+/=-]{8,})").expect("a valid pattern")
});

/// A JSON Web Token: three base64url parts, the first two starting with `eyJ`, `{"`.
static JWT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*")
        .expect("a valid pattern")
});

/// Tokens whose provider marks them with a prefix: GitHub's five kinds and fine-grained tokens,
/// Slack's, Anthropic and `OpenAI` keys, AWS access key ids, Google API keys, Atlassian API
/// tokens, and GitLab's personal tokens.
static PREFIXED_SECRET_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\b(?:gh[pousr]_[A-Za-z0-9]{10,}",
        r"|github_pat_[A-Za-z0-9_]{10,}",
        r"|xox[a-z]-[A-Za-z0-9-]{8,}",
        r"|xapp-[A-Za-z0-9-]{8,}",
        r"|sk-(?:ant-)?[A-Za-z0-9_-]{16,}",
        r"|(?:AKIA|ASIA)[0-9A-Z]{16}",
        r"|AIza[0-9A-Za-z_-]{30,}",
        r"|ATATT[A-Za-z0-9_=-]{20,}",
        r"|glpat-[A-Za-z0-9_-]{16,})",
    ))
    .expect("a valid pattern")
});

/// A secret written after its name, as a query string, a form, JSON, or a log line writes it:
/// `password=hunter2`, `"token": "abc123..."`, `cookie: session=...`.
static NAMED_SECRET_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"(?i)\b(password|passwd|pwd|secret|client_secret|token|access_token|refresh_token"#,
        r#"|id_token|api[_-]?key|access[_-]?key|private[_-]?key|authorization|cookie|set-cookie"#,
        r#"|session[_-]?id|sid|signature|x-amz-signature|code_verifier)"#,
        r#"(["']?\s*[:=]\s*["']?)([^\s"'&,;]+)"#,
    ))
    .expect("a valid pattern")
});

static UUID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
        .expect("a valid pattern")
});

static IPV4_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\b(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}",
        r"(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\b",
    ))
    .expect("a valid pattern")
});

/// Something shaped like an IPv6 address, which is redacted only when it parses as one. It
/// must not touch a word on its left, so a Rust path such as `crate::github` is left alone.
static IPV6_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^|[^0-9a-z:_])((?:[0-9a-f]{1,4})?(?::[0-9a-f]{0,4}){2,7})\b")
        .expect("a valid pattern")
});

/// A home directory, whose name is usually its person's login.
static HOME_DIRECTORY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(/home/|/Users/|[A-Za-z]:\\Users\\)([^/\\\s]+)").expect("a valid pattern")
});

/// Slack's ids for people, bots, workspaces, and conversations, such as `U024BE7LH`. Redacted
/// only when they hold a digit, so an upper-case word such as `CONNECTION` is left alone.
static SLACK_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[UWBTCDGE][A-Z0-9]{8,11}\b").expect("a valid pattern"));

/// A long hexadecimal id, such as an Atlassian account id. Redacted only when it mixes digits
/// and letters, so a long number is left to [`LONG_NUMBER_PATTERN`].
static LONG_HEX_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[0-9a-f]{12,}\b").expect("a valid pattern"));

/// A long string that looks random: a key or token with no prefix this module knows. Redacted
/// only when it mixes digits with upper- and lower-case letters, which identifiers and words do
/// not.
static RANDOM_STRING_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9_+/-]{24,}={0,2}").expect("a valid pattern"));

/// A run of seven or more digits: an account number, a phone number's digits, a numeric user id.
static LONG_NUMBER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{7,}\b").expect("a valid pattern"));

/// An `@` mention of someone, which would also notify them if it were posted as written.
static MENTION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|[^A-Za-z0-9_@.\[])@([A-Za-z0-9][A-Za-z0-9_-]{0,38})").expect("a valid pattern")
});

/// Redacts what a report carries, with the workspace's known personal values. Build one per
/// report; it holds no state between calls, so redacting the same text twice gives the same
/// result.
#[derive(Debug, Clone)]
pub struct Redactor {
    /// Every known value at once, longest first, or `None` when there are none.
    known: Option<Regex>,
}

impl Redactor {
    /// A redactor that also hides each of `values` wherever it appears.
    ///
    /// A value is redacted whole and, for a name of several words, word by word; an email
    /// address also by its local part; a URL by its host's labels. Values shorter than
    /// [`KNOWN_VALUE_MIN_CHARACTERS`] are dropped.
    pub fn new(values: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let mut phrases: Vec<String> = Vec::new();
        for value in values {
            phrases.extend(phrases_of(value.as_ref()));
        }
        phrases.retain(|phrase| phrase.chars().count() >= KNOWN_VALUE_MIN_CHARACTERS);
        for phrase in &mut phrases {
            *phrase = phrase.to_lowercase();
        }
        // Longest first, so "Ada Lovelace" is taken whole before "Ada" could take part of it;
        // then alphabetically, so the pattern is the same whatever order the values came in.
        phrases.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        phrases.dedup();

        if phrases.is_empty() {
            return Self { known: None };
        }
        let alternatives: Vec<String> = phrases
            .iter()
            .map(|phrase| {
                // A word boundary only where the phrase itself starts or ends with a word
                // character, since `\b` beside a symbol would never match.
                let starts_with_word = phrase.chars().next().is_some_and(is_word_character);
                let ends_with_word = phrase.chars().last().is_some_and(is_word_character);
                format!(
                    "{}{}{}",
                    if starts_with_word { r"\b" } else { "" },
                    regex::escape(phrase),
                    if ends_with_word { r"\b" } else { "" },
                )
            })
            .collect();
        let known = Regex::new(&format!("(?i)(?:{})", alternatives.join("|")));
        match known {
            Ok(known) => Self { known: Some(known) },
            Err(error) => {
                // Escaped literals always compile, short of the size limit. Losing the known
                // values would post names unredacted, so a redactor without them refuses
                // everything rather than letting a name through.
                tracing::event!(
                    name: "issue_reports.redact.known_values_unusable",
                    tracing::Level::ERROR,
                    error.message = %error,
                    "the known personal values could not be compiled; every text redacts whole",
                );
                Self {
                    known: Some(Regex::new(r"(?s).+").expect("a valid pattern")),
                }
            }
        }
    }

    /// `text` with everything that could name a person, an account, or a secret replaced by a
    /// placeholder.
    pub fn redact(&self, text: &str) -> String {
        // URLs first: their hosts and paths hold names that later rules would only half catch.
        let text =
            URL_PATTERN.replace_all(text, |captures: &Captures<'_>| redact_url(&captures[0]));
        let text = replace_where(&TENANT_HOST_PATTERN, &text, |captures| {
            let host = captures[0].to_lowercase();
            if PUBLIC_HOSTS.contains(&host.as_str()) {
                return None;
            }
            Some(redact_host(&host))
        });
        // Before email addresses, which a Teams id resembles.
        let text = TEAMS_CONVERSATION_PATTERN.replace_all(&text, "[conversation]");
        let text = EMAIL_PATTERN.replace_all(&text, "[email]");

        let text = replace_where(&AUTHORIZATION_PATTERN, &text, |captures| {
            // "token refreshed" is prose; a credential carries a digit or a symbol.
            let credential = &captures[3];
            let looks_issued = credential
                .chars()
                .any(|character| !character.is_ascii_alphabetic());
            looks_issued.then(|| format!("{}{}[secret]", &captures[1], &captures[2]))
        });
        let text = JWT_PATTERN.replace_all(&text, "[secret]");
        let text = PREFIXED_SECRET_PATTERN.replace_all(&text, "[secret]");
        let text = replace_where(&NAMED_SECRET_PATTERN, &text, |captures| {
            let separator = &captures[2];
            let value = &captures[3];
            if value.starts_with('[') {
                return None;
            }
            // `password=x` is always a secret. After a colon it may be prose ("token: expired"),
            // so only a value that looks issued is taken.
            let is_assignment = separator.contains('=');
            let looks_issued = value.chars().any(|character| character.is_ascii_digit())
                || value.chars().count() >= 20;
            (is_assignment || looks_issued).then(|| format!("{}{separator}[secret]", &captures[1]))
        });

        let text = match &self.known {
            Some(known) => known.replace_all(&text, "[name]").into_owned(),
            None => text,
        };

        let text = UUID_PATTERN.replace_all(&text, "[id]");
        let text = IPV4_PATTERN.replace_all(&text, "[ip]");
        let text = replace_where(&IPV6_PATTERN, &text, |captures| {
            Ipv6Addr::from_str(&captures[2])
                .is_ok()
                .then(|| format!("{}[ip]", &captures[1]))
        });
        let text = HOME_DIRECTORY_PATTERN.replace_all(&text, "${1}[user]");
        let text = replace_where(&SLACK_ID_PATTERN, &text, |captures| {
            has_digit(&captures[0]).then(|| "[id]".to_owned())
        });
        let text = replace_where(&LONG_HEX_PATTERN, &text, |captures| {
            let id = &captures[0];
            let has_letter = id.chars().any(|character| character.is_ascii_alphabetic());
            (has_digit(id) && has_letter).then(|| "[id]".to_owned())
        });
        let text = replace_where(&RANDOM_STRING_PATTERN, &text, |captures| {
            let candidate = &captures[0];
            let has_upper = candidate
                .chars()
                .any(|character| character.is_ascii_uppercase());
            let has_lower = candidate
                .chars()
                .any(|character| character.is_ascii_lowercase());
            (has_digit(candidate) && has_upper && has_lower).then(|| "[secret]".to_owned())
        });
        let text = LONG_NUMBER_PATTERN.replace_all(&text, "[number]");
        let text = MENTION_PATTERN.replace_all(&text, "${1}@[user]");
        text.into_owned()
    }
}

/// The phrases one known value is redacted by.
fn phrases_of(value: &str) -> Vec<String> {
    let value = value.trim();
    if value.is_empty() {
        return Vec::new();
    }

    // A site: the host, and the label that names its owner.
    if let Ok(url) = Url::parse(value)
        && let Some(Host::Domain(domain)) = url.host()
    {
        let mut phrases = vec![domain.to_owned()];
        phrases.extend(owner_label(domain));
        return phrases;
    }

    let mut phrases = vec![value.to_owned()];
    // An address: its local part, which errors sometimes repeat on its own, and its domain's
    // owner, which names an employer unless it is a mail provider everyone uses.
    if let Some((local, domain)) = value.split_once('@') {
        phrases.push(local.to_owned());
        if !PUBLIC_MAIL_DOMAINS.contains(&domain.to_lowercase().as_str()) {
            phrases.extend(owner_label(domain));
        }
        return phrases;
    }
    // A name of several words: each word too, since an error may use the first name alone.
    let words: Vec<&str> = value.split_whitespace().collect();
    if words.len() > 1 {
        phrases.extend(words.into_iter().map(str::to_owned));
    }
    phrases
}

/// The label of `domain` that names who it belongs to: the one in front of a tenant suffix
/// (`acme` of `acme.atlassian.net`), or else the one in front of the top-level domain (`acme` of
/// `mail.acme.dev`). `None` for a domain of one label.
fn owner_label(domain: &str) -> Option<String> {
    let suffix_labels = TENANT_SUFFIXES
        .iter()
        .find(|suffix| domain.ends_with(&format!(".{suffix}")))
        .map_or(1, |suffix| suffix.split('.').count());
    let labels: Vec<&str> = domain.split('.').collect();
    let owner = labels.len().checked_sub(suffix_labels + 1)?;
    Some(labels[owner].to_owned())
}

/// A URL with its path, query, fragment, and credentials gone, and its host gone too unless it
/// names a public service.
fn redact_url(matched: &str) -> String {
    // Sentence punctuation after a URL is not part of it.
    let trimmed = matched.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '}', '\'', '"']);
    let trailing = &matched[trimmed.len()..];
    let Ok(url) = Url::parse(trimmed) else {
        return format!("[url]{trailing}");
    };

    let (host, is_public) = match url.host() {
        Some(Host::Domain(domain)) => {
            let domain = domain.to_lowercase();
            if PUBLIC_HOSTS.contains(&domain.as_str()) {
                (domain, true)
            } else {
                (redact_host(&domain), false)
            }
        }
        Some(Host::Ipv4(_) | Host::Ipv6(_)) => ("[ip]".to_owned(), false),
        // `file:///home/ada/...` and the like: nothing in it is worth keeping.
        None => return format!("[url]{trailing}"),
    };
    // A port says which service answered, and identifies no one, on a host that is kept.
    let port = match url.port() {
        Some(port) if is_public => format!(":{port}"),
        _ => String::new(),
    };
    let has_path = !matches!(url.path(), "" | "/") || url.query().is_some();
    let path = if has_path { "/[path]" } else { "" };
    format!("{}://{host}{port}{path}{trailing}", url.scheme())
}

/// A host that is not public: its tenant suffix when it has one, such as `[site].atlassian.net`,
/// and otherwise nothing of it.
fn redact_host(host: &str) -> String {
    TENANT_SUFFIXES
        .iter()
        .find(|suffix| host.ends_with(&format!(".{suffix}")))
        .map_or_else(|| "[host]".to_owned(), |suffix| format!("[site].{suffix}"))
}

/// `regex.replace_all`, where `replace` may decline a match by answering `None`, leaving it as
/// it was.
fn replace_where(
    regex: &Regex,
    text: &str,
    replace: impl Fn(&Captures<'_>) -> Option<String>,
) -> String {
    regex
        .replace_all(text, |captures: &Captures<'_>| {
            replace(captures).unwrap_or_else(|| captures[0].to_owned())
        })
        .into_owned()
}

fn has_digit(text: &str) -> bool {
    text.chars().any(|character| character.is_ascii_digit())
}

/// What `\b` counts as a word character, for the ASCII the known values are written in.
fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Redactor {
        Redactor::new(Vec::<String>::new())
    }

    /// Asserts that none of `secrets` survives anywhere in `redacted`, in any case.
    fn assert_hidden(redacted: &str, secrets: &[&str]) {
        let lowered = redacted.to_lowercase();
        for secret in secrets {
            assert!(
                !lowered.contains(&secret.to_lowercase()),
                "{secret:?} survived redaction: {redacted}"
            );
        }
    }

    #[test]
    fn email_addresses_are_hidden_whole() {
        let redacted = plain().redact("Mailbox ada.lovelace+work@example.co.uk refused the login");
        assert_eq!(redacted, "Mailbox [email] refused the login");
    }

    #[test]
    fn github_tokens_of_every_kind_are_hidden() {
        let redacted = plain().redact(
            "tried ghp_16C7e42F292c6912E7710c838347Ae178B4a, gho_abcdefghij1234, \
             github_pat_11ABCDEFG0abcdefghij_KLMNOPqrstuv and ghs_ZYXWVUTSRQ0987",
        );
        assert_hidden(
            &redacted,
            &[
                "16C7e42F292c",
                "gho_abcdefghij",
                "github_pat_11",
                "KLMNOPqrstuv",
                "ghs_ZYX",
            ],
        );
        assert_eq!(redacted.matches("[secret]").count(), 4, "{redacted}");
    }

    #[test]
    fn provider_keys_with_known_prefixes_are_hidden() {
        let redacted = plain().redact(concat!(
            "slack xoxb-1234-5678-abcdefgh, xoxc-99999-abc-def ",
            "anthropic sk-ant-api03-abcdefghijklmnopqrstuv ",
            "openai sk-proj1234567890abcdefgh ",
            "aws AKIAIOSFODNN7EXAMPLE ",
            "google AIzaSyA-1234567890abcdefghijklmnopqrstu ",
            "atlassian ATATT3xFfGF0abcdefghijklmnopqrst ",
            "gitlab glpat-abcdefghij1234567890",
        ));
        assert_hidden(
            &redacted,
            &[
                "xoxb-1234",
                "xoxc-9999",
                "api03",
                "sk-proj",
                "AKIAIOSF",
                "AIzaSyA",
                "ATATT3x",
                "glpat-",
            ],
        );
    }

    #[test]
    fn bearer_and_basic_credentials_are_hidden_but_prose_about_tokens_is_not() {
        let redacted = plain()
            .redact("Authorization: Bearer abc.def-123_xyz and Basic dXNlcjpwYXNzd29yZA== failed");
        assert_eq!(
            redacted,
            "Authorization: Bearer [secret] and Basic [secret] failed"
        );

        let prose = plain().redact("the token refreshed, then the bearer expired");
        assert_eq!(prose, "the token refreshed, then the bearer expired");
    }

    #[test]
    fn json_web_tokens_are_hidden() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let redacted = plain().redact(&format!("cookie jwt {jwt} rejected"));
        assert_hidden(&redacted, &["eyJhbGci", "dozjgNry"]);
    }

    #[test]
    fn secrets_written_after_their_name_are_hidden() {
        let redacted = plain().redact(
            r#"password=hunter2&user=1 {"access_token": "a1b2c3d4e5", "client_secret":"s3cr3t"} cookie: sid=99abc"#,
        );
        assert_hidden(&redacted, &["hunter2", "a1b2c3d4e5", "s3cr3t", "99abc"]);

        // After a colon, plain words are prose rather than a value.
        let prose = plain().redact("token: expired; session: closed");
        assert_eq!(prose, "token: expired; session: closed");
    }

    #[test]
    fn urls_keep_a_public_host_and_lose_every_path() {
        let redacted = plain().redact(
            "GET https://api.github.com/repos/acme/secret-project/issues?state=open failed.",
        );
        assert_eq!(redacted, "GET https://api.github.com/[path] failed.");
    }

    #[test]
    fn urls_on_a_tenant_keep_only_the_service() {
        let redacted = plain().redact(
            "error sending request for url (https://acme-corp.atlassian.net/rest/api/3/myself)",
        );
        assert_eq!(
            redacted,
            "error sending request for url (https://[site].atlassian.net/[path])"
        );

        let bare = plain().redact("site acme-corp.atlassian.net and widgets.youtrack.cloud");
        assert_eq!(bare, "site [site].atlassian.net and [site].youtrack.cloud");
    }

    #[test]
    fn urls_on_a_private_host_lose_it_with_their_credentials() {
        let redacted = plain().redact("connect https://ada:pa55word@mail.lovelace.dev:8443/inbox");
        assert_eq!(redacted, "connect https://[host]/[path]");
        assert_hidden(&redacted, &["ada", "pa55word", "lovelace", "8443"]);
    }

    #[test]
    fn urls_on_compose_services_keep_their_host_and_port() {
        let redacted = plain().redact("Kratos at http://kratos:4434 refused");
        assert_eq!(redacted, "Kratos at http://kratos:4434 refused");
    }

    #[test]
    fn a_public_slack_host_is_kept_and_a_workspace_is_not() {
        let redacted = plain()
            .redact("opened app.slack.com, then https://lovelace-labs.slack.com/archives/C1");
        assert_eq!(
            redacted,
            "opened app.slack.com, then https://[site].slack.com/[path]"
        );
    }

    #[test]
    fn ip_addresses_of_both_families_are_hidden() {
        let redacted =
            plain().redact("from 203.0.113.42 and 2001:db8::8a2e:370:7334 or fe80::1 at 10:30:00");
        assert_eq!(redacted, "from [ip] and [ip] or [ip] at 10:30:00");
    }

    #[test]
    fn rust_paths_and_times_are_not_mistaken_for_addresses() {
        let text = "crate::github::issues failed at 12:00:05 in std::fmt";
        assert_eq!(plain().redact(text), text);
    }

    #[test]
    fn uuids_slack_ids_and_long_hex_ids_are_hidden() {
        let redacted = plain().redact(
            "connector 0199a3c4-0000-7000-8000-000000000001 user U024BE7LH in T0G9PQBBK, \
             account 5b10ac8d82e05b22cc7d4ef5",
        );
        assert_eq!(redacted, "connector [id] user [id] in [id], account [id]");
    }

    #[test]
    fn upper_case_words_and_plain_numbers_stay() {
        let text = "CONNECTION REFUSED with status 502 after 30 seconds";
        assert_eq!(plain().redact(text), text);
    }

    #[test]
    fn teams_conversation_ids_are_hidden_before_they_read_as_addresses() {
        let redacted =
            plain().redact("chat 19:a1b2c3d4e5f6@thread.v2 and 19:meeting_abc@thread.skype");
        assert_eq!(redacted, "chat [conversation] and [conversation]");
    }

    #[test]
    fn home_directories_lose_their_owner() {
        let redacted = plain().redact("no such file /home/ada/.claude and C:\\Users\\ada\\x");
        assert_eq!(
            redacted,
            "no such file /home/[user]/.claude and C:\\Users\\[user]\\x"
        );
    }

    #[test]
    fn long_numbers_and_random_strings_are_hidden() {
        let redacted = plain().redact("account 123456789 key Zq9xW2vB7nM4kL1pR8tY3uI6oA5sD0fG");
        assert_eq!(redacted, "account [number] key [secret]");
    }

    #[test]
    fn identifiers_in_snake_case_survive() {
        let text = "check list_projects skipped, blocked by sign_in (fine_grained_scopes)";
        assert_eq!(plain().redact(text), text);
    }

    #[test]
    fn mentions_are_hidden_so_nobody_is_notified() {
        let redacted = plain().redact("assigned to @ada-lovelace by @bob");
        assert_eq!(redacted, "assigned to @[user] by @[user]");
    }

    #[test]
    fn known_names_are_hidden_whole_and_by_each_part_in_any_case() {
        let redactor = Redactor::new(["Ada Lovelace"]);
        let redacted =
            redactor.redact("Ada Lovelace's mailbox; ADA again; lovelace too; adamant stays");
        assert_eq!(
            redacted,
            "[name]'s mailbox; [name] again; [name] too; adamant stays"
        );
    }

    #[test]
    fn known_logins_accounts_and_sites_are_hidden() {
        let redactor = Redactor::new([
            "ada-lovelace",
            "712020:0f1e2d3c",
            "https://analytical-engines.atlassian.net",
            "ada@lovelace.dev",
        ]);
        let redacted = redactor
            .redact("login ada-lovelace, account 712020:0f1e2d3c on analytical-engines, user ada");
        assert_hidden(&redacted, &["lovelace", "712020", "analytical", " ada"]);
    }

    #[test]
    fn short_known_values_are_ignored() {
        let redactor = Redactor::new(["Al", "", "  "]);
        assert_eq!(redactor.redact("Al was here"), "Al was here");
    }

    #[test]
    fn known_values_order_does_not_change_the_result() {
        let text = "Ada Lovelace and Charles Babbage";
        let first = Redactor::new(["Ada Lovelace", "Charles Babbage", "Ada"]).redact(text);
        let second = Redactor::new(["Ada", "Charles Babbage", "Ada Lovelace"]).redact(text);
        assert_eq!(first, second);
        assert_eq!(first, "[name] and [name]");
    }

    #[test]
    fn redaction_is_deterministic_and_idempotent() {
        let redactor = Redactor::new(["Ada Lovelace", "ada-lovelace"]);
        let text = concat!(
            "Ada Lovelace (ada@lovelace.dev, @ada-lovelace) at https://acme.atlassian.net/browse/X-1 ",
            "from 198.51.100.7 with ghp_16C7e42F292c6912E7710c838347Ae178B4a and ",
            "id 0199a3c4-0000-7000-8000-000000000001 in /home/ada/work",
        );
        let once = redactor.redact(text);
        assert_eq!(once, redactor.redact(text));
        assert_eq!(redactor.redact(&once), once);
        assert_hidden(
            &once,
            &[
                "lovelace",
                "acme",
                "198.51",
                "ghp_",
                "0199a3c4",
                "/home/ada",
            ],
        );
    }

    #[test]
    fn two_people_hitting_the_same_failure_redact_to_the_same_text() {
        let first = Redactor::new(["Ada Lovelace", "ada@lovelace.dev"]).redact(
            "Jira refused ada@lovelace.dev at https://lovelace.atlassian.net/rest/api/3/myself: 401",
        );
        let second = Redactor::new(["Grace Hopper", "grace@navy.mil"]).redact(
            "Jira refused grace@navy.mil at https://hopper-cobol.atlassian.net/rest/api/3/myself: 401",
        );
        assert_eq!(first, second);
    }
}
