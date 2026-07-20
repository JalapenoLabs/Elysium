//! Orchestration: the service control plane.
//!
//! Elysium provisions and manages best-of-breed OSS containers (Forgejo, a mail
//! server, a registry, Astraval) instead of re-implementing those services (see
//! `docs/integrations.md`). The API never hardcodes a container engine and never
//! holds the raw Docker socket: everything the control plane wants from an engine
//! goes through the backend-agnostic [`provisioner::Provisioner`] trait. The
//! [`docker`] backend implements it against the Docker Engine API behind a scoped
//! socket-proxy (and drives a Docker-compatible Podman socket unchanged); a
//! Kubernetes backend can slot in behind the same seam without touching the
//! integrations that call it.
//!
//! The layers, bottom up:
//!
//! - [`provisioner`]: the declarative [`provisioner::ServiceSpec`] model, the
//!   engine seam, and the in-memory backend used in tests and zero-config boot.
//! - [`docker`]: the [`docker::DockerProvisioner`] backend, selected at boot when
//!   a Docker proxy URL is configured.
//! - [`integrations`]: the integration data model (kinds, modes, owner scopes,
//!   capability-gated storage-role bindings) and the `/api/v1/integrations`
//!   management surface.
//! - [`integration_policies`]: the per-kind member-linking toggles and their
//!   admin control plane (`/api/v1/orgs/{id}/integration-policies`), enforced by
//!   the personal-linked creation path in [`integrations`].
//! - [`config`]: per-kind config templating and secret injection, turning an
//!   integration's stored config and sealed secret into the env its blessed
//!   image expects.
//! - [`reconcile`]: the desired-state reconciler converging managed rows to
//!   containers on boot and after every change; it also registers and unregisters
//!   HTTP routes as integrations come and go.
//! - [`routing`]: the embedded HTTP reverse proxy that makes a managed service's
//!   web surface reachable and embeddable at `/services/{id}` (streaming, with
//!   websocket upgrades and a forward-auth seam for #33).
//!
//! The module-wide `dead_code` allowance covers the parts of the seam landed
//! ahead of their consumers (`Provisioner::logs` awaits the console's log
//! surface) without warning on a binary build that does not call them yet.
#![allow(dead_code)]

pub mod config;
pub mod docker;
pub mod integration_policies;
pub mod integrations;
pub mod provisioner;
pub mod reconcile;
pub mod routing;
