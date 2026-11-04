// Copyright © 2026 Jalapeno Labs

//! Where a mail goes, in a draft's terms: its recipients, its subject, and the message it
//! answers. See `docs/connectors.md`, Sending.
//!
//! Mail is answered from a mailbox of the person's own, over SMTP ([`super::transport`]),
//! whatever read the message: Outlook and Gmail are read through a browser, which has no way to
//! send that is not the page's own markup or the services' internal calls.
//!
//! A reply goes to the message's sender, as the record names them (`Ada <ada@example.com>`),
//! under its subject with `Re: ` in front unless it already starts with one. When the record
//! keeps the `Message-ID` it answers (`messageId`, and the conversation's `references`), the
//! reply carries them, so the recipient's mail client files it in that conversation. The
//! browser readers keep neither today; reading mail over IMAP will.

use lettre::message::Mailbox as Address;
use serde::{Deserialize, Serialize};

use crate::models::timeline_record::TimelineRecord;

/// The longest subject kept, so a reply to a long one is still a valid header.
const SUBJECT_MAX_CHARACTERS: usize = 998;

/// Where a mail goes. A draft keeps it as its target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailTarget {
    /// Each recipient as an address, optionally with a name.
    pub to: Vec<String>,
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
}

/// Where a reply to the mail `record` goes.
///
/// # Errors
/// Why it cannot be answered by mail: its sender has no address.
pub fn reply_target(record: &TimelineRecord) -> Result<MailTarget, String> {
    let sender = record.actor.trim();
    let recipient = sender
        .parse::<Address>()
        .map_err(|_unparsed| format!("{sender:?} has no address to reply to"))?;

    let in_reply_to = record.metadata["messageId"].as_str().map(str::to_owned);
    let mut references: Vec<String> = record.metadata["references"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|reference| reference.as_str().map(str::to_owned))
        .collect();
    if let Some(answered) = &in_reply_to
        && !references.contains(answered)
    {
        references.push(answered.clone());
    }

    Ok(MailTarget {
        to: vec![recipient.to_string()],
        subject: reply_subject(&record.title),
        in_reply_to,
        references,
    })
}

/// A new mail's target: the recipients as typed, each checked to be an address.
///
/// # Errors
/// Naming a recipient that is not an address, or none at all.
pub fn new_target(to: &[String], subject: &str) -> Result<MailTarget, String> {
    if to.is_empty() {
        return Err("name at least one recipient".to_owned());
    }
    let mut recipients = Vec::with_capacity(to.len());
    for recipient in to {
        let address = recipient
            .trim()
            .parse::<Address>()
            .map_err(|_unparsed| format!("{recipient:?} is not an address"))?;
        recipients.push(address.to_string());
    }
    Ok(MailTarget {
        to: recipients,
        subject: subject
            .trim()
            .chars()
            .take(SUBJECT_MAX_CHARACTERS)
            .collect(),
        in_reply_to: None,
        references: Vec::new(),
    })
}

/// `subject` as a reply's: `Re: ` in front, once.
pub fn reply_subject(subject: &str) -> String {
    let subject = subject.trim();
    let already_a_reply = subject
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:"));
    let reply = if already_a_reply {
        subject.to_owned()
    } else {
        format!("Re: {subject}")
    };
    reply.trim().chars().take(SUBJECT_MAX_CHARACTERS).collect()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::{Value, json};

    use super::*;

    fn mail(actor: &str, title: &str, metadata: Value) -> TimelineRecord {
        TimelineRecord {
            id: uuid::Uuid::now_v7(),
            source_kind: "gmail".to_owned(),
            connector_id: None,
            external_id: Some("1812345678901234567".to_owned()),
            occurred_at: Utc::now(),
            actor: actor.to_owned(),
            title: title.to_owned(),
            body: "Are we launching Friday?".to_owned(),
            url: None,
            media: None,
            metadata,
            action_item_id: None,
            created_by: uuid::Uuid::now_v7(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn a_reply_goes_to_the_sender_under_the_subject() {
        let target = reply_target(&mail("Ada Lovelace <ada@example.org>", "Launch", json!({})))
            .expect("answerable");
        assert_eq!(
            target,
            MailTarget {
                to: vec!["Ada Lovelace <ada@example.org>".to_owned()],
                subject: "Re: Launch".to_owned(),
                in_reply_to: None,
                references: Vec::new(),
            }
        );
    }

    #[test]
    fn a_bare_address_is_replied_to_as_it_is() {
        let target =
            reply_target(&mail("ada@example.org", "Launch", json!({}))).expect("answerable");
        assert_eq!(target.to, ["ada@example.org"]);
    }

    #[test]
    fn a_sender_shown_by_name_alone_cannot_be_answered() {
        reply_target(&mail("Ada Lovelace", "Launch", json!({}))).expect_err("no address");
    }

    #[test]
    fn a_kept_message_id_threads_the_reply() {
        let record = mail(
            "ada@example.org",
            "Launch",
            json!({ "messageId": "<b@example.org>", "references": ["<a@example.org>"] }),
        );
        let target = reply_target(&record).expect("answerable");
        assert_eq!(target.in_reply_to.as_deref(), Some("<b@example.org>"));
        assert_eq!(target.references, ["<a@example.org>", "<b@example.org>"]);
    }

    #[test]
    fn a_subject_gains_re_once() {
        assert_eq!(reply_subject("Launch"), "Re: Launch");
        assert_eq!(reply_subject("RE: Launch"), "RE: Launch");
        assert_eq!(reply_subject("re:Launch"), "re:Launch");
        assert_eq!(reply_subject(""), "Re:");
        assert_eq!(reply_subject("Réunion"), "Re: Réunion");
    }

    #[test]
    fn a_new_mail_checks_each_recipient() {
        let target = new_target(&["Ada <ada@example.org>".to_owned()], " Launch ").expect("valid");
        assert_eq!(target.subject, "Launch");
        new_target(&[], "Launch").expect_err("nobody");
        new_target(&["Ada".to_owned()], "Launch").expect_err("not an address");
    }
}
