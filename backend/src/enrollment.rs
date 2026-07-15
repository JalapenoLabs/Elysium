//! Shared credential enrollment: a passkey, or an OPAQUE password + TOTP.
//!
//! Enrolling a credential is the same three steps everywhere it happens: `begin`
//! issues the ceremony material, `verify` checks what the client sends back, and
//! `insert` persists it. First-run claim ([`crate::bootstrap`]), invite acceptance
//! ([`crate::invites`]), and authenticated add-credential ([`crate::credentials`])
//! all build on these, differing only in what they wrap around the credential
//! (an org, a membership, or nothing). Keeping the ceremony in one place means the
//! passkey and OPAQUE handling is written and audited once.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, PasskeyRegistration, RegisterPublicKeyCredential,
};
use webauthn_rs_core::proto::ResidentKeyRequirement;

use crate::crypto::Crypto;
use crate::login_events::Method;
use crate::password::normalize;
use crate::state::AppState;
use crate::{opaque, totp, webauthn};

/// In-flight credential state carried between begin and finish (rides a cookie).
#[derive(Serialize, Deserialize)]
pub enum PendingCredential {
    /// Passkey enrollment: the in-flight WebAuthn registration state.
    Passkey(PasskeyRegistration),
    /// Password enrollment: the TOTP secret (base64) minted at begin, confirmed
    /// against a code at finish before anything persists.
    Password { totp_secret: String },
}

/// Verified credential material, ready to persist.
pub enum Credential {
    /// A passkey as its `(credential_id, passkey_json)` storage form.
    Passkey((String, serde_json::Value)),
    /// An OPAQUE record plus the confirmed TOTP secret.
    Password { record: Vec<u8>, secret: Vec<u8> },
}

impl Credential {
    /// Which login method this credential represents, for the audit trail.
    pub fn method(&self) -> Method {
        match self {
            Self::Passkey(_) => Method::Passkey,
            Self::Password { .. } => Method::Password,
        }
    }
}

/// Finish-request fields common to every enrollment flow (method-dependent).
#[derive(Deserialize, Default)]
pub struct FinishBody {
    /// Passkey path: the created credential from `navigator.credentials`.
    pub credential: Option<RegisterPublicKeyCredential>,
    /// Password path: the OPAQUE registration upload (base64).
    pub opaque_registration_upload: Option<String>,
    /// Password path: a current TOTP code proving the authenticator is set up.
    pub totp_code: Option<String>,
}

/// Requests a discoverable credential on a passkey registration challenge.
///
/// `webauthn-rs`'s `start_passkey_registration` asks for `residentKey: "discouraged"`
/// (it calls `require_resident_key(false)` internally). Credential-manager passkey
/// providers such as Bitwarden and 1Password only create discoverable credentials, so
/// under "discouraged" they step aside and the platform authenticator (Windows Hello,
/// Touch ID) silently takes over, leaving the user no way to save the passkey in their
/// manager. Requesting a discoverable credential restores that choice and makes the
/// passkey a true passwordless factor, matching `docs/auth-model.md`.
///
/// Only the challenge sent to the client is changed; the stored registration state is
/// left as-is. That is sound because `finish_passkey_registration` does not verify the
/// resident-key requirement against the returned credential (a discoverable credential
/// is a strict superset of a non-discoverable one), so the ceremony still completes.
fn request_discoverable_credential(challenge: &mut CreationChallengeResponse) {
    if let Some(selection) = challenge.public_key.authenticator_selection.as_mut() {
        selection.resident_key = Some(ResidentKeyRequirement::Required);
        selection.require_resident_key = true;
    }
}

/// Starts enrollment for `method`, returning the pending state and client payload.
///
/// `user_id` is the WebAuthn user handle for the passkey path (the caller keeps it
/// so the stored credential binds to the eventual user row). The `payload` is the
/// JSON to return to the client: a passkey challenge, or the OPAQUE registration
/// response plus a TOTP provisioning URI.
///
/// # Errors
/// Returns a ready `Response` (400/500) when the method is unsupported or a
/// ceremony cannot start.
#[expect(
    clippy::result_large_err,
    reason = "the Err is a ready HTTP Response the caller returns as-is"
)]
pub fn begin(
    state: &AppState,
    user_id: Uuid,
    email: &str,
    display_name: &str,
    method: &str,
    opaque_request: Option<&str>,
) -> Result<(PendingCredential, serde_json::Value), Response> {
    match method {
        "passkey" => {
            let (mut challenge, registration) = state
                .webauthn
                .start_passkey_registration(user_id, email, display_name, None)
                .map_err(|error| {
                    tracing::error!(error = %error, "start_passkey_registration failed");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                })?;
            request_discoverable_credential(&mut challenge);
            Ok((
                PendingCredential::Passkey(registration),
                json!({ "passkey": challenge }),
            ))
        }
        "password" => {
            let Some(request) = opaque_request else {
                return Err(
                    (StatusCode::BAD_REQUEST, "missing registration request").into_response()
                );
            };
            let identifier = normalize(email);
            let response =
                opaque::registration_response(&state.opaque_setup, request, identifier.as_bytes())
                    .map_err(|error| {
                        tracing::debug!(error = %error, "opaque registration start failed");
                        (StatusCode::BAD_REQUEST, "invalid registration request").into_response()
                    })?;
            let secret = totp::generate_secret();
            let uri = totp::provisioning_uri(&secret, email).map_err(|error| {
                tracing::error!(error = %error, "totp provisioning failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            })?;
            Ok((
                PendingCredential::Password {
                    totp_secret: STANDARD.encode(&secret),
                },
                json!({
                    "opaque_registration_response": response,
                    "totp": { "provisioning_uri": uri },
                }),
            ))
        }
        other => {
            tracing::debug!(method = other, "unsupported enrollment method");
            Err((StatusCode::BAD_REQUEST, "unsupported method").into_response())
        }
    }
}

/// Verifies the finish payload against the pending credential.
///
/// # Errors
/// Returns a ready `Response` (400/500) when the payload is missing, the passkey
/// or OPAQUE upload is invalid, or the TOTP code does not match.
#[expect(
    clippy::result_large_err,
    reason = "the Err is a ready HTTP Response the caller returns as-is"
)]
pub fn verify(
    state: &AppState,
    pending: &PendingCredential,
    body: &FinishBody,
) -> Result<Credential, Response> {
    match pending {
        PendingCredential::Passkey(registration) => {
            let Some(credential) = &body.credential else {
                return Err((StatusCode::BAD_REQUEST, "missing credential").into_response());
            };
            let passkey = state
                .webauthn
                .finish_passkey_registration(credential, registration)
                .map_err(|error| {
                    tracing::debug!(error = %error, "passkey registration verification failed");
                    (StatusCode::BAD_REQUEST, "registration failed").into_response()
                })?;
            let row = webauthn::credential_row(&passkey).map_err(|error| {
                tracing::error!(error = %error, "failed to serialize passkey");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            })?;
            Ok(Credential::Passkey(row))
        }
        PendingCredential::Password { totp_secret } => {
            let (Some(upload), Some(code)) = (&body.opaque_registration_upload, &body.totp_code)
            else {
                return Err((StatusCode::BAD_REQUEST, "missing password or code").into_response());
            };
            let Ok(secret) = STANDARD.decode(totp_secret) else {
                return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
            };
            // Confirm the authenticator is really set up before persisting.
            if !totp::verify(&secret, code).unwrap_or(false) {
                return Err((StatusCode::BAD_REQUEST, "invalid code").into_response());
            }
            let record = opaque::finalize_registration(upload).map_err(|error| {
                tracing::debug!(error = %error, "opaque registration finalize failed");
                (StatusCode::BAD_REQUEST, "invalid registration").into_response()
            })?;
            Ok(Credential::Password { record, secret })
        }
    }
}

/// Persists a verified credential for `user_id` on the given connection.
///
/// Runs inside the caller's transaction (pass `&mut *tx`), so the credential lands
/// atomically with whatever else the flow writes. A TOTP secret is sealed under
/// `crypto` (bound to the user id) before it is stored (see [`crate::crypto`]).
///
/// # Errors
/// Returns an error if any insert fails.
pub async fn insert(
    conn: &mut PgConnection,
    crypto: &Crypto,
    user_id: Uuid,
    credential: &Credential,
) -> sqlx::Result<()> {
    match credential {
        Credential::Passkey((credential_id, passkey)) => {
            sqlx::query(
                "INSERT INTO webauthn_credentials (user_id, credential_id, passkey)
                 VALUES ($1, $2, $3)",
            )
            .bind(user_id)
            .bind(credential_id)
            .bind(passkey)
            .execute(&mut *conn)
            .await?;
        }
        Credential::Password { record, secret } => {
            // A user has at most one password; upsert so add-credential can also
            // change an existing password in place.
            sqlx::query(
                "INSERT INTO user_password_credentials (user_id, record) VALUES ($1, $2)
                 ON CONFLICT (user_id) DO UPDATE SET record = EXCLUDED.record",
            )
            .bind(user_id)
            .bind(record)
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                "INSERT INTO user_totp_secrets (user_id, secret, confirmed_at)
                 VALUES ($1, $2, now())
                 ON CONFLICT (user_id) DO UPDATE SET secret = EXCLUDED.secret, confirmed_at = now()",
            )
            .bind(user_id)
            .bind(crypto.seal(secret, user_id.as_bytes()))
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// The passkey challenge we send must request a discoverable credential, otherwise
    /// credential-manager providers (Bitwarden, 1Password) decline and only the platform
    /// authenticator is offered. This drives the real `webauthn-rs` ceremony and asserts
    /// the emitted wire JSON, so a library default change that regresses this is caught.
    #[test]
    fn passkey_challenge_requests_a_discoverable_credential() {
        let webauthn =
            crate::webauthn::build("localhost", "http://localhost:5173").expect("RP builds");
        let (mut challenge, _registration) = webauthn
            .start_passkey_registration(Uuid::new_v4(), "owner@example.com", "Owner", None)
            .expect("registration starts");

        // Guard the premise: webauthn-rs's untouched default is the setting we override.
        let before = serde_json::to_value(&challenge).expect("challenge serializes");
        assert_eq!(
            before["publicKey"]["authenticatorSelection"]["residentKey"],
            "discouraged"
        );

        request_discoverable_credential(&mut challenge);

        let after = serde_json::to_value(&challenge).expect("challenge serializes");
        assert_eq!(
            after["publicKey"]["authenticatorSelection"]["residentKey"],
            "required"
        );
        assert_eq!(
            after["publicKey"]["authenticatorSelection"]["requireResidentKey"],
            true
        );
    }
}
