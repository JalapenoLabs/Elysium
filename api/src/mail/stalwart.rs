// Copyright © 2026 Jalapeno Labs

//! Administers the bundled Stalwart mail server over its JMAP management API.
//!
//! Stalwart keeps every setting (domains, accounts, listeners) as a JMAP object in
//! its own datastore, managed through `x:<Object>/get|query|set` calls at `/jmap/`.
//!
//! A new Stalwart starts in bootstrap mode: only the management listener runs, and a
//! temporary administrator named `admin` is printed once to its log. The settings page
//! takes that password and [`Stalwart::complete_setup`] finishes the setup with it,
//! which gives the server its domain and issues the permanent [`Administrator`] the
//! API signs in as from then on. Stalwart restarts once to leave bootstrap mode; the
//! entrypoint in `stalwart/entrypoint.sh` does that as soon as setup writes its
//! configuration file.
//!
//! Mail itself never goes through here. Once a mailbox exists it is reached over
//! IMAP and SMTP like any other, see [`super::transport`].

use std::time::Duration;

use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use url::Url;

use super::transport::{Endpoint, Security};

/// The temporary administrator a Stalwart in bootstrap mode prints to its log.
const BOOTSTRAP_ADMIN_USER: &str = "admin";

/// JMAP capabilities every management call declares.
const USING: [&str; 2] = ["urn:ietf:params:jmap:core", "urn:stalwart:jmap"];

/// Management calls are local and small; anything slower means Stalwart is down.
const STALWART_TIMEOUT: Duration = Duration::from_secs(10);

/// How long setup waits for Stalwart to restart out of bootstrap mode. It takes about
/// two seconds; this stays well inside the API's 30-second request timeout.
const RESTART_TIMEOUT: Duration = Duration::from_secs(20);

/// How often setup asks whether the restarted server accepts the new administrator.
const RESTART_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Stalwart's implicit-TLS listeners, present in every new installation.
const IMAPS_PORT: u16 = 993;
const SUBMISSIONS_PORT: u16 = 465;

/// Stalwart refused a management call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum StalwartError {
    /// Another account already holds the address.
    #[error("the mail server already has an account for that address")]
    AddressTaken,
    /// The server rejected the administrator's username or password.
    #[error("the mail server rejected the administrator credentials")]
    Unauthorized,
    #[error("mail server: {0}")]
    Refused(String),
}

/// Credentials for Stalwart's management API.
#[derive(Debug)]
pub struct Administrator {
    pub username: String,
    pub password: SecretString,
}

#[derive(Debug, Clone)]
pub struct Stalwart {
    http: reqwest::Client,
    base_url: Url,
}

impl Stalwart {
    pub const fn new(http: reqwest::Client, base_url: Url) -> Self {
        Self { http, base_url }
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

    /// Finishes a bootstrap-mode server's setup with its temporary password, making
    /// `domain` the server's default domain, and returns the permanent administrator.
    ///
    /// Stalwart shows the permanent administrator's password only in this response, so
    /// the caller must store it before anything else can fail.
    ///
    /// # Errors
    /// Returns [`StalwartError::Unauthorized`] when the temporary password is wrong or
    /// the server is already set up, and [`StalwartError::Refused`] otherwise.
    pub async fn complete_setup(
        &self,
        bootstrap_password: &SecretString,
        domain: &str,
    ) -> Result<Administrator, StalwartError> {
        let bootstrap = Administrator {
            username: BOOTSTRAP_ADMIN_USER.to_owned(),
            password: bootstrap_password.clone(),
        };
        let account_id = self.admin_account_id(&bootstrap).await?;

        let responses = self
            .call(
                &bootstrap,
                json!([[
                    "x:Bootstrap/set",
                    {
                        "accountId": account_id,
                        "update": {
                            "singleton": {
                                "serverHostname": format!("mail.{domain}"),
                                "defaultDomain": domain,
                                // Mail stays on this host, so no certificate authority can
                                // reach it. Stalwart serves a self-signed certificate instead.
                                "requestTlsCertificate": false,
                                // Ready for the day the domain gets DNS; harmless until then.
                                "generateDkimKeys": true,
                                // Stalwart's own default writes rotating files inside the
                                // container, where nobody reads them.
                                "tracer": { "@type": "Stdout", "level": "info" }
                            }
                        }
                    },
                    "setup"
                ]]),
            )
            .await?;

        let issued = &responses[0]["updated"]["singleton"];
        let (Some(username), Some(secret)) =
            (issued["username"].as_str(), issued["secret"].as_str())
        else {
            return Err(StalwartError::Refused(format!(
                "setup not completed: {}",
                responses[0]["notUpdated"]["singleton"]
            )));
        };
        Ok(Administrator {
            username: username.to_owned(),
            password: SecretString::from(secret.to_owned()),
        })
    }

    /// Waits for a just-configured server to restart and accept `administrator`.
    ///
    /// # Errors
    /// Returns [`StalwartError::Refused`] when the server is not back within
    /// [`RESTART_TIMEOUT`].
    pub async fn wait_until_serving(
        &self,
        administrator: &Administrator,
    ) -> Result<(), StalwartError> {
        let serving = async {
            // Until the restart the server is still in bootstrap mode, which refuses the
            // new administrator; while it restarts, nothing answers at all.
            while self.verify(administrator).await.is_err() {
                tokio::time::sleep(RESTART_POLL_INTERVAL).await;
            }
        };
        tokio::time::timeout(RESTART_TIMEOUT, serving)
            .await
            .map_err(|_elapsed| {
                StalwartError::Refused(format!(
                    "the server did not restart within {} seconds of its setup",
                    RESTART_TIMEOUT.as_secs()
                ))
            })
    }

    /// Checks that the server answers and accepts `administrator`.
    ///
    /// # Errors
    /// Returns [`StalwartError::Unauthorized`] for rejected credentials and
    /// [`StalwartError::Refused`] when the server cannot be reached.
    pub async fn verify(&self, administrator: &Administrator) -> Result<(), StalwartError> {
        self.admin_account_id(administrator).await.map(drop)
    }

    /// Creates a user `local_part@domain` with `password`, creating the domain first
    /// when the server does not have it. Returns Stalwart's id for the new account.
    ///
    /// # Errors
    /// Returns [`StalwartError::AddressTaken`] when the address exists, and
    /// [`StalwartError::Refused`] for any other failure.
    pub async fn create_mailbox(
        &self,
        administrator: &Administrator,
        local_part: &str,
        domain: &str,
        password: &SecretString,
    ) -> Result<String, StalwartError> {
        let account_id = self.admin_account_id(administrator).await?;
        let domain_id = self
            .ensure_domain(administrator, &account_id, domain)
            .await?;

        let responses = self
            .call(
                administrator,
                json!([[
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
                ]]),
            )
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
    pub async fn destroy_mailbox(
        &self,
        administrator: &Administrator,
        stalwart_account_id: &str,
    ) -> Result<(), StalwartError> {
        let account_id = self.admin_account_id(administrator).await?;
        let responses = self
            .call(
                administrator,
                json!([[
                    "x:Account/set",
                    { "accountId": account_id, "destroy": [stalwart_account_id] },
                    "destroy"
                ]]),
            )
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

    /// The domain's id, creating it when absent. Setup already created the server's
    /// default domain; any other domain is added the first time a mailbox names it.
    async fn ensure_domain(
        &self,
        administrator: &Administrator,
        account_id: &str,
        domain: &str,
    ) -> Result<String, StalwartError> {
        let found = self
            .call(
                administrator,
                json!([[
                    "x:Domain/query",
                    { "accountId": account_id, "filter": { "name": domain } },
                    "query"
                ]]),
            )
            .await?;
        if let Some(existing) = found[0]["ids"][0].as_str() {
            return Ok(existing.to_owned());
        }

        let created = self
            .call(
                administrator,
                json!([[
                    "x:Domain/set",
                    { "accountId": account_id, "create": { "domain": { "name": domain } } },
                    "create"
                ]]),
            )
            .await?;
        created[0]["created"]["domain"]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                StalwartError::Refused(format!(
                    "domain not created: {}",
                    created[0]["notCreated"]["domain"]
                ))
            })
    }

    /// The administrator's JMAP account id, which every management call names.
    async fn admin_account_id(
        &self,
        administrator: &Administrator,
    ) -> Result<String, StalwartError> {
        let request = self.http.get(
            self.base_url
                .join("jmap/session")
                .expect("a static path joins"),
        );
        let session = send(request, administrator).await?;

        session["primaryAccounts"]["urn:stalwart:jmap"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                StalwartError::Refused("the session grants no management account".to_owned())
            })
    }

    /// Sends method calls and returns each call's arguments, in order.
    async fn call(
        &self,
        administrator: &Administrator,
        method_calls: Value,
    ) -> Result<Vec<Value>, StalwartError> {
        let request = self
            .http
            .post(self.base_url.join("jmap/").expect("a static path joins"))
            .json(&json!({ "using": USING, "methodCalls": method_calls }));
        let reply = send(request, administrator).await?;

        method_responses(&reply)
    }
}

/// Authenticates and sends a management request, reading the JSON reply.
async fn send(
    request: reqwest::RequestBuilder,
    administrator: &Administrator,
) -> Result<Value, StalwartError> {
    let response = request
        .basic_auth(
            &administrator.username,
            Some(administrator.password.expose_secret()),
        )
        .timeout(STALWART_TIMEOUT)
        .send()
        .await
        .map_err(|error| StalwartError::Refused(error.to_string()))?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(StalwartError::Unauthorized);
    }
    response
        .error_for_status()
        .map_err(|error| StalwartError::Refused(error.to_string()))?
        .json()
        .await
        .map_err(|error| StalwartError::Refused(error.to_string()))
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
        );
        let (imap, smtp) = stalwart.endpoints();
        assert_eq!((imap.host.as_str(), imap.port), ("stalwart", 993));
        assert_eq!((smtp.host.as_str(), smtp.port), ("stalwart", 465));
        assert!(!imap.verify_certificate && !smtp.verify_certificate);
    }

    #[test]
    fn administrators_never_print_their_password() {
        let administrator = Administrator {
            username: "admin@elysium.local".to_owned(),
            password: SecretString::from("issued-at-setup"),
        };
        let rendered = format!("{administrator:?}");
        assert!(rendered.contains("admin@elysium.local"));
        assert!(!rendered.contains("issued-at-setup"));
    }
}
