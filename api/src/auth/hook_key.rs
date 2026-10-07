// Copyright © 2026 Jalapeno Labs

//! The key Kratos sends with every webhook and courier message, so the API's internal
//! routes answer Kratos alone.
//!
//! `kratos/entrypoint.sh` derives Kratos's secrets from `ELYSIUM_ENCRYPTION_KEY` as the hex
//! SHA-256 of `elysium/kratos/<purpose>:<key>`; the hook key is the `hook` purpose. Both
//! sides pin the same test vector below, so they cannot drift apart.

use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};

/// The header Kratos sends the key in, named in `kratos/kratos.yml`.
pub const HOOK_KEY_HEADER: &str = "x-elysium-hook-key";

/// The key Kratos's webhooks carry, derived from the encryption key.
pub fn derive(encryption_key: &SecretString) -> SecretString {
    let digest = Sha256::digest(format!(
        "elysium/kratos/hook:{}",
        encryption_key.expose_secret()
    ));
    SecretString::from(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same vector `kratos/entrypoint.sh` produces for `ELYSIUM_ENCRYPTION_KEY=testkey`:
    /// `printf '%s' 'elysium/kratos/hook:testkey' | sha256sum`.
    #[test]
    fn matches_the_kratos_entrypoint() {
        let key = derive(&SecretString::from("testkey"));
        assert_eq!(
            key.expose_secret(),
            "d4870895079863d8b6814a32312cce9aec390f1abb9b472c4fb683d00fa83166"
        );
    }
}
