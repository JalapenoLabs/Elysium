// Copyright © 2026 Jalapeno Labs

//! Moving a person's two-factor authentication to a new authenticator app, never leaving it off.
//!
//! Kratos holds one authenticator app per identity, and its settings flow offers a new app's QR
//! code only once the old app is removed. Moving through it would leave the person without an
//! app from removing the old one until verifying the new one, and someone who walked away in
//! between would be left without two-factor authentication at all. So Elysium moves it itself:
//!
//! 1. [`NewAuthenticator::generate`] makes the new app's secret as Kratos would make it (20
//!    random bytes, SHA-1, six digits, 30-second steps, issued by Elysium to the person's email),
//!    and `crate::models::authenticator_replacement` holds it sealed for
//!    [`REPLACEMENT_LIFETIME`] while the person scans it.
//! 2. The person enters a code from the new app, which [`NewAuthenticator::accepts`] checks as
//!    Kratos checks a sign-in code: the current step, or one either side of it.
//! 3. [`replace`] writes the new secret over the old one in Kratos in one admin write, then
//!    forgets the person's cached sessions, since no settings webhook fires for an admin write.
//!
//! Until the third step the old app is the one in use, so leaving at any point changes nothing,
//! and from it on only the new app's codes sign in. The session doing it stays signed in with
//! two factors throughout. Only a session that signed in recently may start or finish a move
//! ([`recent_session`]), as Kratos asks of its own changes to how a person signs in.
//!
//! Adding a first app needs none of this: it goes through Kratos's settings flow, which also
//! marks the session as having used it. See `docs/auth.md`, Authenticator apps.

use std::time::Duration;

use chrono::{DateTime, Utc};
use redis::aio::ConnectionManager;
use totp_rs::{Builder, Totp, TotpError};
use uuid::Uuid;

use super::kratos::{Kratos, KratosError, Session};
use super::{AccessRefusal, sessions};
use crate::errors::ApiError;

/// How long a new app waits for its first code. Long enough to find a phone and scan a code
/// several times over; a row left behind past it is refused, and removed at the next start.
pub const REPLACEMENT_LIFETIME: Duration = Duration::from_mins(15);

/// How recently a session must have signed in to move to a new app:
/// `selfservice.flows.settings.privileged_session_max_age` in `kratos/kratos.yml`, so a move asks
/// what Kratos asks of every other change to how a person signs in. Change the two together.
pub const RECENT_SIGN_IN: chrono::Duration = chrono::Duration::minutes(15);

/// Who issues the app's codes, as authenticator apps label them:
/// `selfservice.methods.totp.config.issuer` in `kratos/kratos.yml`.
const ISSUER: &str = "Elysium";

/// How long a new secret is: Kratos's own `secretSize`, 160 bits, the size RFC 4226 recommends.
const SECRET_BYTES: usize = 20;

/// The authenticator app a person is moving to: its secret, and what their app shows it as.
pub struct NewAuthenticator {
    totp: Totp,
}

impl std::fmt::Debug for NewAuthenticator {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NewAuthenticator(..)")
    }
}

impl NewAuthenticator {
    /// A new app with a fresh random secret, for the person whose email is `account_name`.
    ///
    /// # Errors
    /// Returns [`TotpError`] for an account name an authenticator app cannot show, one holding
    /// a `:`.
    ///
    /// # Panics
    /// Panics if the operating system cannot supply random bytes; a secret without them would
    /// be guessable, so there is no fallback.
    pub fn generate(account_name: &str) -> Result<Self, TotpError> {
        let mut secret = zeroize::Zeroizing::new([0u8; SECRET_BYTES]);
        getrandom::fill(secret.as_mut_slice())
            .expect("the OS random number generator is available");
        Self::from_secret(secret.as_slice(), account_name)
    }

    /// The app whose secret is `secret`, as [`Self::generate`] made it and Elysium stored it.
    ///
    /// # Errors
    /// Returns [`TotpError`] for a secret shorter than 128 bits, or an account name holding a
    /// `:`.
    pub fn from_secret(secret: &[u8], account_name: &str) -> Result<Self, TotpError> {
        // SHA-1, six digits, and 30-second steps with one step of skew are both the builder's
        // defaults and what Kratos signs in with (`pquerna/otp`'s `totp.Validate`), whatever
        // the stored URL says, so they are spelled out rather than left to either default.
        let totp = Builder::new()
            .with_algorithm(totp_rs::Algorithm::SHA1)
            .with_digits(6)
            .with_step_duration(30)
            .with_skew(1)
            .with_secret(secret.to_vec())
            .with_issuer(Some(ISSUER))
            .with_account_name(account_name)
            .build()?;
        Ok(Self { totp })
    }

    /// The raw secret, for Elysium to seal while the person scans it.
    pub fn secret(&self) -> &[u8] {
        self.totp.secret().as_ref()
    }

    /// The secret as the key a person types into an app that cannot scan.
    pub fn secret_key(&self) -> String {
        self.totp.secret().to_base32()
    }

    /// The `otpauth://` URL the QR code holds, and Kratos stores as the app.
    pub fn url(&self) -> String {
        self.totp
            .to_url()
            .expect("the account name was checked when the app was built")
    }

    /// The QR code to scan, as a PNG `data:` URL, which the web app's policy allows for images.
    pub fn qr_code(&self) -> String {
        let png = self
            .totp
            .to_qr_base64()
            .expect("an email-length URL fits in a QR code, and encoding it as a PNG cannot fail");
        format!("data:image/png;base64,{png}")
    }

    /// Whether `code` is one the app shows at `at`, or one step either side of it, as Kratos
    /// accepts a code at sign-in.
    pub fn accepts(&self, code: &str, at: DateTime<Utc>) -> bool {
        let Ok(seconds) = u64::try_from(at.timestamp()) else {
            return false;
        };
        self.totp.check(code.trim(), seconds).is_some()
    }
}

/// The session `cookie` holds, asked of Kratos rather than the cache, refused unless it signed in
/// within [`RECENT_SIGN_IN`]. A cached answer could predate the sign-in the person just made to
/// get here: signing in again is no settings change, so no webhook drops the cached one.
///
/// # Errors
/// Returns [`AccessRefusal::RecentSignInRequired`] for a session that signed in too long ago, and
/// what [`Kratos::whoami`] does.
pub async fn recent_session(kratos: &Kratos, cookie: &str) -> Result<Session, ApiError> {
    let session = kratos.whoami(cookie).await?;
    if !signed_in_recently(&session, Utc::now()) {
        return Err(ApiError::AccessRefused(AccessRefusal::RecentSignInRequired));
    }
    Ok(session)
}

/// Whether `session` signed in within [`RECENT_SIGN_IN`] of `now`.
fn signed_in_recently(session: &Session, now: DateTime<Utc>) -> bool {
    now - session.authenticated_at <= RECENT_SIGN_IN
}

/// Makes `new` the authenticator app of `identity`, replacing the one it has, and forgets every
/// session of theirs the API cached so each asks Kratos again on its next request.
///
/// # Errors
/// Returns what [`Kratos`] does when it refuses, as it does for an identity without an app.
pub async fn replace(
    redis: &ConnectionManager,
    kratos: &Kratos,
    identity: Uuid,
    new: &NewAuthenticator,
) -> Result<(), KratosError> {
    kratos.replace_authenticator(identity, &new.url()).await?;
    sessions::forget(redis, identity).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::kratos::{AssuranceLevel, Identity, PersonName, Traits};

    /// RFC 6238's SHA-1 test secret, `12345678901234567890`.
    const RFC_SECRET: &[u8] = b"12345678901234567890";

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("a time after the epoch")
    }

    fn session_signed_in_at(authenticated_at: DateTime<Utc>) -> Session {
        Session {
            id: Uuid::now_v7(),
            expires_at: authenticated_at + chrono::Duration::hours(72),
            authenticated_at,
            authenticator_assurance_level: AssuranceLevel::Aal2,
            identity: Identity {
                id: Uuid::now_v7(),
                traits: Traits {
                    email: "ada@example.com".to_owned(),
                    name: PersonName::Whole("Ada".to_owned()),
                },
            },
        }
    }

    /// RFC 6238, Appendix B: at 59 seconds the six-digit SHA-1 code is `287082`.
    #[test]
    fn accepts_the_code_the_rfc_gives() {
        let app = NewAuthenticator::from_secret(RFC_SECRET, "ada@example.com").expect("builds");
        assert!(app.accepts("287082", at(59)));
        assert!(
            app.accepts(" 287082 ", at(59)),
            "pasted with spaces around it"
        );
        assert!(!app.accepts("287083", at(59)));
    }

    /// One step of skew either side, as Kratos allows at sign-in, and no more.
    #[test]
    fn accepts_a_code_one_step_either_side_and_no_further() {
        let app = NewAuthenticator::from_secret(RFC_SECRET, "ada@example.com").expect("builds");
        // `287082` is the code for the step from 30 to 59 seconds.
        assert!(app.accepts("287082", at(89)), "one step late");
        assert!(app.accepts("287082", at(0)), "one step early");
        assert!(!app.accepts("287082", at(90)), "two steps late");
    }

    #[test]
    fn a_generated_app_is_kratos_shaped_and_round_trips() {
        let app = NewAuthenticator::generate("ada@example.com").expect("builds");
        assert_eq!(app.secret().len(), SECRET_BYTES);

        let url = app.url();
        assert!(
            url.starts_with("otpauth://totp/Elysium:ada%40example.com?"),
            "{url}"
        );
        assert!(url.contains("issuer=Elysium"), "{url}");
        assert!(
            url.contains(&format!("secret={}", app.secret_key())),
            "{url}"
        );
        assert!(app.qr_code().starts_with("data:image/png;base64,"));

        let stored =
            NewAuthenticator::from_secret(app.secret(), "ada@example.com").expect("builds");
        assert_eq!(stored.url(), url, "the sealed secret rebuilds the same app");
    }

    #[test]
    fn two_generated_apps_never_share_a_secret() {
        let first = NewAuthenticator::generate("ada@example.com").expect("builds");
        let second = NewAuthenticator::generate("ada@example.com").expect("builds");
        assert_ne!(first.secret(), second.secret());
    }

    #[test]
    fn its_debug_output_holds_no_secret() {
        let app = NewAuthenticator::from_secret(RFC_SECRET, "ada@example.com").expect("builds");
        let rendered = format!("{app:?}");
        assert!(!rendered.contains(&app.secret_key()), "{rendered}");
        assert!(!rendered.contains("ada@example.com"), "{rendered}");
    }

    #[test]
    fn a_session_is_recent_for_fifteen_minutes_from_signing_in() {
        let now = Utc::now();
        assert!(signed_in_recently(&session_signed_in_at(now), now));
        assert!(signed_in_recently(
            &session_signed_in_at(now - RECENT_SIGN_IN),
            now
        ));
        assert!(!signed_in_recently(
            &session_signed_in_at(now - RECENT_SIGN_IN - chrono::Duration::seconds(1)),
            now,
        ));
    }
}
