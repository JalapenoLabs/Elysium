// Copyright © 2026 Jalapeno Labs

//! Authenticated encryption for secrets stored in Postgres.
//!
//! Secrets are sealed with XChaCha20-Poly1305 under the 32-byte key from
//! `ELYSIUM_ENCRYPTION_KEY`. Each sealed value is a self-describing envelope:
//!
//! ```text
//! version (1 byte) || nonce (24 bytes) || ciphertext || tag (16 bytes)
//! ```
//!
//! Every seal also takes a *context*, authenticated but not stored, that binds the
//! ciphertext to where it lives (for example the owning row's id). Copying a sealed
//! value into another row therefore fails to open instead of silently leaking the
//! wrong credential. The version byte leaves room for key rotation without a
//! flag-day migration.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use secrecy::{ExposeSecret, SecretSlice, SecretString};
use zeroize::Zeroizing;

/// Required key length. XChaCha20-Poly1305 takes exactly a 256-bit key.
pub const ENCRYPTION_KEY_BYTES: usize = 32;

/// Envelope format this build writes. Bump alongside a new key or algorithm.
const ENVELOPE_VERSION: u8 = 1;

/// `XChaCha20`'s extended nonce. Random 192-bit nonces make reuse a non-concern.
const NONCE_BYTES: usize = 24;

/// Poly1305 authentication tag appended by the AEAD.
const TAG_BYTES: usize = 16;

/// The smallest valid envelope: an empty plaintext. Mirrored by the
/// `llms_secret_token_sealed` check constraint.
pub const MIN_ENVELOPE_BYTES: usize = 1 + NONCE_BYTES + TAG_BYTES;

/// Why an encryption key was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("encryption key is not valid base64")]
    NotBase64,
    #[error("encryption key decodes to {0} bytes, expected {ENCRYPTION_KEY_BYTES}")]
    WrongLength(usize),
}

/// Why a sealed value could not be opened.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OpenError {
    #[error("sealed value is shorter than the smallest possible envelope")]
    Truncated,
    #[error("sealed value uses unsupported envelope version {0}")]
    UnsupportedVersion(u8),
    /// Wrong key, wrong context, or tampered bytes. Deliberately indistinguishable.
    #[error("sealed value failed authentication")]
    Inauthentic,
}

/// Seals and opens secrets under one key.
#[derive(Clone)]
pub struct Cipher {
    aead: XChaCha20Poly1305,
}

impl std::fmt::Debug for Cipher {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Cipher(..)")
    }
}

impl Cipher {
    /// Builds a cipher from a base64-encoded 32-byte key.
    ///
    /// # Errors
    /// Returns [`KeyError`] when the text is not base64 or not exactly 32 bytes.
    pub fn from_base64_key(encoded: &SecretString) -> Result<Self, KeyError> {
        let key_bytes = Zeroizing::new(
            BASE64
                .decode(encoded.expose_secret().trim())
                .map_err(|_decode_error| KeyError::NotBase64)?,
        );

        let key: &[u8; ENCRYPTION_KEY_BYTES] = key_bytes
            .as_slice()
            .try_into()
            .map_err(|_length_error| KeyError::WrongLength(key_bytes.len()))?;

        Ok(Self {
            aead: XChaCha20Poly1305::new(&(*key).into()),
        })
    }

    /// Encrypts `plaintext`, binding it to `context`.
    ///
    /// # Panics
    /// Panics if the operating system cannot supply random bytes for the nonce;
    /// sealing without a fresh nonce would be unsafe, so there is no fallback.
    pub fn seal(&self, plaintext: &[u8], context: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).expect("the OS random number generator is available");

        let ciphertext = self
            .aead
            .encrypt(
                &nonce.into(),
                Payload {
                    msg: plaintext,
                    aad: context,
                },
            )
            .expect("XChaCha20-Poly1305 accepts any plaintext that fits in memory");

        let mut envelope = Vec::with_capacity(1 + NONCE_BYTES + ciphertext.len());
        envelope.push(ENVELOPE_VERSION);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        envelope
    }

    /// Decrypts an envelope produced by [`Cipher::seal`] with the same `context`.
    ///
    /// # Errors
    /// Returns [`OpenError`] for short input, an unknown version, or any
    /// authentication failure (wrong key, wrong context, altered bytes).
    pub fn open(&self, envelope: &[u8], context: &[u8]) -> Result<SecretSlice<u8>, OpenError> {
        if envelope.len() < MIN_ENVELOPE_BYTES {
            return Err(OpenError::Truncated);
        }

        let (&version, rest) = envelope.split_first().ok_or(OpenError::Truncated)?;
        if version != ENVELOPE_VERSION {
            return Err(OpenError::UnsupportedVersion(version));
        }

        let (nonce, ciphertext) = rest.split_at(NONCE_BYTES);
        let nonce: [u8; NONCE_BYTES] = nonce
            .try_into()
            .expect("split_at yields exactly NONCE_BYTES");

        let plaintext = self
            .aead
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: context,
                },
            )
            .map_err(|_aead_error| OpenError::Inauthentic)?;

        Ok(plaintext.into())
    }
}

/// Produces a fresh random key, base64-encoded, for `ELYSIUM_ENCRYPTION_KEY`.
///
/// # Panics
/// Panics if the operating system cannot supply random bytes.
pub fn generate_key() -> String {
    let mut key = Zeroizing::new([0u8; ENCRYPTION_KEY_BYTES]);
    getrandom::fill(key.as_mut_slice()).expect("the OS random number generator is available");
    BASE64.encode(key.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> Cipher {
        Cipher::from_base64_key(&SecretString::from(generate_key()))
            .expect("generated keys are valid")
    }

    #[test]
    fn seal_then_open_returns_the_plaintext() {
        let cipher = cipher();
        let envelope = cipher.seal(b"sk-ant-secret", b"llms.secret_token:1");

        let opened = cipher
            .open(&envelope, b"llms.secret_token:1")
            .expect("same key and context open");
        assert_eq!(opened.expose_secret(), b"sk-ant-secret");
    }

    #[test]
    fn envelope_never_contains_the_plaintext() {
        let envelope = cipher().seal(b"sk-ant-secret", b"context");
        assert!(
            !envelope
                .windows(b"sk-ant-secret".len())
                .any(|window| window == b"sk-ant-secret")
        );
        assert_eq!(envelope.len(), MIN_ENVELOPE_BYTES + b"sk-ant-secret".len());
        assert_eq!(envelope[0], ENVELOPE_VERSION);
    }

    #[test]
    fn sealing_twice_uses_distinct_nonces() {
        let cipher = cipher();
        assert_ne!(
            cipher.seal(b"same", b"context"),
            cipher.seal(b"same", b"context")
        );
    }

    #[test]
    fn empty_plaintext_round_trips() {
        let cipher = cipher();
        let envelope = cipher.seal(b"", b"context");
        assert_eq!(envelope.len(), MIN_ENVELOPE_BYTES);
        assert!(
            cipher
                .open(&envelope, b"context")
                .expect("empty plaintext opens")
                .expose_secret()
                .is_empty()
        );
    }

    #[test]
    fn open_rejects_a_different_context() {
        let cipher = cipher();
        let envelope = cipher.seal(b"secret", b"llms.secret_token:row-a");
        assert_eq!(
            cipher
                .open(&envelope, b"llms.secret_token:row-b")
                .unwrap_err(),
            OpenError::Inauthentic
        );
    }

    #[test]
    fn open_rejects_a_different_key() {
        let envelope = cipher().seal(b"secret", b"context");
        assert_eq!(
            cipher().open(&envelope, b"context").unwrap_err(),
            OpenError::Inauthentic
        );
    }

    #[test]
    fn open_rejects_any_flipped_byte() {
        let cipher = cipher();
        let envelope = cipher.seal(b"secret", b"context");

        // Byte 0 is the version, which fails earlier with its own error.
        for index in 1..envelope.len() {
            let mut tampered = envelope.clone();
            tampered[index] ^= 0x01;
            assert_eq!(
                cipher.open(&tampered, b"context").unwrap_err(),
                OpenError::Inauthentic,
                "byte {index}"
            );
        }
    }

    #[test]
    fn open_rejects_unknown_versions_and_short_input() {
        let cipher = cipher();
        let mut envelope = cipher.seal(b"secret", b"context");

        assert_eq!(
            cipher
                .open(&envelope[..MIN_ENVELOPE_BYTES - 1], b"context")
                .unwrap_err(),
            OpenError::Truncated
        );

        envelope[0] = ENVELOPE_VERSION + 1;
        assert_eq!(
            cipher.open(&envelope, b"context").unwrap_err(),
            OpenError::UnsupportedVersion(ENVELOPE_VERSION + 1)
        );
    }

    #[test]
    fn key_parsing_rejects_bad_input() {
        let not_base64 = SecretString::from("definitely not base64!");
        assert_eq!(
            Cipher::from_base64_key(&not_base64).unwrap_err(),
            KeyError::NotBase64
        );

        let too_short = SecretString::from(BASE64.encode([7u8; 16]));
        assert_eq!(
            Cipher::from_base64_key(&too_short).unwrap_err(),
            KeyError::WrongLength(16)
        );
    }

    #[test]
    fn key_parsing_tolerates_surrounding_whitespace() {
        let padded = SecretString::from(format!("  {}\n", generate_key()));
        Cipher::from_base64_key(&padded).expect("whitespace around the key is ignored");
    }

    #[test]
    fn debug_output_never_reveals_key_material() {
        let key = generate_key();
        let cipher = Cipher::from_base64_key(&SecretString::from(key.clone())).expect("valid key");

        let rendered = format!("{cipher:?}");
        assert_eq!(rendered, "Cipher(..)");
        assert!(!rendered.contains(&key));
    }
}
