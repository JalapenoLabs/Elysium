// Copyright © 2026 Jalapeno Labs

//! Runs the Stalwart container through Docker.
//!
//! Stalwart is not part of `compose.yml`: the API creates it the first time a mail
//! server is set up, and keeps it running from then on. Everything the API creates is
//! named here and labelled [`MANAGED_LABEL`]:
//!
//! | Kind      | Name                      | Purpose                                         |
//! |-----------|---------------------------|-------------------------------------------------|
//! | network   | `elysium-mail`            | Joins the API and Stalwart, and nothing else    |
//! | volume    | `elysium-stalwart-config` | `/etc/stalwart`, written by setup               |
//! | volume    | `elysium-stalwart-data`   | `/var/lib/stalwart`, every domain, account, and message |
//! | container | `elysium-stalwart`        | The server, reachable as `stalwart` on the network |
//!
//! The network belongs to the API rather than to compose, so `docker compose down` can
//! remove the stack's own network while Stalwart keeps running. The API attaches its
//! own container to the network at startup, since a recreated API container starts
//! without it.
//!
//! Docker is reached through the filtered socket proxy in `compose.yml`, which allows
//! the container, image, volume, and network endpoints and nothing else.

use std::collections::HashMap;
use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{
    ContainerCreateBody, EndpointSettings, HostConfig, NetworkConnectRequest, NetworkCreateRequest,
    NetworkingConfig, RestartPolicy, RestartPolicyNameEnum, VolumeCreateRequest,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, LogsOptionsBuilder,
};
use futures_util::StreamExt;
use secrecy::SecretString;
use tracing::{Level, event};

use super::STALWART_HOST;

/// The image every Elysium mail server runs. Pinned: a new Stalwart release changes the
/// management API this module and [`super::Stalwart`] depend on.
const IMAGE_REPOSITORY: &str = "stalwartlabs/stalwart";
const IMAGE_TAG: &str = "v0.16.22-alpine";

const CONTAINER_NAME: &str = "elysium-stalwart";
const NETWORK_NAME: &str = "elysium-mail";
const CONFIG_VOLUME: &str = "elysium-stalwart-config";
const DATA_VOLUME: &str = "elysium-stalwart-data";

/// Marks what the API created, so an operator can find it with `docker ps --filter label=...`.
const MANAGED_LABEL: (&str, &str) = ("dev.elysium.managed", "mail-server");

/// How long a new container may take to print its temporary administrator. It prints
/// within a second or two of starting; the rest covers a slow disk.
const BOOTSTRAP_BANNER_TIMEOUT: Duration = Duration::from_secs(60);
const BOOTSTRAP_BANNER_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// The line Stalwart prints above its temporary administrator's credentials.
const BOOTSTRAP_BANNER: &str = "temporary administrator account";

/// Docker refused a call, or the container did not behave as a new Stalwart does.
#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("Docker: {0}")]
    Docker(#[from] DockerError),
    /// The API cannot name its own container, so it cannot join the mail network.
    #[error("the API is not running in a Docker container, so it cannot reach the mail server")]
    NotContainerized,
    #[error(
        "the mail server printed no setup password; if its volumes hold a server set up before, remove \
         the {CONTAINER_NAME} container and the {CONFIG_VOLUME} and {DATA_VOLUME} volumes to start over"
    )]
    NoBootstrapPassword,
}

#[derive(Debug, Clone)]
pub struct StalwartContainer {
    docker: Docker,
}

impl StalwartContainer {
    pub const fn new(docker: Docker) -> Self {
        Self { docker }
    }

    /// Creates the mail network when absent and attaches this API's container to it, so
    /// `stalwart` resolves from here.
    ///
    /// # Errors
    /// [`ContainerError::NotContainerized`] outside Docker, and
    /// [`ContainerError::Docker`] when Docker refuses.
    pub async fn attach_api(&self) -> Result<(), ContainerError> {
        // Docker sets a container's hostname to its short id unless compose names one.
        let own_container =
            std::env::var("HOSTNAME").map_err(|_missing| ContainerError::NotContainerized)?;

        if let Err(error) = self.docker.inspect_network(NETWORK_NAME, None).await {
            if !has_status(&error, 404) {
                return Err(error.into());
            }
            let created = self
                .docker
                .create_network(NetworkCreateRequest {
                    name: NETWORK_NAME.to_owned(),
                    driver: Some("bridge".to_owned()),
                    labels: Some(managed_labels()),
                    ..Default::default()
                })
                .await;
            // Another API replica may have created it in the meantime.
            if let Err(error) = created
                && !has_status(&error, 409)
            {
                return Err(error.into());
            }
        }

        let own = self.docker.inspect_container(&own_container, None).await?;
        let attached = own
            .network_settings
            .and_then(|settings| settings.networks)
            .is_some_and(|networks| networks.contains_key(NETWORK_NAME));
        if attached {
            return Ok(());
        }

        self.docker
            .connect_network(
                NETWORK_NAME,
                NetworkConnectRequest {
                    container: own_container,
                    endpoint_config: None,
                },
            )
            .await?;
        event!(
            name: "mail.container.api_attached",
            Level::INFO,
            network.name = NETWORK_NAME,
            "attached the API to the mail network",
        );
        Ok(())
    }

    /// Pulls the pinned image unless Docker already has it.
    ///
    /// # Errors
    /// [`ContainerError::Docker`] when the pull fails.
    pub async fn ensure_image(&self) -> Result<(), ContainerError> {
        let image = format!("{IMAGE_REPOSITORY}:{IMAGE_TAG}");
        match self.docker.inspect_image(&image).await {
            Ok(_) => return Ok(()),
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }

        event!(name: "mail.container.pull_started", Level::INFO, container.image.name = %image, "pulling");
        let options = CreateImageOptionsBuilder::new()
            .from_image(IMAGE_REPOSITORY)
            .tag(IMAGE_TAG)
            .build();
        let mut progress = self.docker.create_image(Some(options), None, None);
        while let Some(update) = progress.next().await {
            update?;
        }
        Ok(())
    }

    /// Starts the container, creating it and its volumes first when it does not exist.
    ///
    /// # Errors
    /// [`ContainerError::Docker`] when Docker refuses.
    pub async fn ensure_running(&self) -> Result<(), ContainerError> {
        match self.docker.inspect_container(CONTAINER_NAME, None).await {
            Ok(existing) => {
                let running = existing
                    .state
                    .and_then(|state| state.running)
                    .unwrap_or(false);
                if !running {
                    self.docker.start_container(CONTAINER_NAME, None).await?;
                }
                return Ok(());
            }
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }

        for volume in [CONFIG_VOLUME, DATA_VOLUME] {
            self.docker
                .create_volume(VolumeCreateRequest {
                    name: Some(volume.to_owned()),
                    labels: Some(managed_labels()),
                    ..Default::default()
                })
                .await?;
        }

        let options = CreateContainerOptionsBuilder::new()
            .name(CONTAINER_NAME)
            .build();
        self.docker
            .create_container(Some(options), container_spec())
            .await?;
        self.docker.start_container(CONTAINER_NAME, None).await?;
        event!(name: "mail.container.created", Level::INFO, container.name = CONTAINER_NAME, "created");
        Ok(())
    }

    /// Waits for a new container to print its temporary administrator and returns the
    /// password. Stalwart prints a new one every time it starts before setup, so the
    /// latest one is the one that works.
    ///
    /// # Errors
    /// [`ContainerError::NoBootstrapPassword`] when none appears in time, which is what a
    /// container that was already set up does.
    pub async fn bootstrap_password(&self) -> Result<SecretString, ContainerError> {
        let deadline = tokio::time::Instant::now() + BOOTSTRAP_BANNER_TIMEOUT;
        loop {
            let logs = self.logs().await?;
            if let Some(password) = find_bootstrap_password(&logs) {
                return Ok(SecretString::from(password.to_owned()));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ContainerError::NoBootstrapPassword);
            }
            tokio::time::sleep(BOOTSTRAP_BANNER_POLL_INTERVAL).await;
        }
    }

    /// Restarts the container, which is how Stalwart leaves bootstrap mode after setup.
    ///
    /// # Errors
    /// [`ContainerError::Docker`] when Docker refuses.
    pub async fn restart(&self) -> Result<(), ContainerError> {
        self.docker.restart_container(CONTAINER_NAME, None).await?;
        Ok(())
    }

    /// Everything the container has written to stdout and stderr.
    async fn logs(&self) -> Result<String, ContainerError> {
        let options = LogsOptionsBuilder::new().stdout(true).stderr(true).build();
        let mut stream = self.docker.logs(CONTAINER_NAME, Some(options));
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            text.push_str(&String::from_utf8_lossy(&chunk?.into_bytes()));
        }
        Ok(text)
    }
}

/// The container Docker creates: the pinned image on the mail network, answering as
/// `stalwart`, with no published ports.
fn container_spec() -> ContainerCreateBody {
    ContainerCreateBody {
        image: Some(format!("{IMAGE_REPOSITORY}:{IMAGE_TAG}")),
        env: Some(vec!["TZ=UTC".to_owned()]),
        labels: Some(managed_labels()),
        host_config: Some(HostConfig {
            binds: Some(vec![
                format!("{CONFIG_VOLUME}:/etc/stalwart"),
                format!("{DATA_VOLUME}:/var/lib/stalwart"),
            ]),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            network_mode: Some(NETWORK_NAME.to_owned()),
            ..Default::default()
        }),
        networking_config: Some(NetworkingConfig {
            endpoints_config: Some(HashMap::from([(
                NETWORK_NAME.to_owned(),
                EndpointSettings {
                    aliases: Some(vec![STALWART_HOST.to_owned()]),
                    ..Default::default()
                },
            )])),
        }),
        ..Default::default()
    }
}

fn managed_labels() -> HashMap<String, String> {
    HashMap::from([(MANAGED_LABEL.0.to_owned(), MANAGED_LABEL.1.to_owned())])
}

fn has_status(error: &DockerError, status: u16) -> bool {
    matches!(error, DockerError::DockerResponseServerError { status_code, .. } if *status_code == status)
}

/// The password from the last temporary-administrator banner in `logs`.
fn find_bootstrap_password(logs: &str) -> Option<&str> {
    let (_before, latest_banner) = logs.rsplit_once(BOOTSTRAP_BANNER)?;
    latest_banner
        .lines()
        .find_map(|line| line.trim().strip_prefix("password:"))
        .map(str::trim)
        .filter(|password| !password.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two starts before setup, as Stalwart 0.16.22 prints them.
    const TWO_BOOTSTRAP_STARTS: &str = "
════════════════════════════════════════════════════════════
🔑 Stalwart bootstrap mode - temporary administrator account

   username: admin
   password: 9JNVNk9u6rA5BmwO

Use these credentials to complete the initial setup at the
════════════════════════════════════════════════════════════
2026-09-15T16:09:34Z WARN Server started in bootstrap mode
════════════════════════════════════════════════════════════
🔑 Stalwart bootstrap mode - temporary administrator account

   username: admin
   password: fuJ6t8pvRDdcZKdX

This password is shown only once. To pin a credential
";

    #[test]
    fn the_latest_bootstrap_password_is_the_one_that_works() {
        assert_eq!(
            find_bootstrap_password(TWO_BOOTSTRAP_STARTS),
            Some("fuJ6t8pvRDdcZKdX")
        );
    }

    #[test]
    fn logs_without_a_banner_have_no_password() {
        assert_eq!(
            find_bootstrap_password("2026-09-15T16:14:03Z INFO Metrics collected"),
            None
        );
    }

    #[test]
    fn the_container_publishes_no_ports_and_answers_as_stalwart() {
        let spec = container_spec();
        let host_config = spec.host_config.expect("host config");
        assert!(host_config.port_bindings.is_none());
        assert_eq!(host_config.network_mode.as_deref(), Some(NETWORK_NAME));
        let endpoint = &spec
            .networking_config
            .and_then(|config| config.endpoints_config)
            .expect("endpoints")[NETWORK_NAME];
        assert_eq!(
            endpoint.aliases.as_deref(),
            Some(&[STALWART_HOST.to_owned()][..])
        );
    }
}
