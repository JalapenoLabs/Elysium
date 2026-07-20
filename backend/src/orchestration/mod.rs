//! Orchestration: the service control plane's provisioning seam.
//!
//! Elysium provisions and manages best-of-breed OSS containers (Forgejo, a mail
//! server, a registry, Astraval) instead of re-implementing those services (see
//! `docs/integrations.md`). The API never hardcodes a container engine and never
//! holds the raw Docker socket: everything the control plane wants from an engine
//! goes through the backend-agnostic [`provisioner::Provisioner`] trait. A Docker
//! backend (#27) implements it first, behind a scoped socket-proxy; Podman and a
//! Kubernetes backend slot in behind the same seam without touching the
//! integrations that call it.
//!
//! A managed service is described declaratively by a
//! [`provisioner::ServiceSpec`], and every operation is idempotent so the
//! desired-state reconciler (#28) can converge repeatedly without special-casing
//! "already done".
//!
//! Nothing here is referenced by the binary yet; #27 and #28 wire it in. The
//! module-wide `dead_code` allowance keeps the seam landed and unit-tested ahead
//! of those consumers, without warning on a binary build that does not call it
//! yet.
#![allow(dead_code)]

pub mod provisioner;
