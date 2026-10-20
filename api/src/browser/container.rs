// Copyright © 2026 Jalapeno Labs

//! Runs each browser's Neko container through Docker.
//!
//! Like the mail server, browsers are not part of `compose.yml`: the API creates one when a
//! person adds it, and keeps it running. Everything is named after the browser's id and
//! labelled [`MANAGED_LABEL`]:
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

use std::collections::HashMap;
use std::net::IpAddr;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{
    ContainerCreateBody, EndpointSettings, HostConfig, NetworkConnectRequest, NetworkCreateRequest,
    NetworkDisconnectRequest, NetworkingConfig, RestartPolicy, RestartPolicyNameEnum,
    VolumeCreateRequest,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, RemoveContainerOptionsBuilder,
    RemoveVolumeOptionsBuilder, UploadToContainerOptionsBuilder,
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

/// The profile directory, as Neko's Chromium is started with it (`neko/chromium.conf`).
const PROFILE_DIRECTORY: &str = "/home/neko/.config/chromium";

const MANAGED_LABEL: (&str, &str) = ("dev.elysium.managed", "browser");
const SESSION_LABEL: &str = "dev.elysium.browser-session";
/// A hash of everything [`container_spec`] and [`setup_archive`] depend on.
const SPEC_LABEL: &str = "dev.elysium.browser-spec";

/// The files copied into every container, at their paths inside it.
const SETUP_FILES: [(&str, &str); 4] = [
    (
        "etc/neko/supervisord/chromium.conf",
        include_str!("neko/chromium.conf"),
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
}

impl BrowserContainers {
    pub const fn new(docker: Docker) -> Self {
        Self { docker }
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

        let fingerprint = spec_fingerprint(spec);
        match self.docker.inspect_container(&name, None).await {
            Ok(existing) => {
                let current = existing
                    .config
                    .and_then(|config| config.labels)
                    .and_then(|labels| labels.get(SPEC_LABEL).cloned());
                if current.as_deref() == Some(fingerprint.as_str()) {
                    let running = existing
                        .state
                        .and_then(|state| state.running)
                        .unwrap_or(false);
                    if !running {
                        self.docker.start_container(&name, None).await?;
                    }
                    return Ok(());
                }
                event!(
                    name: "browser.container.replaced",
                    Level::INFO,
                    container.name = %name,
                    "replacing a browser container built from an older spec",
                );
                self.remove_container(&name).await?;
            }
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }

        let options = CreateContainerOptionsBuilder::new().name(&name).build();
        self.docker
            .create_container(Some(options), container_spec(spec, &fingerprint))
            .await?;
        let upload = UploadToContainerOptionsBuilder::new().path("/").build();
        self.docker
            .upload_to_container(
                &name,
                Some(upload),
                bollard::body_full(setup_archive()?.into()),
            )
            .await?;
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

    async fn remove_container(&self, name: &str) -> Result<(), ContainerError> {
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
                        labels: Some(managed_labels(session_id)),
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
            let request = NetworkConnectRequest {
                container: id,
                endpoint_config: None,
            };
            self.docker.connect_network(name, request).await?;
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
                labels: Some(managed_labels(session_id)),
                ..Default::default()
            })
            .await?;
        Ok(())
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

/// The name a browser's network, volume, and container share.
fn resource_name(session_id: Uuid) -> String {
    format!("elysium-browser-{session_id}")
}

/// What the browser answers as on its network: nginx derives it from the port a packet
/// arrived on (`nginx/browser.conf`), and the API dials it.
pub fn host_name(port: u16) -> String {
    format!("elysium-browser-{port}")
}

fn managed_labels(session_id: Uuid) -> HashMap<String, String> {
    HashMap::from([
        (MANAGED_LABEL.0.to_owned(), MANAGED_LABEL.1.to_owned()),
        (SESSION_LABEL.to_owned(), session_id.to_string()),
    ])
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
fn container_spec(spec: &BrowserSpec, fingerprint: &str) -> ContainerCreateBody {
    let name = resource_name(spec.session_id);
    let mut labels = managed_labels(spec.session_id);
    labels.insert(SPEC_LABEL.to_owned(), fingerprint.to_owned());

    ContainerCreateBody {
        image: Some(IMAGE.to_owned()),
        env: Some(environment(
            spec,
            crate::browser::generate_password().expose_secret(),
        )),
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

/// A hash of the image, the media port, the public address, the password, and the setup
/// files: everything a running container would have to be replaced to change.
fn spec_fingerprint(spec: &BrowserSpec) -> String {
    let mut hasher = Sha256::new();
    hasher.update(IMAGE);
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
        let body = container_spec(&browser, "fingerprint");
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
    fn the_fingerprint_changes_with_the_address_and_nothing_else_varies_it() {
        let first = spec();
        let mut moved = spec();
        moved.public_address = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4));

        assert_eq!(spec_fingerprint(&first), spec_fingerprint(&spec()));
        assert_ne!(spec_fingerprint(&first), spec_fingerprint(&moved));
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

    #[test]
    fn chromium_keeps_its_profile_and_opens_the_devtools_port() {
        let chromium = SETUP_FILES[0].1;
        assert!(chromium.contains("--remote-debugging-port=9222"));
        assert!(!chromium.contains("--bwsi"));
        assert!(chromium.contains(&format!("--user-data-dir={PROFILE_DIRECTORY}")));

        let policies: serde_json::Value =
            serde_json::from_str(SETUP_FILES[3].1).expect("the policy is JSON");
        assert_eq!(policies["DefaultCookiesSetting"], 1);
    }
}
