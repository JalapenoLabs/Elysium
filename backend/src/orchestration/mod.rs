//! Orchestration: the service control plane.
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
//! The layers, bottom up:
//!
//! - [`provisioner`]: the declarative [`provisioner::ServiceSpec`] model, the
//!   engine seam, and the in-memory backend standing in until #27.
//! - [`integrations`]: the integration data model (kinds, modes, owner scopes,
//!   capability-gated storage-role bindings) and the `/api/v1/integrations`
//!   management surface.
//! - [`reconcile`]: the desired-state reconciler converging managed rows to
//!   containers on boot and after every change.
//!
//! The module-wide `dead_code` allowance covers the parts of the seam landed
//! ahead of their consumers (`Provisioner::logs` and several error constructors
//! await #27's Docker backend and the console's log surface) without warning on
//! a binary build that does not call them yet.
#![allow(dead_code)]

pub mod integrations;
pub mod provisioner;
pub mod reconcile;
