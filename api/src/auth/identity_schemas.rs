// Copyright © 2026 Jalapeno Labs

//! Moves people's identities from the legacy `person` schema to `person-v2`.
//!
//! Kratos validates traits only when an identity is written, and never rewrites a stored
//! identity when a schema changes. So a schema change is a new schema beside the old one
//! (`kratos/kratos.yml` registers both, and new identities take `person-v2`), and each
//! existing identity is moved on with one JSON Patch that names the new schema and rewrites
//! the traits it changed. Kratos validates the patched identity against `person-v2` and leaves
//! its credentials alone, so passwords, passkeys, and authenticator apps keep working.
//!
//! `person-v2` splits the name into first and last. A legacy name is split at its first
//! space: the first word is the first name, and the rest the last name, which keeps
//! compound surnames ("García Márquez", "van Beethoven") whole. A one-word name has no last
//! name. People correct a wrong split on their profile.
//!
//! The move runs in the background on every start, since the API does not wait for Kratos:
//! it retries until Kratos answers, moves what it finds, and ends. Until an identity is
//! moved, its traits still read as before ([`PersonName::Whole`]), so nobody is locked out
//! in between. See `docs/auth.md`.

use std::time::Duration;

use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use super::kratos::{Kratos, KratosError, ListedIdentity};

/// The schema every identity was created with before names were split.
pub const LEGACY_SCHEMA_ID: &str = "person";

/// The schema new identities take, and every identity is moved to.
pub const CURRENT_SCHEMA_ID: &str = "person-v2";

/// How long to wait before asking Kratos again. Kratos starts beside the API and is usually
/// up within seconds; this keeps a long outage from flooding the log.
const RETRY_DELAY: Duration = Duration::from_secs(15);

/// The longest first or last name `person-v2` allows, in characters.
const NAME_PART_MAX_CHARACTERS: usize = 50;

/// Moves every legacy identity onto [`CURRENT_SCHEMA_ID`], in the background, until done or
/// shut down.
pub fn start(kratos: Kratos, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match move_legacy_identities(&kratos).await {
                Ok(()) => return,
                Err(error) => event!(
                    name: "auth.identities.migrate.retry",
                    Level::WARN,
                    error.message = %error,
                    "Kratos could not list identities to move them to the current schema; retrying",
                ),
            }
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = tokio::time::sleep(RETRY_DELAY) => {}
            }
        }
    })
}

/// One pass. Fails only when Kratos cannot list identities; an identity Kratos refuses to
/// move is logged and left on the legacy schema, which still signs in, until the next start.
async fn move_legacy_identities(kratos: &Kratos) -> Result<(), KratosError> {
    let identities = kratos.identities().await?;
    let mut moved = 0_usize;
    for identity in identities
        .iter()
        .filter(|identity| identity.schema_id == LEGACY_SCHEMA_ID)
    {
        match move_identity(kratos, identity).await {
            Ok(()) => moved += 1,
            Err(error) => event!(
                name: "auth.identities.migrate.refused",
                Level::ERROR,
                identity.id = %identity.id,
                error.message = %error,
                "an identity could not be moved to the current schema; it keeps the legacy one",
            ),
        }
    }
    if moved > 0 {
        event!(
            name: "auth.identities.migrate.success",
            Level::INFO,
            identities.moved = moved,
            schema.id = CURRENT_SCHEMA_ID,
            "moved identities to the current schema",
        );
    }
    Ok(())
}

async fn move_identity(kratos: &Kratos, identity: &ListedIdentity) -> Result<(), KratosError> {
    let (first, last) =
        split_legacy_name(&identity.traits.name.full_name(), &identity.traits.email);
    // Only the name changes, so an email edited meanwhile is never overwritten.
    let patch = json!([
        { "op": "replace", "path": "/schema_id", "value": CURRENT_SCHEMA_ID },
        { "op": "replace", "path": "/traits/name", "value": { "first": first, "last": last } },
    ]);
    kratos.patch_identity(identity.id, patch).await
}

/// Splits a legacy one-string name at its first run of whitespace, into the first and the last
/// name. A blank name, which only an identity written through the admin API can have, takes
/// the email's local part, as `crate::models::user::Profile` does.
pub fn split_legacy_name(name: &str, email: &str) -> (String, String) {
    let name = name.trim();
    let (first, last) = name
        .split_once(char::is_whitespace)
        .map_or((name, ""), |(first, last)| (first, last.trim()));
    let first = if first.is_empty() {
        email
            .split('@')
            .next()
            .filter(|local| !local.is_empty())
            .unwrap_or(email)
    } else {
        first
    };
    (
        first.chars().take(NAME_PART_MAX_CHARACTERS).collect(),
        last.chars().take(NAME_PART_MAX_CHARACTERS).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(name: &str) -> (String, String) {
        split_legacy_name(name, "someone@example.com")
    }

    #[test]
    fn the_first_word_is_the_first_name_and_the_rest_the_last() {
        assert_eq!(
            split("Alex Navarro"),
            ("Alex".to_owned(), "Navarro".to_owned())
        );
        assert_eq!(
            split("Gabriel García Márquez"),
            ("Gabriel".to_owned(), "García Márquez".to_owned()),
        );
        assert_eq!(
            split("  Ludwig   van Beethoven "),
            ("Ludwig".to_owned(), "van Beethoven".to_owned())
        );
    }

    #[test]
    fn one_word_has_no_last_name() {
        assert_eq!(split("Cher"), ("Cher".to_owned(), String::new()));
    }

    #[test]
    fn a_blank_name_takes_the_email() {
        assert_eq!(split("   "), ("someone".to_owned(), String::new()));
        assert_eq!(
            split_legacy_name("", "@example.com"),
            ("@example.com".to_owned(), String::new()),
        );
    }

    #[test]
    fn parts_fit_the_current_schema() {
        let long = "é".repeat(80);
        let (first, last) = split(&format!("{long} {long}"));
        assert_eq!(first.chars().count(), NAME_PART_MAX_CHARACTERS);
        assert_eq!(last.chars().count(), NAME_PART_MAX_CHARACTERS);
    }
}
