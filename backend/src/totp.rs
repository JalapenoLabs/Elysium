//! TOTP second factor (RFC 6238), wrapping `totp-rs`.
//!
//! Kept free of HTTP so the auth handlers stay thin. Secrets are raw bytes; the
//! parameters (SHA-1, 6 digits, 30-second step, one-step skew) are the defaults
//! every common authenticator app expects (Google Authenticator, Bitwarden, ...),
//! so an enrolled secret works without app-specific configuration.

use eyre::WrapErr;
use totp_rs::{Algorithm, Secret, TOTP};

/// Issuer label shown in the authenticator app.
const ISSUER: &str = "Elysium";

/// Digits per code. Six is the universal authenticator-app default.
const DIGITS: usize = 6;

/// Steps of clock skew tolerated on either side (one 30s window).
const SKEW: u8 = 1;

/// Seconds per code. Thirty is the RFC 6238 and authenticator-app default.
const STEP: u64 = 30;

/// Generates a fresh random TOTP secret as raw bytes.
pub fn generate_secret() -> Vec<u8> {
    // The default 20-byte (160-bit) secret exceeds the SHA-1 minimum, so
    // building a `TOTP` from it below never fails on length.
    Secret::generate_secret()
        .to_bytes()
        .expect("generated TOTP secret is valid")
}

/// Builds an `otpauth://` provisioning URI for `account` to render as a QR code.
///
/// # Errors
/// Returns an error if the secret is too short for the algorithm.
pub fn provisioning_uri(secret: &[u8], account: &str) -> eyre::Result<String> {
    Ok(build(secret, account)?.get_url())
}

/// Verifies `code` against `secret` for the current time window.
///
/// # Errors
/// Returns an error if the secret is invalid or the system clock is unavailable.
pub fn verify(secret: &[u8], code: &str) -> eyre::Result<bool> {
    build(secret, "elysium")?
        .check_current(code)
        .wrap_err("failed to check TOTP code against the system clock")
}

/// Builds a `TOTP` from a secret with Elysium's fixed parameters.
///
/// The `account` label is presentational (it rides in the provisioning URI) and
/// does not affect code computation, so verification passes a placeholder.
fn build(secret: &[u8], account: &str) -> eyre::Result<TOTP> {
    TOTP::new(
        Algorithm::SHA1,
        DIGITS,
        SKEW,
        STEP,
        secret.to_vec(),
        Some(ISSUER.to_string()),
        account.to_string(),
    )
    .wrap_err("failed to build TOTP from secret")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_codes_own_secret_and_rejects_a_different_one() {
        let secret_a = generate_secret();
        let secret_b = generate_secret();
        let code_a = build(&secret_a, "elysium")
            .expect("builds")
            .generate_current()
            .expect("generates a current code");

        // The matching secret accepts its own current code; a different secret does
        // not. This is what lets a multi-authenticator sign-in identify which one
        // matched (see `password::verify_totp`).
        assert!(verify(&secret_a, &code_a).unwrap_or(false));
        assert!(!verify(&secret_b, &code_a).unwrap_or(false));
    }
}
