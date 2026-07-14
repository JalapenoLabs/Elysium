//! Authenticated encryption for secrets at rest (XChaCha20-Poly1305).
//!
//! A few database blobs would defeat authentication even offline if they leaked:
//! the OPAQUE server setup (the root of every stored password record) and each
//! user's TOTP secret. This module seals them under a process-wide key read from
//! the `ELYSIUM_SECRET_KEY` environment variable, so a database dump alone,
//! without the key, reveals nothing usable.
//!
//! The sealed envelope is `MAGIC || nonce || ciphertext+tag`. `MAGIC` (`ELY1`)
//! marks the format and scheme version; being a fixed four-byte tag, it also lets
//! [`Crypto::open`] tell a sealed blob from legacy plaintext with a negligible
//! (`2^-32`) false match, so rows written before encryption existed migrate
//! transparently on first read. Associated data binds each ciphertext to its
//! context (a TOTP secret to its user id), so a dump cannot be replayed by
//! shuffling ciphertext between rows.
//!
//! The 192-bit XChaCha20 nonce is random per seal; at that width, random nonces
//! carry no meaningful collision risk, so no nonce counter is needed.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use eyre::{eyre, WrapErr};

/// Marks our sealed envelope and its scheme version; bump to `ELY2` on a new one.
const MAGIC: &[u8; 4] = b"ELY1";

/// XChaCha20-Poly1305 nonce length, in bytes.
const NONCE_LEN: usize = 24;

/// Required key length, in bytes (XChaCha20-Poly1305 uses a 256-bit key).
const KEY_LEN: usize = 32;

/// The name of the environment variable holding the base64-encoded master key.
const KEY_VAR: &str = "ELYSIUM_SECRET_KEY";

/// Seals and opens secrets under the deployment's master key.
///
/// Cheaply cloneable (it holds the key schedule, not connections), so it lives in
/// [`crate::state::AppState`] and is handed to any code that reads or writes an
/// encrypted column.
#[derive(Clone)]
pub struct Crypto {
    cipher: XChaCha20Poly1305,
}

// Redacts the key: the debug output must never carry key material, even by
// accident, so it is a fixed string (see the `debug_redacts_key` test).
impl std::fmt::Debug for Crypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Crypto(<redacted>)")
    }
}

/// The result of opening a stored blob.
///
/// `was_sealed` is false for legacy plaintext written before encryption; the
/// caller should reseal and rewrite it so the column converges to all-encrypted.
pub struct Opened {
    pub plaintext: Vec<u8>,
    pub was_sealed: bool,
}

impl Crypto {
    /// Builds the cipher from `ELYSIUM_SECRET_KEY` (base64 of 32 random bytes).
    ///
    /// Generate a key with `openssl rand -base64 32`. This is required in every
    /// environment; there is no insecure default, so a missing or malformed key
    /// stops the process at boot rather than silently running unencrypted.
    ///
    /// # Errors
    /// Errors if the variable is unset, is not valid base64, or does not decode to
    /// exactly 32 bytes.
    pub fn from_env() -> eyre::Result<Self> {
        let encoded = std::env::var(KEY_VAR).map_err(|_| {
            eyre!(
                "{KEY_VAR} must be set (base64 of 32 random bytes, e.g. `openssl rand -base64 32`)"
            )
        })?;
        let key = STANDARD
            .decode(encoded.trim())
            .wrap_err_with(|| format!("{KEY_VAR} is not valid base64"))?;
        Self::new(&key)
    }

    /// Builds the cipher from raw key bytes, validating the length.
    ///
    /// # Errors
    /// Errors if `key` is not exactly [`KEY_LEN`] bytes.
    fn new(key: &[u8]) -> eyre::Result<Self> {
        if key.len() != KEY_LEN {
            return Err(eyre!(
                "{KEY_VAR} must decode to {KEY_LEN} bytes, got {}",
                key.len()
            ));
        }
        let cipher = XChaCha20Poly1305::new_from_slice(key).expect("key length was just validated");
        Ok(Self { cipher })
    }

    /// Seals `plaintext`, bound to `aad`, into a `MAGIC || nonce || ciphertext` blob.
    ///
    /// The same `aad` must be supplied to [`Crypto::open`]; it authenticates but is
    /// not stored, so callers pass a stable context value (e.g. a user id).
    pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .expect("XChaCha20-Poly1305 encryption does not fail for in-memory input");

        let mut sealed = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
        sealed.extend_from_slice(MAGIC);
        sealed.extend_from_slice(nonce.as_slice());
        sealed.extend_from_slice(&ciphertext);
        sealed
    }

    /// Opens `blob`: decrypts a sealed envelope, or passes legacy plaintext through.
    ///
    /// A blob without the `MAGIC` prefix predates encryption and is returned as-is
    /// with `was_sealed = false`, so the caller can reseal it.
    ///
    /// # Errors
    /// Errors only when the blob is sealed (carries `MAGIC`) but fails to
    /// authenticate, which means a truncated blob, tampering, or the wrong key,
    /// never legacy plaintext.
    pub fn open(&self, blob: &[u8], aad: &[u8]) -> eyre::Result<Opened> {
        let Some(body) = blob.strip_prefix(MAGIC.as_slice()) else {
            return Ok(Opened {
                plaintext: blob.to_vec(),
                was_sealed: false,
            });
        };
        if body.len() < NONCE_LEN {
            return Err(eyre!("sealed secret is truncated"));
        }
        let (nonce_bytes, ciphertext) = body.split_at(NONCE_LEN);
        let plaintext = self
            .cipher
            .decrypt(
                XNonce::from_slice(nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|error| {
                eyre!("failed to open sealed secret (wrong key or tampered): {error}")
            })?;
        Ok(Opened {
            plaintext,
            was_sealed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed 32-byte key for deterministic tests.
    fn test_crypto() -> Crypto {
        Crypto::new(&[7u8; KEY_LEN]).expect("32-byte key is valid")
    }

    #[test]
    fn seal_then_open_round_trips() {
        let crypto = test_crypto();
        let sealed = crypto.seal(b"totp-secret", b"user-42");
        let opened = crypto.open(&sealed, b"user-42").expect("opens");
        assert_eq!(opened.plaintext, b"totp-secret");
        assert!(opened.was_sealed);
    }

    #[test]
    fn each_seal_uses_a_fresh_nonce() {
        let crypto = test_crypto();
        // Same input, different ciphertext: the random nonce must differ per seal.
        assert_ne!(crypto.seal(b"x", b"a"), crypto.seal(b"x", b"a"));
    }

    #[test]
    fn wrong_aad_fails_to_open() {
        let crypto = test_crypto();
        let sealed = crypto.seal(b"secret", b"user-1");
        assert!(crypto.open(&sealed, b"user-2").is_err());
    }

    #[test]
    fn legacy_plaintext_passes_through_for_migration() {
        let crypto = test_crypto();
        let opened = crypto
            .open(b"legacy-plaintext", b"aad")
            .expect("passes through");
        assert_eq!(opened.plaintext, b"legacy-plaintext");
        assert!(!opened.was_sealed);
    }

    #[test]
    fn debug_redacts_key() {
        let rendered = format!("{:?}", test_crypto());
        assert_eq!(rendered, "Crypto(<redacted>)");
    }

    #[test]
    fn wrong_key_length_is_rejected() {
        assert!(Crypto::new(&[0u8; 16]).is_err());
    }
}
