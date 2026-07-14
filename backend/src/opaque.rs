//! OPAQUE aPAKE primitives for password authentication (RFC 9807).
//!
//! This module is the server half of the OPAQUE protocol, kept free of HTTP so
//! the handlers in [`crate::password`] and [`crate::bootstrap`] stay thin. The
//! server never learns the password or any replayable hash of it: registration
//! stores an opaque record, and login is a two-message key exchange that the
//! server can complete only for the record the client actually registered.
//!
//! The cipher suite (ristretto255 OPRF, triple-DH over SHA-512, Argon2 KSF) is
//! chosen to match `@serenity-kit/opaque` on the client, so the messages defined
//! by that library deserialize here and vice versa. All wire values in this
//! module are base64url without padding, the encoding that library emits.
//!
//! The key-stretching function runs client-side; the server's copy of the suite
//! is a type-level match only and never executes Argon2.

use argon2::Argon2;
// `@serenity-kit/opaque` encodes every wire message as base64url without padding,
// so the server must use the same alphabet to interop.
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use eyre::{eyre, WrapErr};
use opaque_ke::{
    CipherSuite, CredentialFinalization, CredentialRequest, RegistrationRequest,
    RegistrationUpload, Ristretto255, ServerLogin, ServerLoginParameters, ServerRegistration,
    ServerSetup, TripleDh,
};
use rand::rngs::OsRng;
use sha2::Sha512;
use sqlx::PgPool;

use crate::crypto::Crypto;

/// Associated data binding the sealed OPAQUE setup to its purpose.
const SETUP_AAD: &[u8] = b"opaque_server_setup";

/// Elysium's OPAQUE cipher suite; mirrors `@serenity-kit/opaque`'s default suite.
pub struct Suite;

impl CipherSuite for Suite {
    type OprfCs = Ristretto255;
    type KeyExchange = TripleDh<Ristretto255, Sha512>;
    type Ksf = Argon2<'static>;
}

/// Loads the persisted server setup, creating and storing it on first run.
///
/// The setup is a process-wide singleton (one row in `opaque_server_setup`);
/// regenerating it would invalidate every stored password record, so it is
/// created exactly once. A concurrent first-boot race is resolved by inserting
/// with `ON CONFLICT DO NOTHING` and then reading the winning row back.
///
/// It is sealed at rest under `crypto` (see [`crate::crypto`]); a row written
/// before encryption existed is read as legacy plaintext and resealed in place, so
/// the setup bytes are preserved exactly and every stored password record keeps
/// working.
///
/// # Errors
/// Returns an error if the database is unreachable, the stored setup cannot be
/// decrypted (wrong key or tampering), or it cannot be deserialized (corruption or
/// a suite change).
pub async fn load_or_create_setup(
    pool: &PgPool,
    crypto: &Crypto,
) -> eyre::Result<ServerSetup<Suite>> {
    if let Some(stored) =
        sqlx::query_scalar::<_, Vec<u8>>("SELECT setup FROM opaque_server_setup WHERE id = TRUE")
            .fetch_optional(pool)
            .await
            .wrap_err("failed to read OPAQUE server setup")?
    {
        let opened = crypto
            .open(&stored, SETUP_AAD)
            .wrap_err("failed to open the stored OPAQUE server setup")?;
        // Migrate a pre-encryption plaintext row to sealed, preserving the bytes.
        if !opened.was_sealed {
            let sealed = crypto.seal(&opened.plaintext, SETUP_AAD);
            sqlx::query("UPDATE opaque_server_setup SET setup = $1 WHERE id = TRUE")
                .bind(&sealed)
                .execute(pool)
                .await
                .wrap_err("failed to reseal the OPAQUE server setup")?;
        }
        return ServerSetup::<Suite>::deserialize(&opened.plaintext)
            .wrap_err("stored OPAQUE server setup is corrupt");
    }

    let setup = ServerSetup::<Suite>::new(&mut OsRng);
    let sealed = crypto.seal(&setup.serialize(), SETUP_AAD);
    sqlx::query(
        "INSERT INTO opaque_server_setup (id, setup) VALUES (TRUE, $1) ON CONFLICT DO NOTHING",
    )
    .bind(&sealed)
    .execute(pool)
    .await
    .wrap_err("failed to store OPAQUE server setup")?;

    // Re-read so a racing insert's winning row is the one every caller uses.
    let stored =
        sqlx::query_scalar::<_, Vec<u8>>("SELECT setup FROM opaque_server_setup WHERE id = TRUE")
            .fetch_one(pool)
            .await
            .wrap_err("failed to re-read OPAQUE server setup")?;
    let opened = crypto
        .open(&stored, SETUP_AAD)
        .wrap_err("failed to open the stored OPAQUE server setup")?;
    ServerSetup::<Suite>::deserialize(&opened.plaintext)
        .wrap_err("stored OPAQUE server setup is corrupt")
}

/// Answers a client's registration request; returns the base64 response message.
///
/// `identifier` binds the record to the account (Elysium uses the lowercased
/// email); the same identifier must be presented at login.
///
/// # Errors
/// Returns an error if the request is malformed or the OPRF evaluation fails.
pub fn registration_response(
    setup: &ServerSetup<Suite>,
    request_b64: &str,
    identifier: &[u8],
) -> eyre::Result<String> {
    let request_bytes = B64
        .decode(request_b64)
        .wrap_err("registration request is not valid base64")?;
    let request = RegistrationRequest::<Suite>::deserialize(&request_bytes)
        .map_err(|error| eyre!("invalid registration request: {error}"))?;
    let result = ServerRegistration::<Suite>::start(setup, request, identifier)
        .map_err(|error| eyre!("registration start failed: {error}"))?;
    Ok(B64.encode(result.message.serialize()))
}

/// Finalizes registration, returning the record bytes to persist for the user.
///
/// # Errors
/// Returns an error if the upload message is malformed.
pub fn finalize_registration(upload_b64: &str) -> eyre::Result<Vec<u8>> {
    let upload_bytes = B64
        .decode(upload_b64)
        .wrap_err("registration upload is not valid base64")?;
    let upload = RegistrationUpload::<Suite>::deserialize(&upload_bytes)
        .map_err(|error| eyre!("invalid registration upload: {error}"))?;
    Ok(ServerRegistration::<Suite>::finish(upload)
        .serialize()
        .to_vec())
}

/// The outcome of starting a login: the base64 KE2 message plus the server state.
///
/// The state must be carried to [`login_finish`]; Elysium rides it in a
/// short-lived cookie, so it is returned as bytes ready to serialize.
pub struct LoginStart {
    /// The base64 credential response (KE2) to return to the client.
    pub ke2: String,
    /// The serialized `ServerLogin` state to carry to finish.
    pub state: Vec<u8>,
}

/// Starts a login exchange, producing KE2 and the state to complete it.
///
/// `record` is the user's stored registration bytes, or `None` for an unknown
/// user; passing `None` yields a response indistinguishable from a real one, so
/// the endpoint never reveals whether an account exists.
///
/// # Errors
/// Returns an error if KE1 or the stored record is malformed, or the exchange
/// cannot start.
pub fn login_start(
    setup: &ServerSetup<Suite>,
    record: Option<&[u8]>,
    ke1_b64: &str,
    identifier: &[u8],
) -> eyre::Result<LoginStart> {
    let password_file = match record {
        Some(bytes) => Some(
            ServerRegistration::<Suite>::deserialize(bytes)
                .wrap_err("stored password record is corrupt")?,
        ),
        None => None,
    };
    let ke1_bytes = B64
        .decode(ke1_b64)
        .wrap_err("login request is not valid base64")?;
    let ke1 = CredentialRequest::<Suite>::deserialize(&ke1_bytes)
        .map_err(|error| eyre!("invalid login request: {error}"))?;

    let result = ServerLogin::start(
        &mut OsRng,
        setup,
        password_file,
        ke1,
        identifier,
        ServerLoginParameters::default(),
    )
    .map_err(|error| eyre!("login start failed: {error}"))?;

    Ok(LoginStart {
        ke2: B64.encode(result.message.serialize()),
        state: result.state.serialize().to_vec(),
    })
}

/// Completes a login exchange; `Ok(())` means the client proved the password.
///
/// A wrong password (or a `None`-backed dummy exchange) fails here rather than at
/// [`login_start`], which is what makes the unknown-user path indistinguishable.
///
/// # Errors
/// Returns an error if the state or KE3 is malformed, or verification fails.
pub fn login_finish(state_bytes: &[u8], ke3_b64: &str) -> eyre::Result<()> {
    let state =
        ServerLogin::<Suite>::deserialize(state_bytes).wrap_err("login state is corrupt")?;
    let ke3_bytes = B64
        .decode(ke3_b64)
        .wrap_err("login finalization is not valid base64")?;
    let ke3 = CredentialFinalization::<Suite>::deserialize(&ke3_bytes)
        .map_err(|error| eyre!("invalid login finalization: {error}"))?;
    state
        .finish(ke3, ServerLoginParameters::default())
        .map_err(|error| eyre!("login verification failed: {error}"))?;
    Ok(())
}
