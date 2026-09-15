// Copyright © 2026 Jalapeno Labs

//! PKCE (RFC 7636), used twice: the broker proves itself to providers, and each
//! Elysium instance proves itself to the broker when redeeming a handoff code.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use sha2::{Digest, Sha256};

/// Length of an S256 challenge: 32 SHA-256 bytes in unpadded base64url.
pub const CHALLENGE_LENGTH: usize = 43;

/// A fresh high-entropy verifier: 32 random bytes as 43 base64url characters.
///
/// # Panics
/// Panics if the operating system cannot supply random bytes.
pub fn new_verifier() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random number generator is available");
    BASE64_URL.encode(bytes)
}

/// The S256 challenge for `verifier`.
pub fn challenge_for(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    BASE64_URL.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_rfc_7636_appendix_b_example() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifiers_are_fresh_and_challenges_have_the_expected_length() {
        let verifier = new_verifier();
        assert_ne!(verifier, new_verifier());
        assert_eq!(challenge_for(&verifier).len(), CHALLENGE_LENGTH);
    }
}
