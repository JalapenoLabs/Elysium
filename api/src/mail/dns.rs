// Copyright © 2026 Jalapeno Labs

//! The DNS records a mail domain needs, and whether public DNS serves them.
//!
//! Stalwart generates every record it recommends for a domain as a zone file (see
//! [`super::stalwart::Stalwart::domain_zone_file`]). Elysium picks out the records mail
//! delivery and authentication depend on, which the operator publishes at their DNS
//! provider:
//!
//! | Purpose | Type | Name                          | Why                                          |
//! |---------|------|-------------------------------|----------------------------------------------|
//! | `mx`    | MX   | the domain                    | Where other servers deliver the domain's mail |
//! | `spf`   | TXT  | the domain, the server's host | Which hosts may send as the domain           |
//! | `dkim`  | TXT  | `<selector>._domainkey.<domain>` | The keys outgoing mail is signed with     |
//! | `dmarc` | TXT  | `_dmarc.<domain>`             | What receivers do with mail that fails both  |
//!
//! Service discovery records (SRV, autoconfig) are left to the full zone file.
//!
//! [`DnsChecker`] then looks each record up through the resolver the API's container
//! uses, which is public DNS. It never changes DNS: publishing records is the operator's
//! part, as is the address record for the server's hostname.

use futures_util::future::join_all;
use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use serde::Serialize;

/// What a recommended record is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordPurpose {
    Mx,
    Spf,
    Dkim,
    Dmarc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RecordType {
    #[serde(rename = "MX")]
    Mx,
    #[serde(rename = "TXT")]
    Txt,
}

/// A record Stalwart recommends. `value` is written the way DNS providers take it: an
/// MX as `<priority> <host>`, a TXT as its text without quotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedRecord {
    pub purpose: RecordPurpose,
    pub record_type: RecordType,
    /// Fully qualified, without the trailing dot.
    pub name: String,
    pub value: String,
}

/// How public DNS compares with a recommended record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordStatus {
    /// Public DNS serves the recommended value.
    Published,
    /// Public DNS serves a record for the same purpose with another value, such as a
    /// DMARC policy the operator chose. Not necessarily wrong.
    Different,
    /// Public DNS serves nothing for the purpose.
    Missing,
    /// The lookup itself failed, so nothing is known.
    Unverified,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckedRecord {
    #[serde(flatten)]
    pub record: RecommendedRecord,
    pub status: RecordStatus,
    /// What public DNS serves for the purpose, in the same notation as `value`.
    pub found: Vec<String>,
    /// Why the lookup failed, when `status` is `unverified`.
    pub error: Option<String>,
}

/// The mail delivery and authentication records in a BIND zone file, in file order.
pub fn essential_records(zone_file: &str) -> Vec<RecommendedRecord> {
    let mut records = Vec::new();
    for entry in zone_entries(zone_file) {
        let mut fields = entry.splitn(4, char::is_whitespace);
        let (Some(owner), Some("IN"), Some(kind), Some(data)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let name = owner.trim_end_matches('.').to_lowercase();

        let record = match kind {
            "MX" => RecommendedRecord {
                purpose: RecordPurpose::Mx,
                record_type: RecordType::Mx,
                name,
                value: data.trim().trim_end_matches('.').to_lowercase(),
            },
            "TXT" => {
                let value = quoted_strings(data);
                let Some(purpose) = txt_purpose(&name, &value) else {
                    continue;
                };
                RecommendedRecord {
                    purpose,
                    record_type: RecordType::Txt,
                    name,
                    value,
                }
            }
            _ => continue,
        };
        records.push(record);
    }
    records
}

/// Zone file entries one per string, with parenthesized continuation lines joined.
fn zone_entries(zone_file: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut pending: Option<String> = None;

    for line in zone_file.lines() {
        if let Some(open) = pending.as_mut() {
            open.push(' ');
            open.push_str(line.trim());
            if line.contains(')') {
                entries.push(open.replace(['(', ')'], " "));
                pending = None;
            }
            continue;
        }
        if line.contains('(') && !line.contains(')') {
            pending = Some(line.trim().to_owned());
            continue;
        }
        if !line.trim().is_empty() {
            entries.push(line.trim().to_owned());
        }
    }
    entries
}

/// The concatenation of every quoted string in `data`, which is how DNS joins a TXT
/// record's character strings.
fn quoted_strings(data: &str) -> String {
    data.split('"').skip(1).step_by(2).collect()
}

fn txt_purpose(name: &str, value: &str) -> Option<RecordPurpose> {
    if name.starts_with("_dmarc.") {
        return Some(RecordPurpose::Dmarc);
    }
    if name.contains("._domainkey.") {
        return Some(RecordPurpose::Dkim);
    }
    if value.starts_with("v=spf1") {
        return Some(RecordPurpose::Spf);
    }
    None
}

/// Compares a record with what public DNS serves under its name.
fn evaluate(record: &RecommendedRecord, served: &[String]) -> (RecordStatus, Vec<String>) {
    // Several records share a name: a domain's TXT set holds SPF alongside site
    // verification strings. Only those serving the same purpose are compared.
    let relevant: Vec<String> = served
        .iter()
        .filter(|value| match record.purpose {
            RecordPurpose::Mx | RecordPurpose::Dkim => true,
            RecordPurpose::Spf => value.starts_with("v=spf1"),
            RecordPurpose::Dmarc => value.starts_with("v=DMARC1"),
        })
        .cloned()
        .collect();

    // Providers reformat whitespace (splitting long DKIM keys, spacing after `;`), which
    // changes nothing a receiver reads.
    let comparable = |value: &str| value.split_whitespace().collect::<String>();
    let expected = comparable(&record.value);

    let status = if relevant.is_empty() {
        RecordStatus::Missing
    } else if relevant.iter().any(|value| comparable(value) == expected) {
        RecordStatus::Published
    } else {
        RecordStatus::Different
    };
    (status, relevant)
}

/// Looks records up in public DNS.
#[derive(Debug, Clone)]
pub struct DnsChecker {
    resolver: TokioResolver,
}

impl DnsChecker {
    /// A checker using the system's resolver configuration.
    ///
    /// # Errors
    /// Fails when the resolver configuration cannot be read.
    pub fn from_system() -> anyhow::Result<Self> {
        let resolver = TokioResolver::builder_tokio()?.build()?;
        Ok(Self { resolver })
    }

    /// Checks every record, concurrently.
    pub async fn check(&self, records: Vec<RecommendedRecord>) -> Vec<CheckedRecord> {
        join_all(records.into_iter().map(|record| self.check_one(record))).await
    }

    async fn check_one(&self, record: RecommendedRecord) -> CheckedRecord {
        // A fully qualified query skips the resolver's search domains.
        let query = format!("{}.", record.name);
        let lookup = match record.record_type {
            RecordType::Mx => self.resolver.mx_lookup(query).await,
            RecordType::Txt => self.resolver.txt_lookup(query).await,
        };

        let served = match lookup {
            Ok(answer) => answer
                .answers()
                .iter()
                .filter_map(|answer| match &answer.data {
                    RData::MX(mx) => Some(format!(
                        "{} {}",
                        mx.preference,
                        mx.exchange.to_ascii().trim_end_matches('.').to_lowercase()
                    )),
                    RData::TXT(txt) => Some(
                        txt.txt_data
                            .iter()
                            .map(|part| String::from_utf8_lossy(part))
                            .collect(),
                    ),
                    _ => None,
                })
                .collect(),
            Err(error) if error.is_no_records_found() || error.is_nx_domain() => Vec::new(),
            Err(error) => {
                return CheckedRecord {
                    record,
                    status: RecordStatus::Unverified,
                    found: Vec::new(),
                    error: Some(error.to_string()),
                };
            }
        };

        let (status, found) = evaluate(&record, &served);
        CheckedRecord {
            record,
            status,
            found,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An excerpt of what Stalwart 0.16.22 generates for its default domain.
    const ZONE_FILE: &str = r#"
v1-ed25519-20260915._domainkey.alpha.test. IN TXT "v=DKIM1; k=ed25519; h=sha256; p=vdwJW9e3"
v1-rsa-20260915._domainkey.alpha.test. IN TXT (
    "v=DKIM1; k=rsa; h=sha256; p=MIIBIjAN"
    "rWG0ijgq"
)
mail.alpha.test. IN TXT "v=spf1 a -all"
alpha.test. IN TXT "v=spf1 mx -all"
alpha.test. IN MX 10 mail.alpha.test.
_dmarc.alpha.test. IN TXT "v=DMARC1; p=reject; rua=mailto:postmaster@alpha.test"
_imaps._tcp.alpha.test. IN SRV 0 1 993 mail.alpha.test.
autoconfig.alpha.test. IN CNAME mail.alpha.test.
_mta-sts.alpha.test. IN TXT "v=STSv1; id=3436708282880028550"
"#;

    #[test]
    fn essential_records_come_from_the_zone_file() {
        let records = essential_records(ZONE_FILE);
        let summary: Vec<(RecordPurpose, &str, &str)> = records
            .iter()
            .map(|record| (record.purpose, record.name.as_str(), record.value.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    RecordPurpose::Dkim,
                    "v1-ed25519-20260915._domainkey.alpha.test",
                    "v=DKIM1; k=ed25519; h=sha256; p=vdwJW9e3"
                ),
                (
                    RecordPurpose::Dkim,
                    "v1-rsa-20260915._domainkey.alpha.test",
                    "v=DKIM1; k=rsa; h=sha256; p=MIIBIjANrWG0ijgq"
                ),
                (RecordPurpose::Spf, "mail.alpha.test", "v=spf1 a -all"),
                (RecordPurpose::Spf, "alpha.test", "v=spf1 mx -all"),
                (RecordPurpose::Mx, "alpha.test", "10 mail.alpha.test"),
                (
                    RecordPurpose::Dmarc,
                    "_dmarc.alpha.test",
                    "v=DMARC1; p=reject; rua=mailto:postmaster@alpha.test"
                ),
            ]
        );
    }

    fn record(purpose: RecordPurpose, value: &str) -> RecommendedRecord {
        RecommendedRecord {
            purpose,
            record_type: RecordType::Txt,
            name: "alpha.test".to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn records_compare_by_purpose_and_ignore_whitespace() {
        let spf = record(RecordPurpose::Spf, "v=spf1 mx -all");
        let served = vec![
            "google-site-verification=abc".to_owned(),
            "v=spf1  mx -all".to_owned(),
        ];
        assert_eq!(
            evaluate(&spf, &served),
            (RecordStatus::Published, vec!["v=spf1  mx -all".to_owned()])
        );

        let dmarc = record(RecordPurpose::Dmarc, "v=DMARC1; p=reject");
        let (status, _found) = evaluate(&dmarc, &["v=DMARC1; p=quarantine".to_owned()]);
        assert_eq!(status, RecordStatus::Different);

        let (status, found) = evaluate(&spf, &["google-site-verification=abc".to_owned()]);
        assert_eq!((status, found.len()), (RecordStatus::Missing, 0));
    }
}
