// Copyright © 2026 Jalapeno Labs

//! Each worker's container, volume, and network in Docker, named after its batch.
//!
//! | Kind      | Name                       | Purpose                                                  |
//! |-----------|----------------------------|----------------------------------------------------------|
//! | network   | `elysium-worker-<batch>`   | The worker's own bridge to the internet; the API joins it as `api` |
//! | volume    | `elysium-worker-<batch>`   | `/workspace`: its clones, `~/.claude`, and its session's transcript |
//! | container | `elysium-worker-<batch>`   | The workspace image, running the batch's turns            |
//!
//! A network per worker is what keeps workers apart: none reaches another worker, the shared
//! workspace, or any compose network (Postgres, Redis, `docker-proxy`, Kratos, Hydra). The only
//! service of Elysium's on it is the API, for agents' tool calls at `http://api:8080`. Compose
//! drops that join whenever it recreates the API, so [`Containers::ensure_network`] joins every
//! replica again on each of the scheduler's looks, as the browser reconciler does for browsers.
//!
//! Everything is labelled as a worker's, with its batch and this deployment
//! (`crate::browser::container::deployment_label`), so [`Containers::leftovers`] finds what a
//! worker left behind and never touches another deployment's on a shared Docker host.

use std::collections::{HashMap, HashSet};

use anyhow::Context;
use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{
    EndpointSettings, NetworkConnectRequest, NetworkCreateRequest, NetworkDisconnectRequest,
};
use bollard::query_parameters::{
    ListContainersOptionsBuilder, ListNetworksOptionsBuilder, ListVolumesOptionsBuilder,
};
use secrecy::SecretString;
use tracing::{Level, event};
use uuid::Uuid;

use crate::browser::container::deployment_label;
use crate::compose;
use crate::runner::workspace::{WorkerContainer, WorkspaceContainer};

const MANAGED_LABEL: (&str, &str) = ("dev.elysium.managed", "worker");
const BATCH_LABEL: &str = "dev.elysium.work-batch";
/// Which deployment created the resource, as browsers are labelled.
const DEPLOYMENT_LABEL: &str = "dev.elysium.deployment";

/// What the API answers as on a worker's network, so agents reach Elysium's tools at
/// `crate::runner::AGENT_API_URL` as they do from the shared workspace.
const API_ALIAS: &str = "api";

/// The gateway priority the API joins a worker's network with: below its own compose networks',
/// so a worker's network never becomes its default route. Docker Engine 28.0 or later honors it,
/// as browsers already need.
const JOINED_GATEWAY_PRIORITY: i64 = -1;

/// The name a worker's network, volume, and container share.
pub fn name(batch_id: Uuid) -> String {
    format!("elysium-worker-{batch_id}")
}

/// Workers' resources in Docker. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Containers {
    docker: Docker,
    /// The workspace image of this release, which every worker runs.
    image: String,
    /// This deployment's [`DEPLOYMENT_LABEL`] value.
    deployment: String,
}

impl Containers {
    pub fn new(docker: Docker, image: String, encryption_key: &SecretString) -> Self {
        Self {
            docker,
            image,
            deployment: deployment_label(encryption_key),
        }
    }

    /// Batch `batch_id`'s worker container, limited to `nano_cpus` billionths of a CPU. Nothing
    /// reaches Docker until it is prepared or worked in.
    pub fn workspace(&self, batch_id: Uuid, nano_cpus: i64) -> WorkspaceContainer {
        WorkspaceContainer::worker(
            self.docker.clone(),
            self.image.clone(),
            WorkerContainer {
                name: name(batch_id),
                labels: self.labels(batch_id),
                nano_cpus,
            },
        )
    }

    /// Every replica of the API, which joins each worker's network.
    ///
    /// # Errors
    /// Describes Docker's refusal, or an engine not in a container it can find.
    pub async fn api_replicas(&self) -> anyhow::Result<Vec<String>> {
        Ok(compose::api_replicas(&self.docker).await?)
    }

    /// Creates batch `batch_id`'s network unless it exists, and joins each of the API's
    /// `replicas` to it that is not joined yet. Cheap when both are in place, so it runs on every
    /// look.
    ///
    /// # Errors
    /// Describes Docker's refusal, such as a daemon out of address pools for another network.
    pub async fn ensure_network(&self, batch_id: Uuid, replicas: &[String]) -> anyhow::Result<()> {
        let name = name(batch_id);
        let network = match self.docker.inspect_network(&name, None).await {
            Ok(network) => network,
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {
                self.docker
                    .create_network(NetworkCreateRequest {
                        name: name.clone(),
                        driver: Some("bridge".to_owned()),
                        labels: Some(self.labels(batch_id)),
                        ..Default::default()
                    })
                    .await
                    .with_context(|| format!("cannot create the worker's network {name}"))?;
                self.docker.inspect_network(&name, None).await?
            }
        };
        let joined = network.containers.unwrap_or_default();
        for replica in replicas {
            if joined.contains_key(replica) {
                continue;
            }
            self.docker
                .connect_network(
                    &name,
                    NetworkConnectRequest {
                        container: replica.clone(),
                        endpoint_config: Some(EndpointSettings {
                            aliases: Some(vec![API_ALIAS.to_owned()]),
                            gw_priority: Some(JOINED_GATEWAY_PRIORITY),
                            ..Default::default()
                        }),
                    },
                )
                .await?;
        }
        Ok(())
    }

    /// Removes batch `batch_id`'s container, volume, and network. What is already gone is
    /// skipped, so a half-removed worker can be removed again.
    ///
    /// # Errors
    /// Propagates Docker's refusal.
    pub async fn remove(&self, batch_id: Uuid) -> anyhow::Result<()> {
        self.workspace(batch_id, 0).remove_with_volume().await?;
        let name = name(batch_id);
        match self.docker.inspect_network(&name, None).await {
            Ok(network) => {
                for container in network.containers.unwrap_or_default().into_keys() {
                    let request = NetworkDisconnectRequest {
                        container,
                        force: Some(true),
                    };
                    self.docker.disconnect_network(&name, request).await?;
                }
                self.docker.remove_network(&name).await?;
            }
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }
        event!(
            name: "worker_pool.worker.removed",
            Level::INFO,
            work_batch.id = %batch_id,
            "removed a worker's container, volume, and network",
        );
        Ok(())
    }

    /// The batches this deployment holds anything for in Docker: a container, network, or
    /// volume labelled as one of its workers'.
    ///
    /// # Errors
    /// Propagates Docker's refusal of a listing.
    pub async fn leftovers(&self) -> anyhow::Result<HashSet<Uuid>> {
        // Docker matches a resource only when it carries every label listed.
        let filters = HashMap::from([(
            "label".to_owned(),
            vec![
                format!("{}={}", MANAGED_LABEL.0, MANAGED_LABEL.1),
                format!("{DEPLOYMENT_LABEL}={}", self.deployment),
            ],
        )]);
        let mut labels: Vec<HashMap<String, String>> = Vec::new();
        let containers = ListContainersOptionsBuilder::new()
            .all(true)
            .filters(&filters)
            .build();
        for container in self.docker.list_containers(Some(containers)).await? {
            labels.extend(container.labels);
        }
        let networks = ListNetworksOptionsBuilder::new().filters(&filters).build();
        for network in self.docker.list_networks(Some(networks)).await? {
            labels.extend(network.labels);
        }
        let volumes = ListVolumesOptionsBuilder::new().filters(&filters).build();
        for volume in self
            .docker
            .list_volumes(Some(volumes))
            .await?
            .volumes
            .unwrap_or_default()
        {
            labels.push(volume.labels);
        }
        Ok(labels
            .iter()
            .filter_map(|labels| labels.get(BATCH_LABEL))
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect())
    }

    fn labels(&self, batch_id: Uuid) -> HashMap<String, String> {
        HashMap::from([
            (MANAGED_LABEL.0.to_owned(), MANAGED_LABEL.1.to_owned()),
            (BATCH_LABEL.to_owned(), batch_id.to_string()),
            (DEPLOYMENT_LABEL.to_owned(), self.deployment.clone()),
        ])
    }
}

fn has_status(error: &DockerError, status: u16) -> bool {
    matches!(error, DockerError::DockerResponseServerError { status_code, .. } if *status_code == status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workers_resources_name_and_label_its_batch_and_deployment() {
        let batch_id = Uuid::now_v7();
        let containers = Containers::new(
            Docker::connect_with_http("http://127.0.0.1:1", 1, bollard::API_DEFAULT_VERSION)
                .expect("a lazy client"),
            "elysium-workspace:test".to_owned(),
            &SecretString::from("an encryption key for tests only"),
        );
        let labels = containers.labels(batch_id);
        assert_eq!(labels.get(BATCH_LABEL), Some(&batch_id.to_string()));
        assert_eq!(
            labels.get(MANAGED_LABEL.0).map(String::as_str),
            Some("worker")
        );
        assert_eq!(
            labels.get(DEPLOYMENT_LABEL),
            Some(&containers.deployment),
            "another deployment's workers are never this one's leftovers"
        );
        let workspace = containers.workspace(batch_id, 1_000_000_000);
        assert_eq!(workspace.name(), format!("elysium-worker-{batch_id}"));
    }
}
