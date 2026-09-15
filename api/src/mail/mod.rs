// Copyright © 2026 Jalapeno Labs

//! Mailboxes Elysium reads and sends mail from.
//!
//! Three kinds of account exist, and they differ only in where their servers are and
//! how a credential is obtained:
//!
//! | Kind          | Servers                          | Credential                               |
//! |---------------|----------------------------------|------------------------------------------|
//! | `gmail`       | Google's IMAP and SMTP           | refresh token, via the [`broker`]        |
//! | `outlook`     | Microsoft 365's IMAP and SMTP    | refresh token, via the [`broker`]        |
//! | `self_hosted` | the bundled [`stalwart`] server  | a generated password                     |
//!
//! Everything after that goes through [`transport`], which is the same for all three.
//! See `docs/mail.md`.

pub mod broker;
pub mod stalwart;
pub mod transport;

use crate::models::mail_account::MailAccountKind;
use broker::Broker;
use stalwart::Stalwart;
use transport::{Endpoint, Security};

/// The mail services this deployment reaches.
#[derive(Debug, Clone)]
pub struct Mail {
    /// `None` when no broker URL is configured: Gmail and Outlook cannot connect.
    pub broker: Option<Broker>,
    /// The bundled server. Its administrator is stored in `mail_servers` once the
    /// settings page has set it up; until then no self-hosted mailbox can be created.
    pub stalwart: Stalwart,
}

impl Mail {
    /// Where an account of `kind` connects: its IMAP server, then its SMTP server.
    pub fn endpoints(&self, kind: MailAccountKind) -> (Endpoint, Endpoint) {
        let provider = |imap_host: &str, smtp_host: &str, smtp_port, smtp_security| {
            (
                Endpoint {
                    host: imap_host.to_owned(),
                    port: 993,
                    security: Security::ImplicitTls,
                    verify_certificate: true,
                },
                Endpoint {
                    host: smtp_host.to_owned(),
                    port: smtp_port,
                    security: smtp_security,
                    verify_certificate: true,
                },
            )
        };

        match kind {
            MailAccountKind::Gmail => provider(
                "imap.gmail.com",
                "smtp.gmail.com",
                465,
                Security::ImplicitTls,
            ),
            // Microsoft's SMTP AUTH endpoint only offers STARTTLS on 587.
            MailAccountKind::Outlook => provider(
                "outlook.office365.com",
                "smtp.office365.com",
                587,
                Security::StartTls,
            ),
            MailAccountKind::SelfHosted => self.stalwart.endpoints(),
        }
    }
}
