// Copyright © 2026 Jalapeno Labs

//! Runs each browser's Neko container through Docker.
//!
//! Like the mail server, browsers are not part of `compose.yml`: the API creates one when a
//! person adds it, and keeps it running. Everything is named after the browser's id and
//! labelled [`MANAGED_LABEL`] and with this deployment's [`DEPLOYMENT_LABEL`]:
//!
//! | Kind      | Name                    | Purpose                                              |
//! |-----------|-------------------------|------------------------------------------------------|
//! | network   | `elysium-browser-<id>`  | The browser's own network, joined by the API and nginx |
//! | volume    | `elysium-browser-<id>`  | `/home/neko/.config/chromium`, the signed-in profile |
//! | container | `elysium-browser-<id>`  | Neko, answering as `elysium-browser-<port>`          |
//!
//! A network per browser is what keeps the `DevTools` port, which has no authentication, out
//! of every other browser's reach. nginx resolves `elysium-browser-<port>` on whichever of
//! its networks has it, and passes that port's media to it (`nginx/browser.conf`).
//!
//! The container runs Neko's published image unchanged. Before it first starts, the files in
//! `neko/` are copied in through Docker's archive endpoint: Chromium's command line, the
//! `DevTools` forwarder and its supervisord program, and the managed policy. Everything that
//! shapes the container is hashed into [`SPEC_LABEL`], so a container built from an older
//! spec (another address, image, or file) is replaced, keeping its profile volume.
//!
//! Docker is shared by everything on the host, another Elysium deployment's browsers
//! included. The reconciler removes what no row owns, so it lists only what carries its own
//! deployment's label, and never removes a browser, or the profile a person signed in on,
//! that another deployment created.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{
    ContainerCreateBody, EndpointSettings, HostConfig, NetworkConnectRequest, NetworkCreateRequest,
    NetworkDisconnectRequest, NetworkingConfig, RestartPolicy, RestartPolicyNameEnum,
    VolumeCreateRequest,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, ListContainersOptionsBuilder,
    ListNetworksOptionsBuilder, ListVolumesOptionsBuilder, RemoveContainerOptionsBuilder,
    RemoveVolumeOptionsBuilder, StopContainerOptionsBuilder, UploadToContainerOptionsBuilder,
};
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use tracing::{Level, event};
use uuid::Uuid;

/// Neko 3.1.6 with Chromium 154, pinned by digest. A new Neko release can change the
/// protocol the frontend speaks and the files below replace, so moving it is a reviewed
/// change. See docs/browser.md.
const IMAGE: &str = "ghcr.io/m1k1o/neko/chromium@sha256:2471119c7b4010067f76f3421a78a8bcf35e3c223b2d1f4db084914ec915fb30";
const IMAGE_REPOSITORY: &str = "ghcr.io/m1k1o/neko/chromium";
const IMAGE_DIGEST: &str =
    "sha256:2471119c7b4010067f76f3421a78a8bcf35e3c223b2d1f4db084914ec915fb30";

/// The compose service's fixed container name, which the API joins to every browser's
/// network so it can pass the media port along.
const NGINX_CONTAINER: &str = "elysium-nginx";

/// The gateway priority nginx and the API join a browser's network with. Docker routes a
/// container's default gateway, and the ports it publishes, through its network of highest
/// priority, breaking ties by name; at the default of 0, a browser's network could win, and
/// removing that browser would then cut every connection nginx was serving. Below 0, their
/// own compose networks always win.
const JOINED_GATEWAY_PRIORITY: i64 = -1;

/// Neko's HTTP port, for its API and the signaling WebSocket. Never published.
pub const NEKO_HTTP_PORT: u16 = 8080;
/// Where `neko/cdp-forward.py` listens for `DevTools` connections.
pub const DEVTOOLS_PORT: u16 = 9223;

/// The desktop Chromium draws on, and what the stream carries: wide enough for a mail
/// client's three panes, at a frame rate that keeps typing responsive.
const SCREEN: &str = "1920x1080@30";

/// Chromium renders to shared memory, and Docker's 64 MiB default crashes tabs. Neko's
/// documentation asks for 2 GiB.
const SHARED_MEMORY_BYTES: i64 = 2 * 1024 * 1024 * 1024;

/// How long a browser has to shut down before Docker kills it: Chromium writes its cookies
/// and releases its profile lock on the way out, in a second or two.
const STOP_TIMEOUT_SECONDS: i32 = 15;

/// The profile directory, as Neko's Chromium is started with it (`neko/chromium.conf`).
const PROFILE_DIRECTORY: &str = "/home/neko/.config/chromium";

/// Docker reports a container that was created but never started as having started at the
/// zero time, `0001-01-01T00:00:00Z`.
const NEVER_STARTED_YEAR: &str = "0001-";

const MANAGED_LABEL: (&str, &str) = ("dev.elysium.managed", "browser");
const SESSION_LABEL: &str = "dev.elysium.browser-session";
/// Which deployment created the resource: a value derived from its encryption key, which is
/// what makes a deployment its own. See [`deployment_label`].
const DEPLOYMENT_LABEL: &str = "dev.elysium.deployment";
/// A hash of everything [`container_spec`] and [`setup_archive`] depend on.
const SPEC_LABEL: &str = "dev.elysium.browser-spec";

/// The files copied into every container, at their paths inside it.
const SETUP_FILES: [(&str, &str); 5] = [
    (
        "etc/neko/supervisord/chromium.conf",
        include_str!("neko/chromium.conf"),
    ),
    (
        "etc/neko/elysium/start-chromium.sh",
        include_str!("neko/start-chromium.sh"),
    ),
    (
        "etc/neko/supervisord/elysium.conf",
        include_str!("neko/elysium.conf"),
    ),
    (
        "etc/neko/elysium/cdp-forward.py",
        include_str!("neko/cdp-forward.py"),
    ),
    (
        "etc/chromium/policies/managed/policies.json",
        include_str!("neko/policies.json"),
    ),
];

/// Everything one browser's container is built from.
#[derive(Debug)]
pub struct BrowserSpec {
    pub session_id: Uuid,
    /// The media port, the same on the container and the host.
    pub port: u16,
    /// The address browsers reach the host at, which the browser names in its candidates.
    pub public_address: IpAddr,
    pub member_password: SecretString,
}

/// Docker refused a call while running a browser.
#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("Docker: {0}")]
    Docker(#[from] DockerError),
    #[error("the setup files could not be packed: {0}")]
    Archive(#[from] std::io::Error),
    #[error(
        "the API is not running in a container it can find (HOSTNAME={0:?}), so it cannot join the \
         browser's network"
    )]
    ApiContainerUnknown(String),
}

#[derive(Debug, Clone)]
pub struct BrowserContainers {
    docker: Docker,
    /// This deployment's [`DEPLOYMENT_LABEL`] value.
    deployment: String,
}

impl BrowserContainers {
    pub fn new(docker: Docker, encryption_key: &SecretString) -> Self {
        Self {
            docker,
            deployment: deployment_label(encryption_key),
        }
    }

    /// Pulls the pinned image unless Docker already has it.
    ///
    /// # Errors
    /// [`ContainerError::Docker`] when the pull fails.
    pub async fn ensure_image(&self) -> Result<(), ContainerError> {
        match self.docker.inspect_image(IMAGE).await {
            Ok(_) => return Ok(()),
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }

        event!(name: "browser.container.pull_started", Level::INFO, container.image.name = IMAGE, "pulling");
        let options = CreateImageOptionsBuilder::new()
            .from_image(IMAGE_REPOSITORY)
            .tag(IMAGE_DIGEST)
            .build();
        let mut progress = self.docker.create_image(Some(options), None, None);
        while let Some(update) = progress.next().await {
            update?;
        }
        Ok(())
    }

    /// Brings the browser's network, volume, and container to `spec`, creating or replacing
    /// whatever is missing or stale, and starts it. Cheap when everything is already in
    /// place, so it runs on every reconcile.
    ///
    /// # Errors
    /// [`ContainerError`] when Docker refuses, or the API cannot find its own container.
    pub async fn ensure(&self, spec: &BrowserSpec) -> Result<(), ContainerError> {
        let name = resource_name(spec.session_id);
        self.ensure_network(&name, spec.session_id).await?;
        self.ensure_volume(&name, spec.session_id).await?;

        let fingerprint = spec_fingerprint(spec, &self.deployment);
        match self.docker.inspect_container(&name, None).await {
            Ok(existing) => {
                let current = existing
                    .config
                    .and_then(|config| config.labels)
                    .and_then(|labels| labels.get(SPEC_LABEL).cloned());
                let state = existing.state.unwrap_or_default();
                // Starting is the last step of creating one, so a container that never ran may
                // be missing its setup files, whatever its label says.
                let has_started = has_ever_started(state.started_at.as_deref());
                if current.as_deref() == Some(fingerprint.as_str()) && has_started {
                    if !state.running.unwrap_or(false) {
                        self.docker.start_container(&name, None).await?;
                    }
                    return Ok(());
                }
                event!(
                    name: "browser.container.replaced",
                    Level::INFO,
                    container.name = %name,
                    container.started = has_started,
                    "replacing a browser container built from another spec, or never started",
                );
                self.remove_container(&name).await?;
            }
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }

        let options = CreateContainerOptionsBuilder::new().name(&name).build();
        self.docker
            .create_container(
                Some(options),
                container_spec(
                    spec,
                    self.managed_labels(spec.session_id),
                    &fingerprint,
                    crate::browser::generate_password().expose_secret(),
                ),
            )
            .await?;
        // The label claims the setup files are in place, so a container that did not receive
        // them must not survive for the next pass to start.
        if let Err(error) = self.upload_setup_files(&name).await {
            self.remove_container(&name).await?;
            return Err(error);
        }
        self.docker.start_container(&name, None).await?;
        event!(name: "browser.container.created", Level::INFO, container.name = %name, "created");
        Ok(())
    }

    /// Removes the browser's container, network, and profile volume. What is already gone
    /// is skipped, so a half-removed browser can be removed again.
    ///
    /// # Errors
    /// [`ContainerError::Docker`] when Docker refuses.
    pub async fn remove(&self, session_id: Uuid) -> Result<(), ContainerError> {
        let name = resource_name(session_id);
        self.remove_container(&name).await?;

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

        let options = RemoveVolumeOptionsBuilder::new().force(true).build();
        match self.docker.remove_volume(&name, Some(options)).await {
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            _removed_or_missing => {}
        }
        event!(name: "browser.container.removed", Level::INFO, container.name = %name, "removed");
        Ok(())
    }

    /// Whether the Docker Engine honors the gateway priority nginx and the API join browser
    /// networks with. Older engines ignore it silently, and a browser's network can then
    /// become nginx's gateway.
    ///
    /// # Errors
    /// [`ContainerError::Docker`] when Docker does not answer.
    pub async fn supports_gateway_priority(&self) -> Result<bool, ContainerError> {
        let version = self.docker.version().await?;
        Ok(version
            .api_version
            .as_deref()
            .is_some_and(api_version_has_gateway_priority))
    }

    async fn upload_setup_files(&self, name: &str) -> Result<(), ContainerError> {
        let upload = UploadToContainerOptionsBuilder::new().path("/").build();
        self.docker
            .upload_to_container(
                name,
                Some(upload),
                bollard::body_full(setup_archive()?.into()),
            )
            .await?;
        Ok(())
    }

    /// The ids of every browser this deployment holds anything for in Docker: a container,
    /// network, or volume labelled as one of its browsers. The reconciler removes those no row
    /// owns any more.
    ///
    /// # Errors
    /// [`ContainerError::Docker`] when Docker refuses a listing.
    pub async fn managed_session_ids(&self) -> Result<HashSet<Uuid>, ContainerError> {
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

        let ids = labels
            .iter()
            .filter_map(|labels| labels.get(SESSION_LABEL))
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect();
        Ok(ids)
    }

    /// Stops the container, giving Chromium time to write its cookies and release its profile,
    /// then removes it. Forcing a running container out would lose whatever Chromium had not
    /// flushed yet.
    async fn remove_container(&self, name: &str) -> Result<(), ContainerError> {
        let stop = StopContainerOptionsBuilder::new()
            .t(STOP_TIMEOUT_SECONDS)
            .build();
        match self.docker.stop_container(name, Some(stop)).await {
            // 304: it was not running; 404: it does not exist.
            Err(error) if !has_status(&error, 304) && !has_status(&error, 404) => {
                return Err(error.into());
            }
            _stopped_or_absent => {}
        }
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        match self.docker.remove_container(name, Some(options)).await {
            Err(error) if !has_status(&error, 404) => Err(error.into()),
            _removed_or_missing => Ok(()),
        }
    }

    /// Creates the browser's network unless it exists, and joins the API and nginx to it.
    /// nginx loses the connection whenever compose recreates it, which is why this runs on
    /// every reconcile rather than once.
    async fn ensure_network(&self, name: &str, session_id: Uuid) -> Result<(), ContainerError> {
        let network = match self.docker.inspect_network(name, None).await {
            Ok(network) => network,
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {
                self.docker
                    .create_network(NetworkCreateRequest {
                        name: name.to_owned(),
                        driver: Some("bridge".to_owned()),
                        labels: Some(self.managed_labels(session_id)),
                        ..Default::default()
                    })
                    .await?;
                self.docker.inspect_network(name, None).await?
            }
        };
        let joined = network.containers.unwrap_or_default();

        for member in [self.api_container().await?, NGINX_CONTAINER.to_owned()] {
            let id = self
                .docker
                .inspect_container(&member, None)
                .await?
                .id
                .unwrap_or_default();
            if joined.contains_key(&id) {
                continue;
            }
            self.docker.connect_network(name, join_request(id)).await?;
        }
        Ok(())
    }

    async fn ensure_volume(&self, name: &str, session_id: Uuid) -> Result<(), ContainerError> {
        match self.docker.inspect_volume(name).await {
            Ok(_) => return Ok(()),
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }
        self.docker
            .create_volume(VolumeCreateRequest {
                name: Some(name.to_owned()),
                labels: Some(self.managed_labels(session_id)),
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    fn managed_labels(&self, session_id: Uuid) -> HashMap<String, String> {
        HashMap::from([
            (MANAGED_LABEL.0.to_owned(), MANAGED_LABEL.1.to_owned()),
            (SESSION_LABEL.to_owned(), session_id.to_string()),
            (DEPLOYMENT_LABEL.to_owned(), self.deployment.clone()),
        ])
    }

    /// The API's own container. Docker sets `HOSTNAME` to the container's short id.
    async fn api_container(&self) -> Result<String, ContainerError> {
        let hostname = std::env::var("HOSTNAME").unwrap_or_default();
        match self.docker.inspect_container(&hostname, None).await {
            Ok(container) => Ok(container.id.unwrap_or(hostname)),
            Err(error) if has_status(&error, 404) || hostname.is_empty() => {
                Err(ContainerError::ApiContainerUnknown(hostname))
            }
            Err(error) => Err(error.into()),
        }
    }
}

/// Gateway priority arrived with Docker Engine 28.0, API version 1.48.
const GATEWAY_PRIORITY_API_VERSION: (u32, u32) = (1, 48);

/// Whether an Engine API version such as `1.55` is at least [`GATEWAY_PRIORITY_API_VERSION`].
fn api_version_has_gateway_priority(api_version: &str) -> bool {
    let mut parts = api_version.split('.').map(str::parse::<u32>);
    let (Some(Ok(major)), Some(Ok(minor))) = (parts.next(), parts.next()) else {
        return false;
    };
    (major, minor) >= GATEWAY_PRIORITY_API_VERSION
}

/// Whether Docker's `State.StartedAt` says the container ran at least once.
fn has_ever_started(started_at: Option<&str>) -> bool {
    started_at.is_some_and(|started_at| !started_at.starts_with(NEVER_STARTED_YEAR))
}

/// How nginx or the API joins a browser's network: never as its default gateway.
fn join_request(container: String) -> NetworkConnectRequest {
    NetworkConnectRequest {
        container,
        endpoint_config: Some(EndpointSettings {
            gw_priority: Some(JOINED_GATEWAY_PRIORITY),
            ..Default::default()
        }),
    }
}

/// The name a browser's network, volume, and container share.
fn resource_name(session_id: Uuid) -> String {
    format!("elysium-browser-{session_id}")
}

/// What the browser answers as on its network: nginx derives it from the port a packet
/// arrived on (`nginx/browser.conf`), and the API dials it.
pub fn host_name(port: u16) -> String {
    format!("elysium-browser-{port}")
}

/// The deployment's [`DEPLOYMENT_LABEL`] value: the hex SHA-256 of
/// `elysium/browser/deployment:<key>`, as `kratos/entrypoint.sh` derives Kratos's secrets
/// from the same key under their own purposes. Anyone who can reach Docker reads it, which
/// is harmless: it names the deployment and reveals nothing about the key.
fn deployment_label(encryption_key: &SecretString) -> String {
    let digest = Sha256::digest(format!(
        "elysium/browser/deployment:{}",
        encryption_key.expose_secret()
    ));
    hex::encode(digest)
}

/// Neko's configuration, as environment variables. The member password is the only way in:
/// a member can watch, hold control, and set the clipboard, which is all a viewer needs. The
/// administrator password is generated for each container and never kept, because nothing
/// needs an administrator session and Neko would otherwise accept its default one.
fn environment(spec: &BrowserSpec, discarded_admin_password: &str) -> Vec<String> {
    vec![
        "NEKO_MEMBER_PROVIDER=multiuser".to_owned(),
        format!(
            "NEKO_MEMBER_MULTIUSER_USER_PASSWORD={}",
            spec.member_password.expose_secret()
        ),
        format!("NEKO_MEMBER_MULTIUSER_ADMIN_PASSWORD={discarded_admin_password}"),
        // A sign-in answers its token in the body, for the API to use; no cookie is set.
        "NEKO_SESSION_COOKIE_ENABLED=false".to_owned(),
        format!("NEKO_DESKTOP_SCREEN={SCREEN}"),
        format!("NEKO_WEBRTC_UDPMUX={}", spec.port),
        format!("NEKO_WEBRTC_TCPMUX={}", spec.port),
        format!("NEKO_WEBRTC_NAT1TO1={}", spec.public_address),
        // The host's address is public, or at least reachable by every viewer, so the
        // browser needs no STUN to find one.
        "NEKO_WEBRTC_ICELITE=true".to_owned(),
        // Nothing reaches the file transfer or chat plugins through the stream route, and
        // nothing should be able to.
        "NEKO_FILETRANSFER_ENABLED=false".to_owned(),
        "NEKO_CHAT_ENABLED=false".to_owned(),
        "TZ=UTC".to_owned(),
    ]
}

/// The container Docker creates: the pinned image on the browser's own network, answering
/// as [`host_name`], with no published ports and the profile on its volume.
/// `discarded_admin_password` is generated per container by the caller and kept nowhere.
fn container_spec(
    spec: &BrowserSpec,
    mut labels: HashMap<String, String>,
    fingerprint: &str,
    discarded_admin_password: &str,
) -> ContainerCreateBody {
    let name = resource_name(spec.session_id);
    labels.insert(SPEC_LABEL.to_owned(), fingerprint.to_owned());

    ContainerCreateBody {
        image: Some(IMAGE.to_owned()),
        env: Some(environment(spec, discarded_admin_password)),
        labels: Some(labels),
        host_config: Some(HostConfig {
            binds: Some(vec![format!("{name}:{PROFILE_DIRECTORY}")]),
            shm_size: Some(SHARED_MEMORY_BYTES),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            network_mode: Some(name.clone()),
            ..Default::default()
        }),
        networking_config: Some(NetworkingConfig {
            endpoints_config: Some(HashMap::from([(
                name,
                EndpointSettings {
                    aliases: Some(vec![host_name(spec.port)]),
                    ..Default::default()
                },
            )])),
        }),
        ..Default::default()
    }
}

/// The files in `neko/`, as the tar archive Docker extracts at the container's root.
fn setup_archive() -> Result<Vec<u8>, std::io::Error> {
    let mut archive = tar::Builder::new(Vec::new());
    for (path, contents) in SETUP_FILES {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append_data(&mut header, path, contents.as_bytes())?;
    }
    archive.into_inner()
}

/// A hash of the image, the media port, the public address, the password, the setup files,
/// and the deployment: everything a running container would have to be replaced to change.
/// Labels are fixed when a container is created, so one without this deployment's is
/// replaced to carry it.
fn spec_fingerprint(spec: &BrowserSpec, deployment: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(IMAGE);
    hasher.update(deployment);
    hasher.update(spec.port.to_be_bytes());
    hasher.update(spec.public_address.to_string());
    hasher.update(spec.member_password.expose_secret());
    for (path, contents) in SETUP_FILES {
        hasher.update(path);
        hasher.update(contents);
    }
    hex::encode(hasher.finalize())
}

fn has_status(error: &DockerError, status: u16) -> bool {
    matches!(error, DockerError::DockerResponseServerError { status_code, .. } if *status_code == status)
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::net::Ipv4Addr;

    use super::*;

    const DEPLOYMENT: &str = "a-deployment";

    fn spec() -> BrowserSpec {
        BrowserSpec {
            session_id: Uuid::nil(),
            port: 59003,
            public_address: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)),
            member_password: SecretString::from("a-generated-password"),
        }
    }

    #[test]
    fn the_container_publishes_nothing_and_answers_by_its_port() {
        let browser = spec();
        let body = container_spec(&browser, HashMap::new(), "fingerprint", "discarded");
        let host_config = body.host_config.expect("host config");
        assert!(host_config.port_bindings.is_none());

        let network = resource_name(browser.session_id);
        assert_eq!(host_config.network_mode.as_deref(), Some(network.as_str()));
        let endpoint = &body
            .networking_config
            .and_then(|config| config.endpoints_config)
            .expect("endpoints")[&network];
        assert_eq!(
            endpoint.aliases.as_deref(),
            Some(&["elysium-browser-59003".to_owned()][..])
        );
        assert_eq!(
            host_config.binds,
            Some(vec![format!("{network}:{PROFILE_DIRECTORY}")])
        );
    }

    // Joined at Docker's default priority, a browser's network became nginx's gateway, the
    // host's requests to the web port arrived through it, and removing the browser cut them.
    #[test]
    fn nginx_and_the_api_never_take_a_browser_network_as_their_gateway() {
        let request = join_request("nginx".to_owned());
        let priority = request
            .endpoint_config
            .and_then(|endpoint| endpoint.gw_priority)
            .expect("a gateway priority is set");
        assert!(
            priority < 0,
            "priority {priority} could outrank compose's networks"
        );
    }

    // A container whose setup upload failed carried the right label but never started; the
    // next pass started it as is, without the setup files, and reported it running.
    #[test]
    fn a_container_that_never_started_is_not_taken_as_set_up() {
        assert!(!has_ever_started(Some("0001-01-01T00:00:00Z")));
        assert!(!has_ever_started(None));
        assert!(has_ever_started(Some("2026-10-20T20:22:46.368773Z")));
    }

    #[test]
    fn media_arrives_on_the_browsers_own_port_from_the_public_address() {
        let environment = environment(&spec(), "discarded");
        for expected in [
            "NEKO_WEBRTC_UDPMUX=59003",
            "NEKO_WEBRTC_TCPMUX=59003",
            "NEKO_WEBRTC_NAT1TO1=203.0.113.7",
            "NEKO_MEMBER_MULTIUSER_USER_PASSWORD=a-generated-password",
            "NEKO_SESSION_COOKIE_ENABLED=false",
        ] {
            assert!(
                environment.iter().any(|variable| variable == expected),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn the_fingerprint_is_stable_and_changes_with_each_input() {
        let original = spec_fingerprint(&spec(), DEPLOYMENT);
        assert_eq!(original, spec_fingerprint(&spec(), DEPLOYMENT));

        let mut moved = spec();
        moved.public_address = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4));
        let mut renumbered = spec();
        renumbered.port = 59004;
        let mut rekeyed = spec();
        rekeyed.member_password = SecretString::from("another-password");
        for changed in [moved, renumbered, rekeyed] {
            assert_ne!(
                original,
                spec_fingerprint(&changed, DEPLOYMENT),
                "{changed:?}"
            );
        }
        assert_ne!(original, spec_fingerprint(&spec(), "another-deployment"));
    }

    #[test]
    fn the_fingerprint_covers_every_setup_file() {
        // Each file's path and contents feed the hash, so any edit to one replaces containers.
        let mut hasher = Sha256::new();
        for (path, contents) in SETUP_FILES {
            hasher.update(path);
            hasher.update(contents);
        }
        let files_only = hex::encode(hasher.finalize());
        assert_ne!(files_only, spec_fingerprint(&spec(), DEPLOYMENT));
        assert_eq!(SETUP_FILES.len(), 5);
    }

    #[test]
    fn the_setup_archive_holds_every_file_at_its_path() {
        let bytes = setup_archive().expect("the archive packs");
        let mut archive = tar::Archive::new(bytes.as_slice());
        let mut found = HashMap::new();
        for entry in archive.entries().expect("entries") {
            let mut entry = entry.expect("an entry");
            let path = entry.path().expect("a path").display().to_string();
            let mut contents = String::new();
            entry.read_to_string(&mut contents).expect("text");
            found.insert(path, contents);
        }

        for (path, contents) in SETUP_FILES {
            assert_eq!(found.get(path).map(String::as_str), Some(contents));
        }
    }

    fn setup_file(path: &str) -> &'static str {
        SETUP_FILES
            .iter()
            .find(|(candidate, _contents)| *candidate == path)
            .map_or_else(
                || panic!("{path} is a setup file"),
                |(_path, contents)| *contents,
            )
    }

    #[test]
    fn chromium_keeps_its_profile_and_opens_the_devtools_port() {
        let chromium = setup_file("etc/neko/supervisord/chromium.conf");
        assert!(chromium.contains("--remote-debugging-port=9222"));
        assert!(!chromium.contains("--bwsi"));
        assert!(chromium.contains(&format!("--user-data-dir={PROFILE_DIRECTORY}")));

        let policies: serde_json::Value =
            serde_json::from_str(setup_file("etc/chromium/policies/managed/policies.json"))
                .expect("the policy is JSON");
        assert_eq!(policies["DefaultCookiesSetting"], 1);
        assert_eq!(policies["DefaultPopupsSetting"], 1);
        // At Neko's 2, a page refuses to be attached to over DevTools, and no page can be read.
        assert_eq!(policies["DeveloperToolsAvailability"], 1);
    }

    // A replaced container inherited its predecessor's profile lock, naming the old hostname,
    // and Chromium refused to start on it. It now starts through a script that clears it.
    #[test]
    fn chromium_starts_on_a_profile_a_replaced_container_left_locked() {
        let chromium = setup_file("etc/neko/supervisord/chromium.conf");
        assert!(chromium.contains("command=/bin/sh /etc/neko/elysium/start-chromium.sh"));

        let start = setup_file("etc/neko/elysium/start-chromium.sh");
        assert!(start.contains(r#""$profile/SingletonLock""#));
        assert!(start.contains(&format!("profile={PROFILE_DIRECTORY}")));
        assert!(start.contains(r#"exec /usr/bin/chromium "$@""#));
    }

    // Two deployments on one host each removed the other's browsers, profiles included,
    // because the sweep listed every browser Docker held. Each now lists only its own.
    #[test]
    fn each_deployment_labels_its_browsers_as_its_own() {
        let ours = deployment_label(&SecretString::from("testkey"));
        let theirs = deployment_label(&SecretString::from("another-key"));
        assert_ne!(ours, theirs);
        // `printf '%s' 'elysium/browser/deployment:testkey' | sha256sum`
        assert_eq!(
            ours,
            "393cff5de2f68ebe16a05a02310659fdd5f725735ece55d103482a69171e8c35"
        );
        assert!(!ours.contains("testkey"));
    }

    #[test]
    fn engines_before_api_1_48_cannot_set_a_gateway_priority() {
        assert!(api_version_has_gateway_priority("1.48"));
        assert!(api_version_has_gateway_priority("1.55"));
        assert!(api_version_has_gateway_priority("2.0"));
        assert!(!api_version_has_gateway_priority("1.47"));
        assert!(!api_version_has_gateway_priority("garbage"));
    }
}
