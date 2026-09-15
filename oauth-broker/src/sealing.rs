// Copyright © 2026 Jalapeno Labs

//! Sealed, expiring tokens: how the broker carries state without storing any.
//!
//! Two values have to survive a trip through someone's browser: the `state` the
//! broker hands a provider, and the handoff code it hands an Elysium instance. Both
//! are sealed here with XChaCha20-Poly1305 under the broker's own key, so the broker
//! needs no database, no cache, and no sticky sessions. Any replica holding the key
//! can open a token any other replica sealed.
//!
//! A token is `base64url(nonce || ciphertext || tag)` over a JSON body that carries
//! its own expiry. The [`Purpose`] is authenticated as associated data, so a state
//! token cannot be replayed as a handoff code or the other way around.
//!
//! What sealing cannot give is single use: a stateless broker cannot remember that a
//! token was already redeemed. Handoff codes are therefore short-lived and bound to a
//! PKCE challenge, so a copied code is useless without the verifier that only the
//! instance's backend holds.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD as BASE64_URL};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// XChaCha20-Poly1305 takes exactly a 256-bit key.
const KEY_BYTES: usize = 32;

/// `XChaCha20`'s extended nonce. Random 192-bit nonces make reuse a non-concern.
const NONCE_BYTES: usize = 24;

/// What a sealed token is for. Authenticated, never stored in the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The `state` round-tripped through a provider's consent screen.
    ProviderState,
    /// The code an Elysium instance redeems for a refresh token.
    Handoff,
}

impl Purpose {
    /// Versioned so a future change to either body can refuse old tokens outright.
    const fn associated_data(self) -> &'static [u8] {
        match self {
            Self::ProviderState => b"elysium-oauth-broker/provider-state/v1",
            Self::Handoff => b"elysium-oauth-broker/handoff/v1",
        }
    }
}

/// Why the sealing key was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("sealing key is not valid base64")]
    NotBase64,
    #[error("sealing key decodes to {0} bytes, expected {KEY_BYTES}")]
    WrongLength(usize),
}

/// Why a token could not be opened.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OpenError {
    /// Not base64url, too short, or not the JSON body this purpose expects.
    #[error("token is malformed")]
    Malformed,
    /// Sealed under another key or purpose, or altered.
    #[error("token is not authentic")]
    Inauthentic,
    #[error("token has expired")]
    Expired,
}

/// The JSON actually sealed: the caller's payload plus its expiry.
#[derive(Serialize, Deserialize)]
struct Body<Payload> {
    /// Unix seconds.
    expires_at: u64,
    payload: Payload,
}

/// Seals and opens tokens under one key.
#[derive(Clone)]
pub struct Sealer {
    aead: XChaCha20Poly1305,
}

impl std::fmt::Debug for Sealer {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Sealer(..)")
    }
}

impl Sealer {
    /// Builds a sealer from a base64-encoded 32-byte key.
    ///
    /// # Errors
    /// Returns [`KeyError`] when the text is not base64 or not exactly 32 bytes.
    pub fn from_base64_key(encoded: &SecretString) -> Result<Self, KeyError> {
        let key_bytes = BASE64
            .decode(encoded.expose_secret().trim())
            .map_err(|_decode_error| KeyError::NotBase64)?;
        let key: [u8; KEY_BYTES] = key_bytes
            .as_slice()
            .try_into()
            .map_err(|_length_error| KeyError::WrongLength(key_bytes.len()))?;

        Ok(Self {
            aead: XChaCha20Poly1305::new(&key.into()),
        })
    }

    /// Seals `payload` for `purpose`, valid for `lifetime` from `now`.
    ///
    /// # Panics
    /// Panics if the operating system cannot supply random bytes for the nonce, or if
    /// `now` is before 1970; neither can happen on a working host.
    pub fn seal<Content: Serialize>(
        &self,
        purpose: Purpose,
        payload: &Content,
        lifetime: Duration,
        now: SystemTime,
    ) -> String {
        let expires_at = (now + lifetime)
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs();
        let body = serde_json::to_vec(&Body {
            expires_at,
            payload,
        })
        .expect("token payloads are plain data and serialize");

        let mut nonce = [0u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).expect("the OS random number generator is available");
        let ciphertext = self
            .aead
            .encrypt(
                &nonce.into(),
                Payload {
                    msg: &body,
                    aad: purpose.associated_data(),
                },
            )
            .expect("XChaCha20-Poly1305 accepts any plaintext that fits in memory");

        let mut token = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
        token.extend_from_slice(&nonce);
        token.extend_from_slice(&ciphertext);
        BASE64_URL.encode(token)
    }

    /// Opens a token sealed for `purpose` and checks it has not expired at `now`.
    ///
    /// # Errors
    /// Returns [`OpenError`] for a malformed, inauthentic, or expired token.
    pub fn open<Content: DeserializeOwned>(
        &self,
        purpose: Purpose,
        token: &str,
        now: SystemTime,
    ) -> Result<Content, OpenError> {
        let bytes = BASE64_URL
            .decode(token)
            .map_err(|_decode_error| OpenError::Malformed)?;
        if bytes.len() <= NONCE_BYTES {
            return Err(OpenError::Malformed);
        }

        let (nonce, ciphertext) = bytes.split_at(NONCE_BYTES);
        let nonce: [u8; NONCE_BYTES] = nonce.try_into().expect("split_at yields NONCE_BYTES");
        let plaintext = self
            .aead
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: purpose.associated_data(),
                },
            )
            .map_err(|_aead_error| OpenError::Inauthentic)?;

        let body: Body<Content> =
            serde_json::from_slice(&plaintext).map_err(|_json_error| OpenError::Malformed)?;
        let now_seconds = now
            .duration_since(UNIX_EPOCH)
            .map_err(|_clock_error| OpenError::Expired)?
            .as_secs();
        if now_seconds >= body.expires_at {
            return Err(OpenError::Expired);
        }

        Ok(body.payload)
    }
}

/// A fresh random key, base64-encoded, for `BROKER_SEALING_KEY`.
///
/// # Panics
/// Panics if the operating system cannot supply random bytes.
pub fn generate_key() -> String {
    let mut key = [0u8; KEY_BYTES];
    getrandom::fill(&mut key).expect("the OS random number generator is available");
    BASE64.encode(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        address: String,
    }

    fn sealer() -> Sealer {
        Sealer::from_base64_key(&SecretString::from(generate_key()))
            .expect("generated keys are valid")
    }

    fn sample() -> Sample {
        Sample {
            address: "someone@example.com".to_owned(),
        }
    }

    #[test]
    fn a_sealed_token_opens_back_to_its_payload() {
        let sealer = sealer();
        let now = SystemTime::now();
        let token = sealer.seal(Purpose::Handoff, &sample(), Duration::from_secs(60), now);

        let opened: Sample = sealer.open(Purpose::Handoff, &token, now).expect("opens");
        assert_eq!(opened, sample());
        assert!(
            !token.contains("someone"),
            "the payload is not readable in the token"
        );
    }

    #[test]
    fn a_token_cannot_be_used_for_another_purpose() {
        let sealer = sealer();
        let now = SystemTime::now();
        let state = sealer.seal(
            Purpose::ProviderState,
            &sample(),
            Duration::from_secs(60),
            now,
        );

        let result: Result<Sample, _> = sealer.open(Purpose::Handoff, &state, now);
        assert_eq!(result.unwrap_err(), OpenError::Inauthentic);
    }

    #[test]
    fn a_token_expires_at_the_end_of_its_lifetime() {
        let sealer = sealer();
        let now = SystemTime::now();
        let token = sealer.seal(Purpose::Handoff, &sample(), Duration::from_secs(60), now);

        let later = now + Duration::from_secs(61);
        let result: Result<Sample, _> = sealer.open(Purpose::Handoff, &token, later);
        assert_eq!(result.unwrap_err(), OpenError::Expired);
    }

    #[test]
    fn another_key_or_an_altered_byte_is_refused() {
        let now = SystemTime::now();
        let token = sealer().seal(Purpose::Handoff, &sample(), Duration::from_secs(60), now);

        let foreign: Result<Sample, _> = sealer().open(Purpose::Handoff, &token, now);
        assert_eq!(foreign.unwrap_err(), OpenError::Inauthentic);

        let sealer = sealer();
        let token = sealer.seal(Purpose::Handoff, &sample(), Duration::from_secs(60), now);
        let mut bytes = BASE64_URL.decode(&token).expect("base64url");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let altered: Result<Sample, _> =
            sealer.open(Purpose::Handoff, &BASE64_URL.encode(bytes), now);
        assert_eq!(altered.unwrap_err(), OpenError::Inauthentic);
    }

    #[test]
    fn garbage_is_malformed_rather_than_a_panic() {
        let now = SystemTime::now();
        for garbage in ["", "not base64!", "AAAA"] {
            let result: Result<Sample, _> = sealer().open(Purpose::Handoff, garbage, now);
            assert_eq!(result.unwrap_err(), OpenError::Malformed, "{garbage:?}");
        }
    }

    #[test]
    fn key_parsing_rejects_bad_input() {
        assert_eq!(
            Sealer::from_base64_key(&SecretString::from("definitely not base64!")).unwrap_err(),
            KeyError::NotBase64
        );
        assert_eq!(
            Sealer::from_base64_key(&SecretString::from(BASE64.encode([7u8; 16]))).unwrap_err(),
            KeyError::WrongLength(16)
        );
    }
}
