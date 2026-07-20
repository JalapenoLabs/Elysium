//! Per-provider metadata, authorize-URL construction, PKCE, and profile parsing.
//!
//! Provider *endpoints* are hardcoded here (stable well-known URLs); only the app
//! *credentials* are configured per deployment (see [`super::store`]). Each provider
//! is one [`Provider`] variant mapping to a `'static` [`ProviderMeta`], the single
//! lookup table the rest of the subsystem dispatches through, so there is no
//! provider `match` scattered across the flow (docs/rust.md M-DI-HIERARCHY).
//!
//! Google and Microsoft are full OIDC providers: they support PKCE (S256) and an
//! OIDC `userinfo` endpoint, and need `access_type=offline`/`offline_access` to
//! return a refresh token. GitHub is plain OAuth2: no PKCE, its profile comes from
//! `/user` plus `/user/emails` (the primary verified address), and its tokens carry
//! neither expiry nor refresh. Every flow still uses an unguessable `state`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// A linked-service OAuth provider Elysium can be a client of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Github,
    Google,
    Microsoft,
}

/// The stable, hardcoded facts about a provider's OAuth/OIDC surface.
///
/// Endpoints are well-known and version-stable, so they live in code, not the
/// database; only the app credentials are configured. `read` names how to pull the
/// external identity out of that provider's `userinfo` JSON.
#[derive(Debug, Clone, Copy)]
pub struct ProviderMeta {
    /// The stored slug, e.g. `"github"`; the wire identifier the SPA uses.
    pub slug: &'static str,
    /// The human-facing label, e.g. `"GitHub"`.
    pub display_name: &'static str,
    /// The provider's authorization endpoint (where the user consents).
    pub authorize_url: &'static str,
    /// The provider's token endpoint (code -> tokens, and refresh).
    pub token_url: &'static str,
    /// The provider's userinfo/profile endpoint (bearer-authenticated).
    pub userinfo_url: &'static str,
    /// A secondary endpoint for the account's emails, when the profile omits one
    /// (GitHub's `/user/emails`); `None` when the profile always carries the email.
    pub emails_url: Option<&'static str>,
    /// The scopes requested by default, appropriate to the linked kind.
    pub default_scopes: &'static [&'static str],
    /// Whether the provider supports PKCE (S256); GitHub does not, so it is `false`.
    pub supports_pkce: bool,
    /// Extra fixed authorize parameters (e.g. Google's `access_type=offline`), which
    /// are what make the provider return a refresh token for silent renewal.
    pub extra_authorize_params: &'static [(&'static str, &'static str)],
}

// --- The provider metadata table ---------------------------------------------

/// GitHub: plain OAuth2. `read:user`/`user:email` for profile + verified email; the
/// email may be private, so it is resolved from `/user/emails` when `/user` omits it.
const GITHUB: ProviderMeta = ProviderMeta {
    slug: "github",
    display_name: "GitHub",
    authorize_url: "https://github.com/login/oauth/authorize",
    token_url: "https://github.com/login/oauth/access_token",
    userinfo_url: "https://api.github.com/user",
    emails_url: Some("https://api.github.com/user/emails"),
    default_scopes: &["read:user", "user:email"],
    supports_pkce: false,
    extra_authorize_params: &[],
};

/// Google: OIDC. `access_type=offline` + `prompt=consent` are required for Google to
/// return (and re-issue) a refresh token so the grant can be renewed silently.
const GOOGLE: ProviderMeta = ProviderMeta {
    slug: "google",
    display_name: "Google",
    authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo",
    emails_url: None,
    default_scopes: &["openid", "email", "profile"],
    supports_pkce: true,
    extra_authorize_params: &[("access_type", "offline"), ("prompt", "consent")],
};

/// Microsoft (multi-tenant `common`): OIDC. `offline_access` is what makes Entra ID
/// issue a refresh token; the OIDC `userinfo` endpoint carries the profile claims.
const MICROSOFT: ProviderMeta = ProviderMeta {
    slug: "microsoft",
    display_name: "Microsoft",
    authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
    token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
    userinfo_url: "https://graph.microsoft.com/oidc/userinfo",
    emails_url: None,
    default_scopes: &["openid", "email", "profile", "offline_access"],
    supports_pkce: true,
    extra_authorize_params: &[],
};

impl Provider {
    /// Every provider, in the order the providers list surfaces them.
    pub const ALL: [Provider; 3] = [Provider::Github, Provider::Google, Provider::Microsoft];

    /// The `'static` metadata for this provider (the single dispatch table).
    pub fn meta(self) -> &'static ProviderMeta {
        match self {
            Self::Github => &GITHUB,
            Self::Google => &GOOGLE,
            Self::Microsoft => &MICROSOFT,
        }
    }

    /// The stored/wire slug for this provider.
    pub fn slug(self) -> &'static str {
        self.meta().slug
    }

    /// Parses a provider slug from the URL/database, or `None` when unrecognized.
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.slug() == slug)
    }
}

// --- The external profile ----------------------------------------------------

/// The identity pulled from a provider's userinfo, normalized across providers.
///
/// `external_id` is the provider's stable, immutable account id and is required
/// (an account cannot be linked without it). The rest are best-effort display
/// fields and may be empty (a GitHub user can hide their email, an OIDC userinfo
/// can omit a picture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalProfile {
    pub external_id: String,
    pub username: String,
    pub email: String,
    pub avatar_url: String,
}

/// Parses a provider's userinfo JSON (plus GitHub's emails JSON) into a profile.
///
/// Returns `None` only when the required stable `external_id` is absent, which
/// means the response was not the profile shape we expect. `emails` is consulted
/// solely for GitHub, and only to fill an email the `/user` profile withheld.
pub fn parse_profile(
    provider: Provider,
    userinfo: &Value,
    emails: Option<&Value>,
) -> Option<ExternalProfile> {
    match provider {
        Provider::Github => parse_github(userinfo, emails),
        Provider::Google => parse_oidc(userinfo, "email"),
        // Microsoft's userinfo may key the address as `email`; fall back to the
        // `preferred_username` (typically the UPN/email) when it does not.
        Provider::Microsoft => parse_oidc(userinfo, "email"),
    }
}

/// Parses a GitHub `/user` object, resolving the email from `/user/emails` if hidden.
fn parse_github(userinfo: &Value, emails: Option<&Value>) -> Option<ExternalProfile> {
    // GitHub's id is a JSON number; stringify it as the stable external id.
    let external_id = match userinfo.get("id") {
        Some(Value::Number(id)) => id.to_string(),
        Some(Value::String(id)) if !id.is_empty() => id.clone(),
        _ => return None,
    };
    let username = string_field(userinfo, "login");
    let avatar_url = string_field(userinfo, "avatar_url");
    // Prefer the profile email; fall back to the primary, verified address.
    let email = {
        let profile_email = string_field(userinfo, "email");
        if profile_email.is_empty() {
            emails.and_then(github_primary_email).unwrap_or_default()
        } else {
            profile_email
        }
    };
    Some(ExternalProfile {
        external_id,
        username,
        email,
        avatar_url,
    })
}

/// Selects the primary, verified address from GitHub's `/user/emails` array.
fn github_primary_email(emails: &Value) -> Option<String> {
    let list = emails.as_array()?;
    // Prefer the primary+verified address; fall back to any verified one.
    let verified = |entry: &&Value| entry.get("verified").and_then(Value::as_bool) == Some(true);
    let primary = |entry: &&Value| entry.get("primary").and_then(Value::as_bool) == Some(true);
    list.iter()
        .find(|entry| primary(entry) && verified(entry))
        .or_else(|| list.iter().find(verified))
        .map(|entry| string_field(entry, "email"))
        .filter(|email| !email.is_empty())
}

/// Parses a standard OIDC userinfo object (`sub`, `email`, `name`, `picture`).
fn parse_oidc(userinfo: &Value, email_key: &str) -> Option<ExternalProfile> {
    let external_id = {
        let sub = string_field(userinfo, "sub");
        if sub.is_empty() {
            return None;
        }
        sub
    };
    let email = {
        let email = string_field(userinfo, email_key);
        if email.is_empty() {
            string_field(userinfo, "preferred_username")
        } else {
            email
        }
    };
    // Display name falls back through name -> email so a card is never blank.
    let username = {
        let name = string_field(userinfo, "name");
        if name.is_empty() {
            email.clone()
        } else {
            name
        }
    };
    Some(ExternalProfile {
        external_id,
        username,
        email,
        avatar_url: string_field(userinfo, "picture"),
    })
}

/// Reads a string field from a JSON object, defaulting a missing/non-string to "".
fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

// --- Authorize URL + PKCE ----------------------------------------------------

/// A generated PKCE pair: the `verifier` to store and the `challenge` to send.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// Generates a PKCE S256 pair: a 43-char verifier and its base64url SHA-256 challenge.
///
/// The verifier is 32 CSPRNG bytes as base64url (43 chars), within RFC 7636's
/// 43-128 range; the challenge is `base64url(sha256(verifier))`.
pub fn generate_pkce() -> Pkce {
    let mut entropy = [0u8; 32];
    OsRng.fill_bytes(&mut entropy);
    let verifier = URL_SAFE_NO_PAD.encode(entropy);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Pkce {
        verifier,
        challenge,
    }
}

/// Generates an opaque, unguessable `state`: 32 CSPRNG bytes as base64url.
pub fn generate_state() -> String {
    let mut entropy = [0u8; 32];
    OsRng.fill_bytes(&mut entropy);
    URL_SAFE_NO_PAD.encode(entropy)
}

/// The lookup hash of a `state` value: lowercase-hex SHA-256 of the plaintext.
///
/// A `state` is a bearer value, only ever compared, so the row keys on this one-way
/// hash; the callback hashes the presented state and looks it up.
pub fn hash_state(state: &str) -> String {
    hex_sha256(state.as_bytes())
}

/// Builds a provider authorization URL for the connect redirect.
///
/// Emits the standard Authorization-Code parameters (`response_type=code`,
/// `client_id`, `redirect_uri`, `scope`, `state`), the S256 PKCE challenge when the
/// provider supports it, and the provider's fixed extra params (e.g. Google's
/// `access_type=offline`). Every value is percent-encoded.
pub fn authorize_url(
    meta: &ProviderMeta,
    client_id: &str,
    redirect_uri: &str,
    scopes: &[String],
    state: &str,
    pkce_challenge: Option<&str>,
) -> String {
    let scope = scopes.join(" ");
    let mut pairs: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", &scope),
        ("state", state),
    ];
    if let Some(challenge) = pkce_challenge {
        pairs.push(("code_challenge", challenge));
        pairs.push(("code_challenge_method", "S256"));
    }
    for (key, value) in meta.extra_authorize_params {
        pairs.push((key, value));
    }
    format!("{}?{}", meta.authorize_url, build_query(&pairs))
}

/// The scopes to request for a fresh connect: the provider's documented defaults.
pub fn default_scopes(meta: &ProviderMeta) -> Vec<String> {
    meta.default_scopes
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// Splits a provider `scope` response into a list, tolerating space or comma joins.
///
/// Google/Microsoft space-delimit; GitHub comma-delimits. An empty or absent value
/// falls back to `fallback` (the scopes we requested) so the stored set is never
/// empty for a provider that echoes nothing.
pub fn parse_granted_scopes(scope: Option<&str>, fallback: &[String]) -> Vec<String> {
    let mut scopes: Vec<String> = Vec::new();
    for token in scope.unwrap_or_default().split([' ', ',']) {
        let token = token.trim();
        if !token.is_empty() && !scopes.iter().any(|existing| existing == token) {
            scopes.push(token.to_string());
        }
    }
    if scopes.is_empty() {
        fallback.to_vec()
    } else {
        scopes
    }
}

// --- Small self-contained encoders -------------------------------------------

/// Joins query pairs, percent-encoding each key and value.
fn build_query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encodes a query value, preserving only the RFC 3986 unreserved set.
///
/// Self-contained so the crate needs no URL dependency (mirrors `oidc::endpoints`).
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Lowercase-hex SHA-256 of the input bytes (the state hashing primitive).
fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Infallible: writing to a String never fails.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- Metadata lookup -----------------------------------------------------

    #[test]
    fn slugs_round_trip_through_the_lookup_table() {
        for provider in Provider::ALL {
            assert_eq!(Provider::from_slug(provider.slug()), Some(provider));
        }
        assert_eq!(Provider::from_slug("gitlab"), None);
        assert_eq!(Provider::from_slug(""), None);
    }

    #[test]
    fn v1_providers_are_github_google_microsoft() {
        let slugs: Vec<&str> = Provider::ALL.iter().map(|p| p.slug()).collect();
        assert_eq!(slugs, vec!["github", "google", "microsoft"]);
    }

    #[test]
    fn only_oidc_providers_support_pkce() {
        // GitHub is plain OAuth2; Google and Microsoft are OIDC and support PKCE.
        assert!(!Provider::Github.meta().supports_pkce);
        assert!(Provider::Google.meta().supports_pkce);
        assert!(Provider::Microsoft.meta().supports_pkce);
    }

    #[test]
    fn refresh_bearing_providers_request_offline_access() {
        // Google gets a refresh token via access_type=offline; Microsoft via the
        // offline_access scope. Both are what make silent refresh possible.
        assert!(Provider::Google
            .meta()
            .extra_authorize_params
            .contains(&("access_type", "offline")));
        assert!(Provider::Microsoft
            .meta()
            .default_scopes
            .contains(&"offline_access"));
    }

    // --- Authorize URL construction ------------------------------------------

    #[test]
    fn authorize_url_carries_the_standard_and_pkce_parameters() {
        let scopes = vec!["openid".to_string(), "email".to_string()];
        let url = authorize_url(
            Provider::Google.meta(),
            "client-123",
            "http://localhost:5173/api/v1/oauth/callback/google",
            &scopes,
            "state-xyz",
            Some("challenge-abc"),
        );
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client-123"));
        // The redirect URI and space-joined scope are percent-encoded.
        assert!(url.contains(
            "redirect_uri=http%3A%2F%2Flocalhost%3A5173%2Fapi%2Fv1%2Foauth%2Fcallback%2Fgoogle"
        ));
        assert!(url.contains("scope=openid%20email"));
        assert!(url.contains("state=state-xyz"));
        assert!(url.contains("code_challenge=challenge-abc"));
        assert!(url.contains("code_challenge_method=S256"));
        // Google's offline params ride through so a refresh token is issued.
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
    }

    #[test]
    fn authorize_url_omits_pkce_for_plain_oauth2() {
        let scopes = default_scopes(Provider::Github.meta());
        let url = authorize_url(
            Provider::Github.meta(),
            "gh-client",
            "http://localhost:5173/api/v1/oauth/callback/github",
            &scopes,
            "st",
            None,
        );
        assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
        // No PKCE parameters when the provider does not support it.
        assert!(!url.contains("code_challenge"));
        assert!(url.contains("scope=read%3Auser%20user%3Aemail"));
    }

    // --- PKCE + state --------------------------------------------------------

    #[test]
    fn pkce_challenge_is_the_base64url_sha256_of_the_verifier() {
        let pkce = generate_pkce();
        // RFC 7636: 32 bytes base64url is 43 chars, within the 43-128 range.
        assert_eq!(pkce.verifier.len(), 43);
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()));
        assert_eq!(pkce.challenge, expected);
        // Each pair is fresh.
        assert_ne!(generate_pkce().verifier, generate_pkce().verifier);
    }

    #[test]
    fn state_hash_is_deterministic_and_matches_a_known_vector() {
        assert_eq!(hash_state("abc"), hash_state("abc"));
        assert_ne!(hash_state("abc"), hash_state("abd"));
        // SHA-256("abc") published vector proves we hash the plaintext.
        assert_eq!(
            hash_state("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(generate_state(), generate_state());
    }

    // --- Granted-scope parsing -----------------------------------------------

    #[test]
    fn granted_scopes_split_on_space_or_comma_and_dedupe() {
        // GitHub comma-joins its granted scopes.
        assert_eq!(
            parse_granted_scopes(Some("read:user,user:email"), &[]),
            vec!["read:user".to_string(), "user:email".to_string()]
        );
        // Google/Microsoft space-join.
        assert_eq!(
            parse_granted_scopes(Some("openid email openid"), &[]),
            vec!["openid".to_string(), "email".to_string()]
        );
        // An empty response falls back to the requested scopes.
        let fallback = vec!["openid".to_string()];
        assert_eq!(parse_granted_scopes(None, &fallback), fallback);
        assert_eq!(parse_granted_scopes(Some("  "), &fallback), fallback);
    }

    // --- Userinfo parsing, one sample body per provider ----------------------

    #[test]
    fn github_profile_uses_numeric_id_and_emails_fallback() {
        // The /user profile hides the email (null), so it comes from /user/emails.
        let userinfo = json!({
            "id": 583231,
            "login": "octocat",
            "email": null,
            "avatar_url": "https://avatars.githubusercontent.com/u/583231"
        });
        let emails = json!([
            { "email": "secondary@example.com", "primary": false, "verified": true },
            { "email": "octocat@github.com", "primary": true, "verified": true }
        ]);
        let profile = parse_profile(Provider::Github, &userinfo, Some(&emails)).expect("a profile");
        assert_eq!(profile.external_id, "583231");
        assert_eq!(profile.username, "octocat");
        assert_eq!(profile.email, "octocat@github.com");
        assert_eq!(
            profile.avatar_url,
            "https://avatars.githubusercontent.com/u/583231"
        );
    }

    #[test]
    fn github_prefers_the_profile_email_when_present() {
        let userinfo = json!({ "id": 1, "login": "mona", "email": "mona@github.com" });
        let profile = parse_profile(Provider::Github, &userinfo, None).expect("a profile");
        assert_eq!(profile.email, "mona@github.com");
    }

    #[test]
    fn github_without_an_id_is_not_a_profile() {
        let userinfo = json!({ "login": "nobody" });
        assert!(parse_profile(Provider::Github, &userinfo, None).is_none());
    }

    #[test]
    fn google_profile_reads_the_oidc_claims() {
        let userinfo = json!({
            "sub": "110248495921238986420",
            "email": "alex@gmail.com",
            "name": "Alex Rivera",
            "picture": "https://lh3.googleusercontent.com/a/photo"
        });
        let profile = parse_profile(Provider::Google, &userinfo, None).expect("a profile");
        assert_eq!(profile.external_id, "110248495921238986420");
        assert_eq!(profile.email, "alex@gmail.com");
        assert_eq!(profile.username, "Alex Rivera");
        assert_eq!(
            profile.avatar_url,
            "https://lh3.googleusercontent.com/a/photo"
        );
    }

    #[test]
    fn microsoft_profile_falls_back_to_preferred_username_for_email() {
        // Entra ID userinfo can omit `email` and carry the UPN as preferred_username.
        let userinfo = json!({
            "sub": "AAAAAAAAAAAAAAAAAAAAAAA",
            "preferred_username": "alex@contoso.com",
            "name": "Alex Rivera"
        });
        let profile = parse_profile(Provider::Microsoft, &userinfo, None).expect("a profile");
        assert_eq!(profile.external_id, "AAAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(profile.email, "alex@contoso.com");
        assert_eq!(profile.username, "Alex Rivera");
    }

    #[test]
    fn oidc_username_falls_back_to_email_when_name_is_absent() {
        let userinfo = json!({ "sub": "s", "email": "e@x.test" });
        let profile = parse_profile(Provider::Google, &userinfo, None).expect("a profile");
        assert_eq!(profile.username, "e@x.test");
    }
}
