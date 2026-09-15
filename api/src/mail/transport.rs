// Copyright © 2026 Jalapeno Labs

//! IMAP and SMTP: the one transport every mailbox uses, whoever hosts it.
//!
//! A [`Mailbox`] is a resolved account: where its servers are and a credential that
//! works right now. Gmail and Outlook authenticate with an OAuth access token over
//! SASL `XOAUTH2`; the self-hosted mail server takes the mailbox password. Past that
//! point nothing here knows or cares which provider it is talking to.
//!
//! Every connection is TLS. The only unverified certificate is the bundled mail
//! server's own self-signed one, reached over the compose network the same way the
//! API reaches Postgres and Redis; see [`Endpoint::verify_certificate`].

use std::sync::Arc;
use std::time::Duration;

use async_imap::Authenticator;
use lettre::message::Mailbox as Address;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use secrecy::{ExposeSecret, SecretString};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// How long one IMAP or SMTP exchange may take before it counts as unreachable.
///
/// Covers connect, TLS, and authentication against a provider under load. A request
/// waits on it, so it stays well inside the API's 30-second request timeout.
const MAIL_SERVER_TIMEOUT: Duration = Duration::from_secs(15);

/// How a connection is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    ImplicitTls,
    /// Plaintext greeting upgraded with `STARTTLS` before authenticating (SMTP 587).
    StartTls,
}

/// One server a mailbox talks to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub security: Security,
    /// False only for the bundled mail server, whose certificate is self-signed.
    /// Its traffic stays on the compose network and never leaves the host.
    pub verify_certificate: bool,
}

/// How a mailbox proves who it is.
#[derive(Debug)]
pub enum Auth {
    Password(SecretString),
    /// A short-lived OAuth access token, used with SASL `XOAUTH2`.
    AccessToken(SecretString),
}

/// A mailbox ready to connect.
#[derive(Debug)]
pub struct Mailbox {
    pub address: String,
    pub display_name: String,
    pub imap: Endpoint,
    pub smtp: Endpoint,
    pub auth: Auth,
}

/// A mail server refused the credential or could not be reached. The message is the
/// server's or the transport's own and never contains the credential.
#[derive(Debug, thiserror::Error)]
#[error("{protocol}: {message}")]
pub struct MailError {
    protocol: &'static str,
    message: String,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "taken by value so the constructors pass straight to map_err"
)]
impl MailError {
    fn imap(message: impl ToString) -> Self {
        Self {
            protocol: "IMAP",
            message: message.to_string(),
        }
    }

    fn smtp(message: impl ToString) -> Self {
        Self {
            protocol: "SMTP",
            message: message.to_string(),
        }
    }
}

/// Signs in to both servers and signs out again.
///
/// # Errors
/// Returns [`MailError`] naming the first server that refused or did not answer.
pub async fn check(mailbox: &Mailbox) -> Result<(), MailError> {
    check_imap(mailbox).await?;

    let reachable = smtp_transport(mailbox)?
        .test_connection()
        .await
        .map_err(MailError::smtp)?;
    if !reachable {
        return Err(MailError::smtp(
            "the server closed the connection after signing in",
        ));
    }
    Ok(())
}

/// Sends a plain-text message from `mailbox` to `recipient`.
///
/// # Errors
/// Returns [`MailError`] when an address does not parse or the server refuses the
/// message.
pub async fn send_text(
    mailbox: &Mailbox,
    recipient: &str,
    subject: &str,
    body: String,
) -> Result<(), MailError> {
    let display_name = Some(mailbox.display_name.clone()).filter(|name| !name.is_empty());
    let from = Address::new(
        display_name,
        mailbox.address.parse().map_err(MailError::smtp)?,
    );
    let message = Message::builder()
        .from(from)
        .to(recipient.parse().map_err(MailError::smtp)?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(MailError::smtp)?;

    smtp_transport(mailbox)?
        .send(message)
        .await
        .map_err(MailError::smtp)?;
    Ok(())
}

async fn check_imap(mailbox: &Mailbox) -> Result<(), MailError> {
    tokio::time::timeout(MAIL_SERVER_TIMEOUT, sign_in_and_out_of_imap(mailbox))
        .await
        .map_err(|_elapsed| MailError::imap("the server did not answer in time"))?
}

async fn sign_in_and_out_of_imap(mailbox: &Mailbox) -> Result<(), MailError> {
    let endpoint = &mailbox.imap;
    let tcp = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .await
        .map_err(MailError::imap)?;
    let server_name = ServerName::try_from(endpoint.host.clone()).map_err(MailError::imap)?;
    let stream = TlsConnector::from(tls_config(endpoint.verify_certificate))
        .connect(server_name, tcp)
        .await
        .map_err(MailError::imap)?;

    let mut client = async_imap::Client::new(stream);
    client
        .read_response()
        .await
        .map_err(MailError::imap)?
        .ok_or_else(|| MailError::imap("the server closed the connection before greeting"))?;

    let signed_in = match &mailbox.auth {
        Auth::Password(password) => {
            client
                .login(&mailbox.address, password.expose_secret())
                .await
        }
        Auth::AccessToken(access_token) => {
            let authenticator = XOAuth2 {
                address: &mailbox.address,
                access_token,
                answered: false,
            };
            client.authenticate("XOAUTH2", authenticator).await
        }
    };
    let mut session = signed_in.map_err(|(error, _client)| MailError::imap(error))?;

    session.logout().await.map_err(MailError::imap)
}

fn smtp_transport(mailbox: &Mailbox) -> Result<AsyncSmtpTransport<Tokio1Executor>, MailError> {
    let endpoint = &mailbox.smtp;
    let parameters = TlsParameters::builder(endpoint.host.clone())
        .dangerous_accept_invalid_certs(!endpoint.verify_certificate)
        // Stalwart's generated certificate names `localhost`, not its compose hostname.
        .dangerous_accept_invalid_hostnames(!endpoint.verify_certificate)
        .build_rustls()
        .map_err(MailError::smtp)?;
    let tls = match endpoint.security {
        Security::ImplicitTls => Tls::Wrapper(parameters),
        Security::StartTls => Tls::Required(parameters),
    };

    let (secret, mechanism) = match &mailbox.auth {
        Auth::Password(password) => (password, Mechanism::Plain),
        Auth::AccessToken(access_token) => (access_token, Mechanism::Xoauth2),
    };

    Ok(
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&endpoint.host)
            .port(endpoint.port)
            .tls(tls)
            .credentials(Credentials::new(
                mailbox.address.clone(),
                secret.expose_secret().to_owned(),
            ))
            .authentication(vec![mechanism])
            .timeout(Some(MAIL_SERVER_TIMEOUT))
            .build(),
    )
}

/// SASL `XOAUTH2`, as Gmail and Outlook define it.
struct XOAuth2<'a> {
    address: &'a str,
    access_token: &'a SecretString,
    /// A refusal arrives as a second challenge carrying a JSON error; the client must
    /// answer it with an empty response to receive the final `NO`.
    answered: bool,
}

impl Authenticator for XOAuth2<'_> {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        if self.answered {
            return String::new();
        }
        self.answered = true;
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.address,
            self.access_token.expose_secret()
        )
    }
}

fn tls_config(verify_certificate: bool) -> Arc<ClientConfig> {
    if verify_certificate {
        let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        return Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
    }

    Arc::new(
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptSelfSigned))
            .with_no_client_auth(),
    )
}

/// Accepts any certificate while still checking the handshake signatures.
///
/// Used only for the bundled mail server, whose endpoint is fixed by configuration and
/// not by anything a request supplies. Its certificate is self-signed and regenerated
/// with its data volume, so there is no stable certificate to pin.
#[derive(Debug)]
struct AcceptSelfSigned;

impl ServerCertVerifier for AcceptSelfSigned {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids rustls's abbreviated `cert` and `dss`"
    )]
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids rustls's abbreviated `cert` and `dss`"
    )]
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoauth2_sends_the_bearer_once_then_acknowledges_the_refusal() {
        let token = SecretString::from("ya29.token");
        let mut authenticator = XOAuth2 {
            address: "someone@gmail.com",
            access_token: &token,
            answered: false,
        };

        assert_eq!(
            authenticator.process(b""),
            "user=someone@gmail.com\x01auth=Bearer ya29.token\x01\x01"
        );
        assert_eq!(authenticator.process(br#"{"status":"401"}"#), "");
    }
}
