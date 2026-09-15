// Copyright © 2026 Jalapeno Labs

//! Administers the Stalwart mail server over its JMAP management API.
//!
//! Stalwart keeps every setting (domains, accounts, listeners) as a JMAP object in
//! its own datastore, managed through `x:<Object>/get|query|set` calls at `/jmap/`. The
//! container itself is run by [`container`].
//!
//! A new Stalwart starts in bootstrap mode: only the management listener runs, and a
//! temporary administrator named `admin` is printed to its log.
//! [`Stalwart::complete_setup`] finishes the setup with that password, which names the
//! server, gives it its first domain, and issues the permanent [`Administrator`] the API
//! signs in as from then on. Stalwart leaves bootstrap mode when it is next restarted.
//!
//! One server hosts any number of domains on one set of listeners. Every domain's MX
//! record points at the server's hostname, and Stalwart generates each domain's DKIM
//! keys and the DNS records it needs, see [`Stalwart::domain_zone_file`].
//!
//! Mail itself never goes through here. Once a mailbox exists it is reached over
//! IMAP and SMTP like any other, see [`super::transport`].

pub mod container;

use std::net::IpAddr;
use std::time::Duration;

use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use super::transport::{Endpoint, Security};

/// The name the API reaches Stalwart by, on the network [`container`] attaches both to.
pub const STALWART_HOST: &str = "stalwart";

/// Stalwart's management listener, plain HTTP on the private mail network.
const MANAGEMENT_URL: &str = "http://stalwart:8080/jmap/";
const SESSION_URL: &str = "http://stalwart:8080/jmap/session";

/// The temporary administrator a Stalwart in bootstrap mode prints to its log.
const BOOTSTRAP_ADMIN_USER: &str = "admin";

/// JMAP capabilities every management call declares.
const USING: [&str; 2] = ["urn:ietf:params:jmap:core", "urn:stalwart:jmap"];

/// Management calls are local and small; anything slower means Stalwart is down.
const STALWART_TIMEOUT: Duration = Duration::from_secs(10);

/// How long to wait for a starting Stalwart to answer: a new container before setup, or
/// a restarted one after it. Either takes a second or two.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

/// How often to ask whether a starting server answers yet.
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(500);

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
}

impl Stalwart {
    pub const fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Finishes a bootstrap-mode server's setup with its temporary password, naming it
    /// `hostname` with `domain` as its first domain, and returns the permanent
    /// administrator.
    ///
    /// Stalwart shows the permanent administrator's password only in this response, so
    /// the caller must store it before anything else can fail.
    ///
    /// Waits for the server to answer first, since it is called as the container starts.
    ///
    /// # Errors
    /// Returns [`StalwartError::Refused`] when the server does not answer to the
    /// temporary password in time, which a server already set up never does, or refuses
    /// the setup.
    pub async fn complete_setup(
        &self,
        bootstrap_password: &SecretString,
        hostname: &str,
        domain: &str,
    ) -> Result<Administrator, StalwartError> {
        let bootstrap = Administrator {
            username: BOOTSTRAP_ADMIN_USER.to_owned(),
            password: bootstrap_password.clone(),
        };
        // The container has only just started when its password is known.
        self.wait_until_serving(&bootstrap).await?;
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
                                "serverHostname": hostname,
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

    /// Waits for a starting server to answer and accept `administrator`.
    ///
    /// # Errors
    /// Returns [`StalwartError::Refused`] when it does not within [`STARTUP_TIMEOUT`].
    pub async fn wait_until_serving(
        &self,
        administrator: &Administrator,
    ) -> Result<(), StalwartError> {
        let serving = async {
            // While it starts nothing answers, and a server still in bootstrap mode
            // refuses an administrator issued by setup.
            while self.verify(administrator).await.is_err() {
                tokio::time::sleep(STARTUP_POLL_INTERVAL).await;
            }
        };
        tokio::time::timeout(STARTUP_TIMEOUT, serving)
            .await
            .map_err(|_elapsed| {
                StalwartError::Refused(format!(
                    "the server did not start within {} seconds",
                    STARTUP_TIMEOUT.as_secs()
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

    /// Creates a user `local_part` on the domain Stalwart knows as `domain_id`, with
    /// `password`. Returns Stalwart's id for the new account.
    ///
    /// # Errors
    /// Returns [`StalwartError::AddressTaken`] when the address exists, and
    /// [`StalwartError::Refused`] for any other failure.
    pub async fn create_mailbox(
        &self,
        administrator: &Administrator,
        local_part: &str,
        domain_id: &str,
        password: &SecretString,
    ) -> Result<String, StalwartError> {
        let account_id = self.admin_account_id(administrator).await?;

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

        destroyed_or_refusal(&responses[0], stalwart_account_id)
            .map_err(|refusal| StalwartError::Refused(format!("account not destroyed: {refusal}")))
    }

    /// Stalwart's id for `domain`, creating the domain when the server does not have it.
    /// Setup creates the first domain itself, so adding that one finds it.
    ///
    /// # Errors
    /// Returns [`StalwartError::Refused`] when the server refuses or is unreachable.
    pub async fn ensure_domain(
        &self,
        administrator: &Administrator,
        domain: &str,
    ) -> Result<String, StalwartError> {
        let account_id = self.admin_account_id(administrator).await?;
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

    /// Removes a domain and its DKIM keys. A domain that is already gone counts as
    /// removed.
    ///
    /// Stalwart refuses to remove a domain other objects still link to. Its DKIM keys
    /// always do, and exist only for the domain, so they are removed with it. Any other
    /// link, such as an account, is left in place and the removal refused.
    ///
    /// # Errors
    /// Returns [`StalwartError::Refused`] when the server refuses, which it does for its
    /// default domain, or is unreachable.
    pub async fn destroy_domain(
        &self,
        administrator: &Administrator,
        domain_id: &str,
    ) -> Result<(), StalwartError> {
        let account_id = self.admin_account_id(administrator).await?;
        let destroy = json!([[
            "x:Domain/set",
            { "accountId": account_id, "destroy": [domain_id] },
            "destroy"
        ]]);

        let responses = self.call(administrator, destroy.clone()).await?;
        let refusal = match destroyed_or_refusal(&responses[0], domain_id) {
            Ok(()) => return Ok(()),
            Err(refusal) => refusal,
        };

        let Some(dkim_keys) = linked_dkim_keys(&refusal) else {
            return Err(StalwartError::Refused(format!(
                "domain not removed: {refusal}"
            )));
        };
        let removed = self
            .call(
                administrator,
                json!([[
                    "x:DkimSignature/set",
                    { "accountId": account_id, "destroy": dkim_keys },
                    "keys"
                ]]),
            )
            .await?;
        if removed[0]["notDestroyed"]
            .as_object()
            .is_some_and(|failures| !failures.is_empty())
        {
            return Err(StalwartError::Refused(format!(
                "domain keys not removed: {}",
                removed[0]["notDestroyed"]
            )));
        }

        let responses = self.call(administrator, destroy).await?;
        destroyed_or_refusal(&responses[0], domain_id)
            .map_err(|refusal| StalwartError::Refused(format!("domain not removed: {refusal}")))
    }

    /// Makes `proxy` the only address Stalwart accepts a PROXY protocol header from, or
    /// no address when `None`. Returns whether that changed anything.
    ///
    /// The header carries a client's real address through nginx. Stalwart then requires it
    /// on every listener, management included, from every trusted address, so exactly
    /// nginx's address is trusted: the API, connecting directly, must not be. A change
    /// applies when the server next starts; restarting is the caller's job.
    ///
    /// # Errors
    /// Returns [`StalwartError::Refused`] when the server refuses or is unreachable.
    pub async fn trust_proxy(
        &self,
        administrator: &Administrator,
        proxy: Option<IpAddr>,
    ) -> Result<bool, StalwartError> {
        let account_id = self.admin_account_id(administrator).await?;
        let current = self
            .call(
                administrator,
                json!([[
                    "x:SystemSettings/get",
                    { "accountId": account_id, "ids": ["singleton"], "properties": ["proxyTrustedNetworks"] },
                    "settings"
                ]]),
            )
            .await?;

        let desired = trusted_networks(proxy);
        if same_networks(&current[0]["list"][0]["proxyTrustedNetworks"], &desired) {
            return Ok(false);
        }

        let updated = self
            .call(
                administrator,
                json!([[
                    "x:SystemSettings/set",
                    {
                        "accountId": account_id,
                        "update": { "singleton": { "proxyTrustedNetworks": desired } }
                    },
                    "settings"
                ]]),
            )
            .await?;
        if updated[0]["updated"].get("singleton").is_none() {
            return Err(StalwartError::Refused(format!(
                "trusted proxy not updated: {}",
                updated[0]["notUpdated"]["singleton"]
            )));
        }
        Ok(true)
    }

    /// Every DNS record Stalwart recommends for a domain, as a BIND zone file: MX, SPF,
    /// DKIM, DMARC, and service discovery. DKIM keys are generated a few seconds after a
    /// domain is added, so a new domain's file may lack them at first.
    ///
    /// # Errors
    /// Returns [`StalwartError::Refused`] when the server refuses or is unreachable.
    pub async fn domain_zone_file(
        &self,
        administrator: &Administrator,
        domain_id: &str,
    ) -> Result<String, StalwartError> {
        let account_id = self.admin_account_id(administrator).await?;
        let responses = self
            .call(
                administrator,
                json!([[
                    "x:Domain/get",
                    { "accountId": account_id, "ids": [domain_id], "properties": ["dnsZoneFile"] },
                    "get"
                ]]),
            )
            .await?;
        responses[0]["list"][0]["dnsZoneFile"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                StalwartError::Refused(format!("domain not found: {}", responses[0]["notFound"]))
            })
    }

    /// The administrator's JMAP account id, which every management call names.
    async fn admin_account_id(
        &self,
        administrator: &Administrator,
    ) -> Result<String, StalwartError> {
        let request = self.http.get(SESSION_URL);
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
            .post(MANAGEMENT_URL)
            .json(&json!({ "using": USING, "methodCalls": method_calls }));
        let reply = send(request, administrator).await?;

        method_responses(&reply)
    }
}

/// The IMAP and SMTP servers self-hosted mailboxes connect to.
pub fn endpoints() -> (Endpoint, Endpoint) {
    let endpoint = |port| Endpoint {
        host: STALWART_HOST.to_owned(),
        port,
        security: Security::ImplicitTls,
        verify_certificate: false,
    };
    (endpoint(IMAPS_PORT), endpoint(SUBMISSIONS_PORT))
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

/// Stalwart's `proxyTrustedNetworks` value trusting exactly `proxy`.
fn trusted_networks(proxy: Option<IpAddr>) -> Value {
    let mut networks = serde_json::Map::new();
    if let Some(address) = proxy {
        let prefix = if address.is_ipv4() { 32 } else { 128 };
        networks.insert(format!("{address}/{prefix}"), Value::Bool(true));
    }
    Value::Object(networks)
}

/// Whether two `proxyTrustedNetworks` values name the same networks. Stalwart reports a
/// single address without its `/32` or `/128`.
fn same_networks(current: &Value, desired: &Value) -> bool {
    let normalized = |value: &Value| -> Vec<String> {
        let mut networks: Vec<String> = value
            .as_object()
            .map(|networks| {
                networks
                    .keys()
                    .map(|network| {
                        network
                            .trim_end_matches("/32")
                            .trim_end_matches("/128")
                            .to_owned()
                    })
                    .collect()
            })
            .unwrap_or_default();
        networks.sort();
        networks
    };
    normalized(current) == normalized(desired)
}

/// `Ok` when a `/set` result destroyed `id` or found it already gone, else the reason
/// Stalwart gave.
fn destroyed_or_refusal(result: &Value, id: &str) -> Result<(), Value> {
    let destroyed = result["destroyed"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|destroyed_id| destroyed_id == id));
    let reason = &result["notDestroyed"][id];
    if destroyed || reason["type"] == "notFound" {
        return Ok(());
    }
    Err(reason.clone())
}

/// The DKIM keys behind an `objectIsLinked` refusal, when they are all that links to the
/// object. `None` when anything else does too.
fn linked_dkim_keys(refusal: &Value) -> Option<Vec<String>> {
    if refusal["type"] != "objectIsLinked" {
        return None;
    }
    let mut keys = Vec::new();
    for linked in refusal["linkedObjects"].as_array()? {
        if linked["object"] != "DkimSignature" {
            return None;
        }
        keys.push(linked["id"].as_str()?.to_owned());
    }
    Some(keys)
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
    fn exactly_the_proxy_address_is_trusted() {
        let nginx: IpAddr = "172.29.53.2".parse().expect("address");
        let desired = trusted_networks(Some(nginx));
        assert_eq!(desired, json!({ "172.29.53.2/32": true }));

        // Stalwart reports the address it stored without the prefix.
        assert!(same_networks(&json!({ "172.29.53.2": true }), &desired));
        assert!(!same_networks(&json!({}), &desired));
        assert!(!same_networks(&json!({ "172.29.53.0/24": true }), &desired));
        assert!(same_networks(&Value::Null, &trusted_networks(None)));
    }

    #[test]
    fn only_dkim_keys_are_removed_along_with_a_domain() {
        let keys_only = json!({
            "type": "objectIsLinked",
            "linkedObjects": [
                { "id": "je7uucuzksqa", "object": "DkimSignature" },
                { "id": "je7uucyhktaa", "object": "DkimSignature" }
            ]
        });
        assert_eq!(
            linked_dkim_keys(&keys_only),
            Some(vec!["je7uucuzksqa".to_owned(), "je7uucyhktaa".to_owned()])
        );

        let with_account = json!({
            "type": "objectIsLinked",
            "linkedObjects": [
                { "id": "je7uucuzksqa", "object": "DkimSignature" },
                { "id": "d", "object": "Account" }
            ]
        });
        assert_eq!(linked_dkim_keys(&with_account), None);
        assert_eq!(linked_dkim_keys(&json!({ "type": "forbidden" })), None);
    }

    #[test]
    fn a_missing_object_counts_as_destroyed() {
        let gone = json!({ "notDestroyed": { "c": { "type": "notFound" } } });
        destroyed_or_refusal(&gone, "c").expect("already gone");
        let destroyed = json!({ "destroyed": ["c"] });
        destroyed_or_refusal(&destroyed, "c").expect("destroyed");
        let linked = json!({ "notDestroyed": { "c": { "type": "objectIsLinked" } } });
        assert_eq!(
            destroyed_or_refusal(&linked, "c").expect_err("refused")["type"],
            "objectIsLinked"
        );
    }

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
        let (imap, smtp) = endpoints();
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
