//! RS256 signing keys: generation, rotation, JWKS publication, and JWT signing.
//!
//! Elysium signs every ID and access token with an RSA-2048 private key and
//! publishes the public halves at the JWKS endpoint, so any OIDC client verifies a
//! token offline without calling back. The signing keys are the root of trust for
//! the whole provider, so their private material is sealed at rest under
//! [`crate::crypto`] exactly like the OPAQUE setup and TOTP secrets: a database
//! dump alone never yields a usable signing key.
//!
//! Exactly one key is *active* and signs new tokens; the JWKS publishes every
//! *non-retired* key. Rotation ([`rotate`]) generates a new active key and flips
//! the prior one inactive-but-not-retired, so a token signed moments before the
//! rotation still verifies against the published set. Retiring a key later drops it
//! from the JWKS and ends its ability to verify. The active key is loaded from the
//! database per signing request rather than cached, so a rotation takes effect on
//! the next token with no process-wide state to invalidate.

use std::fmt::{self, Write as _};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use eyre::{eyre, WrapErr};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header};
use rand::rngs::OsRng;
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::crypto::Crypto;

/// The only signing algorithm Elysium issues today (the DB CHECK matches).
pub const ALGORITHM: &str = "RS256";

/// RSA modulus size, in bits. 2048 is the OIDC-conformant floor and what every
/// client (Forgejo included) expects; larger only slows signing with no gain here.
const RSA_BITS: usize = 2048;

/// A freshly generated signing keypair, ready to seal and persist.
///
/// `private_pem` is the sensitive half (PKCS#8 PEM); its custom [`fmt::Debug`]
/// keeps it out of logs. `public_jwk` is the JWK object served verbatim by JWKS.
pub struct GeneratedKey {
    pub kid: String,
    pub private_pem: String,
    pub public_jwk: Value,
}

// Redacts the private half: a stray log line must never carry key material.
impl fmt::Debug for GeneratedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeneratedKey")
            .field("kid", &self.kid)
            .field("private_pem", &"<redacted>")
            .field("public_jwk", &self.public_jwk)
            .finish()
    }
}

/// The active key resolved for signing: its `kid` and the encoding key.
///
/// The `kid` is written into the JWT header so a verifier selects the matching
/// public key from the JWKS.
pub struct Signer {
    pub kid: String,
    pub encoding: EncodingKey,
}

/// Generates an RSA-2048 keypair and derives its public JWK.
///
/// The `kid` is a fresh UUID, unique per key so a rotated set never collides. The
/// private half is returned as PKCS#8 PEM (the form [`jsonwebtoken`] signs with),
/// and the JWK carries the base64url `n`/`e` derived from the public key.
///
/// # Errors
/// Returns an error only if RSA key generation or PEM encoding fails, which does
/// not happen for an in-memory 2048-bit key.
pub fn generate_key() -> eyre::Result<GeneratedKey> {
    let private = RsaPrivateKey::new(&mut OsRng, RSA_BITS)
        .wrap_err("failed to generate an RSA signing key")?;
    let private_pem = private
        .to_pkcs8_pem(LineEnding::LF)
        .wrap_err("failed to encode the signing key as PKCS#8 PEM")?
        .to_string();
    let kid = Uuid::new_v4().to_string();
    let public_jwk = derive_jwk(&kid, &RsaPublicKey::from(&private));
    Ok(GeneratedKey {
        kid,
        private_pem,
        public_jwk,
    })
}

/// Builds the public JWK object for `public`, tagged with `kid`.
///
/// The RSA modulus (`n`) and exponent (`e`) are big-endian, base64url without
/// padding, per RFC 7518. `use: "sig"` and `alg: "RS256"` tell a client this key
/// verifies signatures.
fn derive_jwk(kid: &str, public: &RsaPublicKey) -> Value {
    json!({
        "kty": "RSA",
        "use": "sig",
        "alg": ALGORITHM,
        "kid": kid,
        "n": URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
        "e": URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
    })
}

/// Ensures an active signing key exists, generating the first one on demand.
///
/// Idempotent and safe to run on every boot (mirrors `ensure_default_drives`): if
/// an active key is already present it is a no-op; otherwise it generates, seals,
/// and stores one, logging that it did. A concurrent first boot is resolved by the
/// single-active partial unique index, so at most one key wins the race.
///
/// # Errors
/// Returns an error if the database is unreachable or key generation fails.
pub async fn ensure_active_key(pool: &PgPool, crypto: &Crypto) -> eyre::Result<()> {
    let has_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM oidc_signing_keys WHERE is_active)",
    )
    .fetch_one(pool)
    .await
    .wrap_err("failed to check for an active OIDC signing key")?;
    if has_active {
        return Ok(());
    }

    let key = generate_key()?;
    // Seal the private PEM bound to its kid, so a ciphertext cannot be replayed
    // under a different key's identity.
    let sealed = crypto.seal(key.private_pem.as_bytes(), key.kid.as_bytes());
    let inserted = sqlx::query(
        "INSERT INTO oidc_signing_keys (kid, algorithm, private_key, public_jwk, is_active)
         VALUES ($1, $2, $3, $4, TRUE)
         ON CONFLICT DO NOTHING",
    )
    .bind(&key.kid)
    .bind(ALGORITHM)
    .bind(&sealed)
    .bind(&key.public_jwk)
    .execute(pool)
    .await
    .wrap_err("failed to store the initial OIDC signing key")?;

    if inserted.rows_affected() == 1 {
        tracing::info!(kid = %key.kid, "generated the initial OIDC signing key");
    } else {
        // Another booting instance won the race; its key is the active one.
        tracing::info!("an OIDC signing key already existed; skipped generation");
    }
    Ok(())
}

/// Loads the active signing key and builds an encoding key ready to sign with.
///
/// The private PEM is unsealed (bound to its `kid`) and handed to [`jsonwebtoken`].
///
/// # Errors
/// Returns an error if there is no active key, the database is unreachable, or the
/// stored key cannot be opened (wrong master key or tampering).
pub async fn active_signer(pool: &PgPool, crypto: &Crypto) -> eyre::Result<Signer> {
    let row = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT kid, private_key FROM oidc_signing_keys WHERE is_active",
    )
    .fetch_optional(pool)
    .await
    .wrap_err("failed to load the active OIDC signing key")?
    .ok_or_else(|| eyre!("no active OIDC signing key exists"))?;

    let (kid, sealed) = row;
    let opened = crypto
        .open(&sealed, kid.as_bytes())
        .wrap_err("failed to open the active OIDC signing key")?;
    let encoding = EncodingKey::from_rsa_pem(&opened.plaintext)
        .wrap_err("stored OIDC signing key is not a valid RSA PEM")?;
    Ok(Signer { kid, encoding })
}

/// Signs `claims` with the active key, stamping its `kid` into the JWT header.
///
/// # Errors
/// Returns an error if no active key is available or the JWT cannot be encoded.
pub async fn sign<C: Serialize>(
    pool: &PgPool,
    crypto: &Crypto,
    claims: &C,
) -> eyre::Result<String> {
    let signer = active_signer(pool, crypto).await?;
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(signer.kid);
    jsonwebtoken::encode(&header, claims, &signer.encoding).wrap_err("failed to sign a JWT")
}

/// Builds the JWKS document from every non-retired public key.
///
/// A retired key is dropped, ending its ability to verify; every other key stays
/// published so a token signed just before a rotation still validates.
///
/// # Errors
/// Returns an error if the database is unreachable.
pub async fn jwks(pool: &PgPool) -> eyre::Result<Value> {
    let rows = sqlx::query_scalar::<_, Value>(
        "SELECT public_jwk FROM oidc_signing_keys WHERE retired_at IS NULL ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await
    .wrap_err("failed to load the OIDC public keys")?;
    Ok(json!({ "keys": rows }))
}

/// Builds a decoding key for `kid`, if a non-retired key with that id exists.
///
/// Used to verify a presented access token at the userinfo endpoint: the token's
/// `kid` header selects the key, and its stored JWK `n`/`e` build the verifier.
///
/// # Errors
/// Returns an error if the database is unreachable or the stored JWK is malformed.
pub async fn verifier_for_kid(pool: &PgPool, kid: &str) -> eyre::Result<Option<DecodingKey>> {
    let jwk = sqlx::query_scalar::<_, Value>(
        "SELECT public_jwk FROM oidc_signing_keys WHERE kid = $1 AND retired_at IS NULL",
    )
    .bind(kid)
    .fetch_optional(pool)
    .await
    .wrap_err("failed to load an OIDC public key by kid")?;

    let Some(jwk) = jwk else {
        return Ok(None);
    };
    let n = jwk
        .get("n")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("stored JWK is missing its modulus"))?;
    let e = jwk
        .get("e")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("stored JWK is missing its exponent"))?;
    let key = DecodingKey::from_rsa_components(n, e)
        .wrap_err("stored JWK components are not a valid RSA key")?;
    Ok(Some(key))
}

/// Rotates the signing key: a new active key signs, the prior stays a verifier.
///
/// In one transaction the current active key is flipped inactive (still non-retired,
/// so JWKS keeps it and it still verifies) and a new sealed active key is inserted.
/// The single-active index makes the ordering matter: demote first, then insert.
///
/// # Errors
/// Returns an error if key generation or any statement fails; the transaction rolls
/// back so the previously active key stays active.
// The rotation capability lands ahead of its HTTP consumer: the issue requires key
// rotation to be supported (and the DB-gated test exercises it), while an admin
// "rotate now" endpoint is a later addition. Mirrors `transport`'s seam-ahead-of-
// consumer allowance.
#[allow(
    dead_code,
    reason = "key-rotation seam; exercised by the DB-gated test, admin endpoint lands later"
)]
pub async fn rotate(pool: &PgPool, crypto: &Crypto) -> eyre::Result<String> {
    let key = generate_key()?;
    let sealed = crypto.seal(key.private_pem.as_bytes(), key.kid.as_bytes());

    let mut tx = pool
        .begin()
        .await
        .wrap_err("failed to begin key rotation")?;
    sqlx::query("UPDATE oidc_signing_keys SET is_active = FALSE WHERE is_active")
        .execute(&mut *tx)
        .await
        .wrap_err("failed to demote the prior active signing key")?;
    sqlx::query(
        "INSERT INTO oidc_signing_keys (kid, algorithm, private_key, public_jwk, is_active)
         VALUES ($1, $2, $3, $4, TRUE)",
    )
    .bind(&key.kid)
    .bind(ALGORITHM)
    .bind(&sealed)
    .bind(&key.public_jwk)
    .execute(&mut *tx)
    .await
    .wrap_err("failed to insert the new active signing key")?;
    tx.commit()
        .await
        .wrap_err("failed to commit key rotation")?;

    tracing::info!(kid = %key.kid, "rotated the active OIDC signing key");
    Ok(key.kid)
}

/// Lowercase-hex SHA-256 of `input`, the one-way hash used for codes and tokens.
///
/// A code or refresh token is a bearer credential, only ever compared, so its hash
/// is stored and the presented value is hashed and looked up (mirrors
/// [`crate::transport`]). Lives here so both the code and token modules share it.
pub fn hash_secret(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(input.as_bytes());
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

    use jsonwebtoken::{decode, DecodingKey, Validation};
    use serde::Deserialize;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct SampleClaims {
        sub: String,
        exp: usize,
    }

    /// Builds a decoding key from a JWK's `n`/`e`, as a real client would.
    fn decoding_key(jwk: &Value) -> DecodingKey {
        let n = jwk["n"].as_str().expect("jwk has a modulus");
        let e = jwk["e"].as_str().expect("jwk has an exponent");
        DecodingKey::from_rsa_components(n, e).expect("valid RSA components")
    }

    #[test]
    fn generated_jwk_has_the_expected_shape() {
        let key = generate_key().expect("key generation succeeds");
        let jwk = &key.public_jwk;
        assert_eq!(jwk["kty"], "RSA");
        assert_eq!(jwk["use"], "sig");
        assert_eq!(jwk["alg"], "RS256");
        assert_eq!(jwk["kid"], key.kid);
        // Modulus and exponent are present and base64url-decodable.
        let n = jwk["n"].as_str().expect("modulus present");
        let e = jwk["e"].as_str().expect("exponent present");
        assert!(URL_SAFE_NO_PAD.decode(n).is_ok());
        assert!(URL_SAFE_NO_PAD.decode(e).is_ok());
        // A 2048-bit modulus is 256 bytes big-endian.
        assert_eq!(URL_SAFE_NO_PAD.decode(n).expect("decodes").len(), 256);
    }

    #[test]
    fn each_generated_key_has_a_unique_kid() {
        let a = generate_key().expect("first key");
        let b = generate_key().expect("second key");
        assert_ne!(a.kid, b.kid);
    }

    #[test]
    fn signed_jwt_verifies_against_the_published_jwk() {
        let key = generate_key().expect("key generation succeeds");
        let encoding = EncodingKey::from_rsa_pem(key.private_pem.as_bytes()).expect("valid PEM");

        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(key.kid.clone());
        let claims = SampleClaims {
            sub: "user-1".to_string(),
            // Far-future expiry so validation does not reject on time.
            exp: 9_999_999_999,
        };
        let token = jsonwebtoken::encode(&header, &claims, &encoding).expect("signs");

        // The header carries the kid a verifier uses to select the key.
        let header = jsonwebtoken::decode_header(&token).expect("header parses");
        assert_eq!(header.kid.as_deref(), Some(key.kid.as_str()));

        // Verifying against the JWK's public components round-trips the claims.
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_aud = false;
        let decoded = decode::<SampleClaims>(&token, &decoding_key(&key.public_jwk), &validation)
            .expect("verifies against the published key");
        assert_eq!(decoded.claims, claims);
    }

    #[test]
    fn a_token_does_not_verify_against_a_different_key() {
        let signing = generate_key().expect("signing key");
        let other = generate_key().expect("unrelated key");
        let encoding =
            EncodingKey::from_rsa_pem(signing.private_pem.as_bytes()).expect("valid PEM");

        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(signing.kid.clone());
        let claims = SampleClaims {
            sub: "user-1".to_string(),
            exp: 9_999_999_999,
        };
        let token = jsonwebtoken::encode(&header, &claims, &encoding).expect("signs");

        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_aud = false;
        // The wrong key must reject the signature.
        assert!(
            decode::<SampleClaims>(&token, &decoding_key(&other.public_jwk), &validation).is_err()
        );
    }

    #[test]
    fn generated_key_debug_redacts_the_private_pem() {
        let key = generate_key().expect("key generation succeeds");
        let rendered = format!("{key:?}");
        assert!(!rendered.contains(&key.private_pem));
        assert!(rendered.contains("<redacted>"));
        // The kid stays visible for debugging.
        assert!(rendered.contains(&key.kid));
    }

    #[test]
    fn hash_secret_matches_a_known_sha256_vector() {
        // SHA-256("abc"); proves we hash the plaintext, lowercase hex.
        assert_eq!(
            hash_secret("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash_secret("x").len(), 64);
    }

    // --- DB-gated rotation ---------------------------------------------------
    //
    // Needs a reachable postgres; ignored by default, run in CI with
    // `cargo test -- --ignored`. This test is the sole toucher of the global
    // `oidc_signing_keys` table, so it may clear and assert absolute state.

    async fn connect_test_pool() -> PgPool {
        use sqlx::postgres::PgPoolOptions;
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");
        pool
    }

    /// The set of `kid`s currently published by JWKS.
    fn published_kids(document: &Value) -> Vec<String> {
        document["keys"]
            .as_array()
            .expect("keys is an array")
            .iter()
            .map(|key| key["kid"].as_str().expect("each key has a kid").to_string())
            .collect()
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn rotation_signs_with_the_new_key_and_keeps_the_old_verifying() {
        let pool = connect_test_pool().await;
        let crypto = Crypto::new(&[5u8; 32]).expect("32-byte key");
        // Sole toucher of the table: start clean so counts are deterministic.
        sqlx::query("DELETE FROM oidc_signing_keys")
            .execute(&pool)
            .await
            .expect("clear keys");

        // The initial key is generated once and is the sole active signer.
        ensure_active_key(&pool, &crypto).await.expect("ensure key");
        let first = active_signer(&pool, &crypto).await.expect("active signer");
        // Idempotent: a second ensure does not add a key.
        ensure_active_key(&pool, &crypto).await.expect("re-ensure");
        assert_eq!(published_kids(&jwks(&pool).await.expect("jwks")).len(), 1);

        // Rotation adds a new active key; the old one demotes but stays published.
        let second_kid = rotate(&pool, &crypto).await.expect("rotate");
        let second = active_signer(&pool, &crypto).await.expect("active signer");
        assert_ne!(first.kid, second.kid);
        assert_eq!(second.kid, second_kid);

        let kids = published_kids(&jwks(&pool).await.expect("jwks"));
        assert!(kids.contains(&first.kid), "old key still published");
        assert!(kids.contains(&second.kid), "new key published");
        // The old key still verifies (a token signed before rotation is honored).
        assert!(verifier_for_kid(&pool, &first.kid)
            .await
            .expect("lookup")
            .is_some());

        // Retiring the old key drops it from JWKS and ends its verification.
        sqlx::query("UPDATE oidc_signing_keys SET retired_at = now() WHERE kid = $1")
            .bind(&first.kid)
            .execute(&pool)
            .await
            .expect("retire old key");
        let kids = published_kids(&jwks(&pool).await.expect("jwks"));
        assert!(!kids.contains(&first.kid), "retired key drops from JWKS");
        assert!(kids.contains(&second.kid), "active key remains");
        assert!(verifier_for_kid(&pool, &first.kid)
            .await
            .expect("lookup")
            .is_none());
    }
}
