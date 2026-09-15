// Copyright © 2026 Jalapeno Labs

//! Administers the bundled Stalwart mail server over its JMAP management API.
//!
//! Stalwart keeps every setting (domains, accounts, listeners) as a JMAP object in
//! its own datastore, managed through `x:<Object>/get|query|set` calls at `/jmap/`.
//! The API signs in as the recovery administrator named in `compose.yml` and uses
//! only what self-hosted mailboxes need: find or create a domain, give the server a
//! hostname the first time, and create or destroy a user account.
//!
//! Mail itself never goes through here. Once a mailbox exists it is reached over
//! IMAP and SMTP like any other, see [`super::transport`].

use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use url::Url;

use super::transport::{Endpoint, Security};

/// The recovery administrator the API signs in as. `compose.yml` sets
/// `STALWART_RECOVERY_ADMIN` to this name and `STALWART_ADMIN_PASSWORD`.
pub const STALWART_ADMIN_USER: &str = "elysium";

/// JMAP capabilities every management call declares.
const USING: [&str; 2] = ["urn:ietf:params:jmap:core", "urn:stalwart:jmap"];

/// Management calls are local and small; anything slower means Stalwart is down.
const STALWART_TIMEOUT: Duration = Duration::from_secs(10);

/// Stalwart's implicit-TLS listeners, present in every new installation.
const IMAPS_PORT: u16 = 993;
const SUBMISSIONS_PORT: u16 = 465;

/// Stalwart refused a management call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum StalwartError {
    /// Another account already holds the address.
    #[error("the mail server already has an account for that address")]
    AddressTaken,
    #[error("mail server: {0}")]
    Refused(String),
}

#[derive(Debug, Clone)]
pub struct Stalwart {
    http: reqwest::Client,
    base_url: Url,
    admin_user: String,
    admin_password: SecretString,
}

impl Stalwart {
    pub const fn new(
        http: reqwest::Client,
        base_url: Url,
        admin_user: String,
        admin_password: SecretString,
    ) -> Self {
        Self {
            http,
            base_url,
            admin_user,
            admin_password,
        }
    }

    /// The IMAP and SMTP servers self-hosted mailboxes connect to.
    pub fn endpoints(&self) -> (Endpoint, Endpoint) {
        let host = self.base_url.host_str().unwrap_or("stalwart").to_owned();
        let endpoint = |port| Endpoint {
            host: host.clone(),
            port,
            security: Security::ImplicitTls,
            verify_certificate: false,
        };
        (endpoint(IMAPS_PORT), endpoint(SUBMISSIONS_PORT))
    }

    /// Creates a user `local_part@domain` with `password`, creating the domain first
    /// when the server does not have it. Returns Stalwart's id for the new account.
    ///
    /// # Errors
    /// Returns [`StalwartError::AddressTaken`] when the address exists, and
    /// [`StalwartError::Refused`] for any other failure.
    pub async fn create_mailbox(
        &self,
        local_part: &str,
        domain: &str,
        password: &SecretString,
    ) -> Result<String, StalwartError> {
        let account_id = self.admin_account_id().await?;
        let domain_id = self.ensure_domain(&account_id, domain).await?;

        let responses = self
            .call(json!([[
                "x:Account/set",
                {
                    "accountId": account_id,
                    "create": {
                        "mailbox": {
                            "@type": "User",
                            "name": local_part,
                            "domainId": domain_id,
                            "credentials": {
                                "0": { "@type": "Password", "secret": password.expose_secret() }
                            }
                        }
                    }
                },
                "create"
            ]]))
            .await?;

        let result = &responses[0];
        if let Some(created) = result["created"]["mailbox"]["id"].as_str() {
            return Ok(created.to_owned());
        }
        if result["notCreated"]["mailbox"]["type"] == "primaryKeyViolation" {
            return Err(StalwartError::AddressTaken);
        }
        Err(StalwartError::Refused(format!(
            "account not created: {}",
            result["notCreated"]["mailbox"]
        )))
    }

    /// Destroys an account and the mail in it. An account that is already gone counts
    /// as destroyed.
    ///
    /// # Errors
    /// Returns [`StalwartError::Refused`] when the server refuses or is unreachable.
    pub async fn destroy_mailbox(&self, stalwart_account_id: &str) -> Result<(), StalwartError> {
        let account_id = self.admin_account_id().await?;
        let responses = self
            .call(json!([[
                "x:Account/set",
                { "accountId": account_id, "destroy": [stalwart_account_id] },
                "destroy"
            ]]))
            .await?;

        let result = &responses[0];
        let destroyed = result["destroyed"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id == stalwart_account_id));
        let already_gone = result["notDestroyed"][stalwart_account_id]["type"] == "notFound";
        if destroyed || already_gone {
            return Ok(());
        }
        Err(StalwartError::Refused(format!(
            "account not destroyed: {}",
            result["notDestroyed"][stalwart_account_id]
        )))
    }

    /// The domain's id, creating it when absent. The first domain also becomes the
    /// server's default, which Stalwart needs before it will name itself in mail.
    async fn ensure_domain(&self, account_id: &str, domain: &str) -> Result<String, StalwartError> {
        let responses = self
            .call(json!([
                ["x:Domain/query", { "accountId": account_id, "filter": { "name": domain } }, "query"],
                ["x:SystemSettings/get", { "accountId": account_id, "ids": ["singleton"] }, "settings"]
            ]))
            .await?;

        if let Some(existing) = responses[0]["ids"][0].as_str() {
            return Ok(existing.to_owned());
        }

        let created = self
            .call(json!([[
                "x:Domain/set",
                { "accountId": account_id, "create": { "domain": { "name": domain } } },
                "create"
            ]]))
            .await?;
        let Some(domain_id) = created[0]["created"]["domain"]["id"].as_str() else {
            return Err(StalwartError::Refused(format!(
                "domain not created: {}",
                created[0]["notCreated"]["domain"]
            )));
        };

        let hostname = responses[1]["list"][0]["defaultHostname"]
            .as_str()
            .unwrap_or_default();
        if hostname.is_empty() {
            let updated = self
                .call(json!([[
                    "x:SystemSettings/set",
                    {
                        "accountId": account_id,
                        "update": {
                            "singleton": { "defaultHostname": format!("mail.{domain}"), "defaultDomainId": domain_id }
                        }
                    },
                    "settings"
                ]]))
                .await?;
            if updated[0]["updated"].get("singleton").is_none() {
                return Err(StalwartError::Refused(format!(
                    "server settings not updated: {}",
                    updated[0]["notUpdated"]["singleton"]
                )));
            }
        }

        Ok(domain_id.to_owned())
    }

    /// The administrator's JMAP account id, which every management call names.
    async fn admin_account_id(&self) -> Result<String, StalwartError> {
        let session: Value = self
            .http
            .get(
                self.base_url
                    .join("jmap/session")
                    .expect("a static path joins"),
            )
            .basic_auth(&self.admin_user, Some(self.admin_password.expose_secret()))
            .timeout(STALWART_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| StalwartError::Refused(error.to_string()))?
            .json()
            .await
            .map_err(|error| StalwartError::Refused(error.to_string()))?;

        session["primaryAccounts"]["urn:stalwart:jmap"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                StalwartError::Refused("the session grants no management account".to_owned())
            })
    }

    /// Sends method calls and returns each call's arguments, in order.
    async fn call(&self, method_calls: Value) -> Result<Vec<Value>, StalwartError> {
        let reply: Value = self
            .http
            .post(self.base_url.join("jmap/").expect("a static path joins"))
            .basic_auth(&self.admin_user, Some(self.admin_password.expose_secret()))
            .json(&json!({ "using": USING, "methodCalls": method_calls }))
            .timeout(STALWART_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| StalwartError::Refused(error.to_string()))?
            .json()
            .await
            .map_err(|error| StalwartError::Refused(error.to_string()))?;

        method_responses(&reply)
    }
}

/// Unwraps `methodResponses`, turning a JMAP method-level `error` into a refusal.
fn method_responses(reply: &Value) -> Result<Vec<Value>, StalwartError> {
    let Some(responses) = reply["methodResponses"].as_array() else {
        return Err(StalwartError::Refused(format!(
            "not a JMAP response: {reply}"
        )));
    };

    let mut arguments = Vec::with_capacity(responses.len());
    for response in responses {
        if response[0] == "error" {
            return Err(StalwartError::Refused(format!(
                "JMAP error: {}",
                response[1]
            )));
        }
        arguments.push(response[1].clone());
    }
    Ok(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_level_errors_become_refusals() {
        let reply = json!({ "methodResponses": [
            ["x:Domain/query", { "ids": ["b"] }, "0"],
            ["error", { "type": "forbidden" }, "1"]
        ]});
        let error = method_responses(&reply).expect_err("the second call failed");
        assert!(error.to_string().contains("forbidden"));

        let fine = json!({ "methodResponses": [["x:Domain/query", { "ids": ["b"] }, "0"]] });
        assert_eq!(method_responses(&fine).expect("ok")[0]["ids"][0], "b");
    }

    #[test]
    fn self_hosted_endpoints_use_the_servers_implicit_tls_listeners() {
        let stalwart = Stalwart::new(
            reqwest::Client::new(),
            "http://stalwart:8080/".parse().expect("url"),
            "elysium".to_owned(),
            SecretString::from("password"),
        );
        let (imap, smtp) = stalwart.endpoints();
        assert_eq!((imap.host.as_str(), imap.port), ("stalwart", 993));
        assert_eq!((smtp.host.as_str(), smtp.port), ("stalwart", 465));
        assert!(!imap.verify_certificate && !smtp.verify_certificate);
    }
}
