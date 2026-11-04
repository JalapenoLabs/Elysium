// Copyright © 2026 Jalapeno Labs

//! Finding this stack's own containers in Docker, for the networks the engine creates and joins
//! them to: each browser's, which the API and nginx join, and each worker's, which the API joins.
//!
//! Compose labels every container it runs with its project and service. The engine finds its
//! own container by `HOSTNAME`, which Docker sets to the short container id, reads its project
//! from it, and lists the project's containers of a service, such as every replica of the API.

use std::collections::HashMap;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::ListContainersOptionsBuilder;

/// The labels compose gives each container it runs: its project, and its service.
const COMPOSE_PROJECT_LABEL: &str = "com.docker.compose.project";
const COMPOSE_SERVICE_LABEL: &str = "com.docker.compose.service";

/// The compose service the API runs as.
pub const API_SERVICE: &str = "api";

/// Why this process's stack could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    #[error("Docker: {0}")]
    Docker(#[from] DockerError),
    #[error(
        "this process is not running in a container it can find (HOSTNAME={0:?}), so it cannot \
         join a network"
    )]
    OwnContainerUnknown(String),
}

/// This process's own container: its full id, and the compose project it belongs to, if any.
///
/// # Errors
/// [`ComposeError::OwnContainerUnknown`] outside a container Docker knows; any other Docker
/// refusal.
pub async fn own_container(docker: &Docker) -> Result<(String, Option<String>), ComposeError> {
    let hostname = std::env::var("HOSTNAME").unwrap_or_default();
    let own = match docker.inspect_container(&hostname, None).await {
        Ok(container) => container,
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => return Err(ComposeError::OwnContainerUnknown(hostname)),
        Err(_error) if hostname.is_empty() => {
            return Err(ComposeError::OwnContainerUnknown(hostname));
        }
        Err(error) => return Err(error.into()),
    };
    let project = own
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get(COMPOSE_PROJECT_LABEL))
        .cloned();
    Ok((own.id.unwrap_or(hostname), project))
}

/// The ids of compose `project`'s running containers of `service`.
///
/// # Errors
/// Propagates Docker's refusal.
pub async fn service_containers(
    docker: &Docker,
    project: &str,
    service: &str,
) -> Result<Vec<String>, DockerError> {
    let filters = HashMap::from([(
        "label".to_owned(),
        vec![
            format!("{COMPOSE_PROJECT_LABEL}={project}"),
            format!("{COMPOSE_SERVICE_LABEL}={service}"),
        ],
    )]);
    let options = ListContainersOptionsBuilder::new()
        .filters(&filters)
        .build();
    Ok(docker
        .list_containers(Some(options))
        .await?
        .into_iter()
        .filter_map(|container| container.id)
        .collect())
}

/// Every replica of the API in this process's compose project. Outside a compose project there
/// are none to find.
///
/// # Errors
/// Propagates [`own_container`]'s and Docker's refusals.
pub async fn api_replicas(docker: &Docker) -> Result<Vec<String>, ComposeError> {
    let (_own, project) = own_container(docker).await?;
    let Some(project) = project else {
        tracing::event!(
            name: "compose.project.unknown",
            tracing::Level::WARN,
            "this process's container belongs to no compose project, so no API replica is found",
        );
        return Ok(Vec::new());
    };
    Ok(service_containers(docker, &project, API_SERVICE).await?)
}
