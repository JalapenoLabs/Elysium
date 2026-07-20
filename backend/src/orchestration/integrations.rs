//! Integrations: the data model and management API of the service control plane.
//!
//! An **integration** is to services what a Drive is to files (see
//! `docs/integrations.md`): an org owns many, of many kinds (git, mail, registry,
//! perforce, runner), each either **managed** (Elysium provisions and owns a
//! container) or **linked** (Elysium holds credentials to an external endpoint).
//! Managed integrations are desired state in Postgres; the sibling
//! [`crate::orchestration::reconcile`] module converges them to actual containers
//! on boot and after every change here.
//!
//! Ownership follows the project tier (`docs/projects.md`): work integrations
//! default to **project** scope (one container per project), with **org** for the
//! rare org-shared service and **personal** for a member's own linked account.
//! Every authz decision resolves through the org -> project -> team hierarchy
//! ([`crate::projects::resolve_project_access`]) or the delegated
//! `manage_integrations` capability, mirroring the storage module's
//! `manage_storage` gate.
//!
//! Storage-role bindings are **capability-gated**: each kind declares roles (git
//! `repos`, registry `blobs`, ...) with a required [`StorageCapability`], and a
//! role binds only to a Drive whose kind advertises it ([`DriveKind::capabilities`]).
//! This is what keeps "git on OneDrive" an enforced impossibility.
//!
//! Routes live under `/api/v1/integrations` (wired in [`crate::api`]) and speak
//! protobuf. Credentials are write-only: sealed with [`crate::crypto`] bound to
//! the integration id and surfaced only as `credential_set`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::authz::{self, Capability, OrgRole};
use crate::crypto::Crypto;
use crate::orchestration::integration_policies;
use crate::orchestration::provisioner::Provisioner;
use crate::orchestration::reconcile;
use crate::projects::{has_project_access, resolve_project_access, ProjectRole};
use crate::proto::common::v1::{Timestamp, Uuid as ProtoUuid};
use crate::proto::integration::v1::{
    CreateIntegrationRequest, Integration, IntegrationKind as ProtoKind,
    IntegrationMode as ProtoMode, IntegrationStorageBinding as ProtoBinding,
    IntegrationVisibility as ProtoVisibility, ListIntegrationsResponse, OwnerScope as ProtoScope,
    ReconcileStatus as ProtoStatus, SetIntegrationBindingsRequest, UpdateIntegrationRequest,
};
use crate::proto::storage::v1::StorageCapability;
use crate::protobuf::Protobuf;
use crate::session::{Principal, SessionUser};
use crate::state::AppState;
use crate::storage::drives::can_access_drive;
use crate::storage::{DriveKind, DriveRow, UnknownDiscriminant};
use crate::users;

/// `integration_access.principal` value for a team grant, mirroring `drive_access`.
const PRINCIPAL_TEAM: i16 = 1;

/// `integration_access.principal` value for a user grant, mirroring `drive_access`.
const PRINCIPAL_USER: i16 = 2;

// --- Wire/database enums -----------------------------------------------------

/// What service an integration provides, mirroring `elysium.integration.v1.IntegrationKind`.
///
/// Persisted as the smallint from [`IntegrationKind::as_db`]; the proto's zero
/// `UNSPECIFIED` value is never stored. The kind is immutable for the life of an
/// integration and determines the blessed default image (managed) and the storage
/// roles it binds (see [`storage_roles`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationKind {
    /// A git forge (managed: Forgejo; linked: GitHub, GitLab).
    Git,
    /// A mail server (managed: self-hosted; linked: Gmail, Outlook).
    Mail,
    /// A container registry (managed: CNCF distribution; linked: Docker Hub).
    Registry,
    /// A Perforce-protocol server (managed: JalapenoLabs Astraval).
    Perforce,
    /// A CI runner / Claude agentic worker container.
    Runner,
}

impl IntegrationKind {
    /// Every kind, for exhaustive table-driven tests and role listings.
    pub const ALL: [Self; 5] = [
        Self::Git,
        Self::Mail,
        Self::Registry,
        Self::Perforce,
        Self::Runner,
    ];

    /// The smallint stored in `integrations.kind` for this kind.
    pub fn as_db(self) -> i16 {
        match self {
            Self::Git => 1,
            Self::Mail => 2,
            Self::Registry => 3,
            Self::Perforce => 4,
            Self::Runner => 5,
        }
    }

    /// Parses a stored `integrations.kind` smallint; unknown values are `None`.
    pub fn from_db(value: i16) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// The wire enum for this kind.
    pub fn to_proto(self) -> ProtoKind {
        match self {
            Self::Git => ProtoKind::Git,
            Self::Mail => ProtoKind::Mail,
            Self::Registry => ProtoKind::Registry,
            Self::Perforce => ProtoKind::Perforce,
            Self::Runner => ProtoKind::Runner,
        }
    }

    /// Parses a wire enum into a kind; the `UNSPECIFIED` sentinel is `None`.
    pub fn from_proto(value: ProtoKind) -> Option<Self> {
        match value {
            ProtoKind::Git => Some(Self::Git),
            ProtoKind::Mail => Some(Self::Mail),
            ProtoKind::Registry => Some(Self::Registry),
            ProtoKind::Perforce => Some(Self::Perforce),
            ProtoKind::Runner => Some(Self::Runner),
            ProtoKind::Unspecified => None,
        }
    }

    /// The kind's lowercase token, used in service names and reserved storage
    /// prefixes (`.elysium/<slug>/<role>`).
    pub fn slug(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Mail => "mail",
            Self::Registry => "registry",
            Self::Perforce => "perforce",
            Self::Runner => "runner",
        }
    }
}

impl TryFrom<i16> for IntegrationKind {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Git),
            2 => Ok(Self::Mail),
            3 => Ok(Self::Registry),
            4 => Ok(Self::Perforce),
            5 => Ok(Self::Runner),
            other => Err(UnknownDiscriminant::new("IntegrationKind", other)),
        }
    }
}

/// Whether Elysium runs the service or connects to one that already exists,
/// mirroring `elysium.integration.v1.IntegrationMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationMode {
    /// Elysium provisions and owns the container lifecycle. Admin infrastructure.
    Managed,
    /// Elysium holds credentials to an external endpoint; no container.
    Linked,
}

impl IntegrationMode {
    /// The smallint stored in `integrations.mode` for this mode.
    pub fn as_db(self) -> i16 {
        match self {
            Self::Managed => 1,
            Self::Linked => 2,
        }
    }

    /// Parses a stored `integrations.mode` smallint; unknown values are `None`.
    pub fn from_db(value: i16) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// The wire enum for this mode.
    pub fn to_proto(self) -> ProtoMode {
        match self {
            Self::Managed => ProtoMode::Managed,
            Self::Linked => ProtoMode::Linked,
        }
    }

    /// Parses a wire enum into a mode; the `UNSPECIFIED` sentinel is `None`.
    pub fn from_proto(value: ProtoMode) -> Option<Self> {
        match value {
            ProtoMode::Managed => Some(Self::Managed),
            ProtoMode::Linked => Some(Self::Linked),
            ProtoMode::Unspecified => None,
        }
    }
}

impl TryFrom<i16> for IntegrationMode {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Managed),
            2 => Ok(Self::Linked),
            other => Err(UnknownDiscriminant::new("IntegrationMode", other)),
        }
    }
}

/// Which tier owns an integration, mirroring `elysium.integration.v1.OwnerScope`.
///
/// Work integrations default to [`OwnerScope::Project`]; [`OwnerScope::Personal`]
/// is a member's own linked account and is never managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerScope {
    Org,
    Project,
    Personal,
}

impl OwnerScope {
    /// The smallint stored in `integrations.owner_scope` for this scope.
    pub fn as_db(self) -> i16 {
        match self {
            Self::Org => 1,
            Self::Project => 2,
            Self::Personal => 3,
        }
    }

    /// Parses a stored `integrations.owner_scope` smallint; unknown values are `None`.
    pub fn from_db(value: i16) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// The wire enum for this scope.
    pub fn to_proto(self) -> ProtoScope {
        match self {
            Self::Org => ProtoScope::Org,
            Self::Project => ProtoScope::Project,
            Self::Personal => ProtoScope::Personal,
        }
    }

    /// Parses a wire enum into a scope; the `UNSPECIFIED` sentinel is `None`.
    pub fn from_proto(value: ProtoScope) -> Option<Self> {
        match value {
            ProtoScope::Org => Some(Self::Org),
            ProtoScope::Project => Some(Self::Project),
            ProtoScope::Personal => Some(Self::Personal),
            ProtoScope::Unspecified => None,
        }
    }
}

impl TryFrom<i16> for OwnerScope {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Org),
            2 => Ok(Self::Project),
            3 => Ok(Self::Personal),
            other => Err(UnknownDiscriminant::new("OwnerScope", other)),
        }
    }
}

/// Who inside the org may see and use an org- or project-scoped integration,
/// mirroring `elysium.integration.v1.IntegrationVisibility` (and DriveVisibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationVisibility {
    /// Every member with access to the owning tier can see and use it.
    OrgWide,
    /// Access granted to a chosen mix of teams and users via `integration_access`.
    Restricted,
}

impl IntegrationVisibility {
    /// The smallint stored in `integrations.visibility` for this visibility.
    pub fn as_db(self) -> i16 {
        match self {
            Self::OrgWide => 1,
            Self::Restricted => 2,
        }
    }

    /// Parses a stored `integrations.visibility` smallint; unknown values are `None`.
    pub fn from_db(value: i16) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// The wire enum for this visibility.
    pub fn to_proto(self) -> ProtoVisibility {
        match self {
            Self::OrgWide => ProtoVisibility::OrgWide,
            Self::Restricted => ProtoVisibility::Restricted,
        }
    }

    /// Parses a wire enum; the `UNSPECIFIED` sentinel is `None`.
    pub fn from_proto(value: ProtoVisibility) -> Option<Self> {
        match value {
            ProtoVisibility::OrgWide => Some(Self::OrgWide),
            ProtoVisibility::Restricted => Some(Self::Restricted),
            ProtoVisibility::Unspecified => None,
        }
    }
}

impl TryFrom<i16> for IntegrationVisibility {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::OrgWide),
            2 => Ok(Self::Restricted),
            other => Err(UnknownDiscriminant::new("IntegrationVisibility", other)),
        }
    }
}

/// The admin's intended lifecycle for a managed integration.
///
/// Internal to the control plane (no proto counterpart): the start/stop endpoints
/// set it, and the reconciler converges the container toward it. The observable
/// outcome is [`ReconcileStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesiredState {
    Running,
    Stopped,
}

impl DesiredState {
    /// The smallint stored in `integrations.desired` for this state.
    pub fn as_db(self) -> i16 {
        match self {
            Self::Running => 1,
            Self::Stopped => 2,
        }
    }

    /// Parses a stored `integrations.desired` smallint; unknown values are `None`.
    pub fn from_db(value: i16) -> Option<Self> {
        Self::try_from(value).ok()
    }
}

impl TryFrom<i16> for DesiredState {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Running),
            2 => Ok(Self::Stopped),
            other => Err(UnknownDiscriminant::new("DesiredState", other)),
        }
    }
}

/// The live provisioning state of a managed integration, mirroring
/// `elysium.integration.v1.ReconcileStatus`.
///
/// The reconciler walks `Pending -> Provisioning -> Running` (or `Failed`); an
/// explicit stop converges to `Stopped`. `status_detail` carries the reason for
/// `Pending` and `Failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileStatus {
    Pending,
    Provisioning,
    Running,
    Stopped,
    Failed,
}

impl ReconcileStatus {
    /// The smallint stored in `integrations.status` for this status.
    pub fn as_db(self) -> i16 {
        match self {
            Self::Pending => 1,
            Self::Provisioning => 2,
            Self::Running => 3,
            Self::Stopped => 4,
            Self::Failed => 5,
        }
    }

    /// Parses a stored `integrations.status` smallint; unknown values are `None`.
    pub fn from_db(value: i16) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// The wire enum for this status.
    pub fn to_proto(self) -> ProtoStatus {
        match self {
            Self::Pending => ProtoStatus::Pending,
            Self::Provisioning => ProtoStatus::Provisioning,
            Self::Running => ProtoStatus::Running,
            Self::Stopped => ProtoStatus::Stopped,
            Self::Failed => ProtoStatus::Failed,
        }
    }

    /// Parses a wire enum; the `UNSPECIFIED` sentinel is `None`.
    pub fn from_proto(value: ProtoStatus) -> Option<Self> {
        match value {
            ProtoStatus::Pending => Some(Self::Pending),
            ProtoStatus::Provisioning => Some(Self::Provisioning),
            ProtoStatus::Running => Some(Self::Running),
            ProtoStatus::Stopped => Some(Self::Stopped),
            ProtoStatus::Failed => Some(Self::Failed),
            ProtoStatus::Unspecified => None,
        }
    }
}

impl TryFrom<i16> for ReconcileStatus {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Pending),
            2 => Ok(Self::Provisioning),
            3 => Ok(Self::Running),
            4 => Ok(Self::Stopped),
            5 => Ok(Self::Failed),
            other => Err(UnknownDiscriminant::new("ReconcileStatus", other)),
        }
    }
}

// --- Rows --------------------------------------------------------------------

/// An `integrations` row exactly as stored.
///
/// The strongly typed enum fields decode straight from their smallint columns; a
/// value outside the known set fails the row decode with [`UnknownDiscriminant`].
/// `config` is the kind-specific non-secret JSONB. `secret` is the sealed
/// credential blob, opened with [`open_secret`] and never sent to a client.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IntegrationRow {
    pub id: Uuid,
    pub org_id: Uuid,
    /// The owning project; `Some` exactly when `owner_scope` is `Project`.
    pub project_id: Option<Uuid>,
    #[sqlx(try_from = "i16")]
    pub kind: IntegrationKind,
    #[sqlx(try_from = "i16")]
    pub mode: IntegrationMode,
    #[sqlx(try_from = "i16")]
    pub owner_scope: OwnerScope,
    /// The owning principal per `owner_scope`: org, project, or user id.
    pub owner_id: Uuid,
    pub name: String,
    #[sqlx(try_from = "i16")]
    pub visibility: IntegrationVisibility,
    pub config: serde_json::Value,
    pub secret: Option<Vec<u8>>,
    #[sqlx(try_from = "i16")]
    pub desired: DesiredState,
    #[sqlx(try_from = "i16")]
    pub status: ReconcileStatus,
    pub status_detail: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An `integration_storage_bindings` row: one role bound to one Drive.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BindingRow {
    pub integration_id: Uuid,
    pub role: String,
    pub drive_id: Uuid,
    /// Path prefix within the Drive; empty uses the reserved consumer area.
    pub prefix: String,
}

// --- Storage roles and the capability gate -----------------------------------

/// One storage role a managed service kind persists data through.
///
/// This table is the enforced form of the consumer matrix in `docs/storage.md`:
/// each role names the capability its Drive must advertise and the path the
/// resolved volume mounts at inside the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageRole {
    /// The role token bindings use, e.g. `"repos"`.
    pub name: &'static str,
    /// The capability a bound Drive must advertise.
    pub requires: StorageCapability,
    /// Absolute mount point inside the service container.
    pub mount_path: &'static str,
}

/// The storage roles of a kind, in the role -> capability map the console renders.
///
/// The map, per `docs/storage.md` and `docs/integrations.md`:
///
/// | Kind     | Role        | Requires       | Why |
/// |----------|-------------|----------------|-----|
/// | git      | `repos`     | `POSIX_FS`     | git object stores need locking, rename, atomic ref updates |
/// | mail     | `bodies`    | `OBJECT_STORE` | message bodies are content-addressed blobs |
/// | registry | `blobs`     | `OBJECT_STORE` | image layers are content-addressed blobs |
/// | perforce | `metadata`  | `POSIX_FS`     | Astraval metadata physically requires a real filesystem |
/// | perforce | `archive`   | `OBJECT_STORE` | depot archives are blobs |
/// | runner   | `workspace` | `POSIX_FS`     | build workspaces need a real filesystem |
pub fn storage_roles(kind: IntegrationKind) -> &'static [StorageRole] {
    match kind {
        IntegrationKind::Git => &[StorageRole {
            name: "repos",
            requires: StorageCapability::PosixFs,
            mount_path: "/data",
        }],
        IntegrationKind::Mail => &[StorageRole {
            name: "bodies",
            requires: StorageCapability::ObjectStore,
            mount_path: "/var/mail",
        }],
        IntegrationKind::Registry => &[StorageRole {
            name: "blobs",
            requires: StorageCapability::ObjectStore,
            mount_path: "/var/lib/registry",
        }],
        IntegrationKind::Perforce => &[
            StorageRole {
                name: "metadata",
                requires: StorageCapability::PosixFs,
                mount_path: "/astraval/metadata",
            },
            StorageRole {
                name: "archive",
                requires: StorageCapability::ObjectStore,
                mount_path: "/astraval/archive",
            },
        ],
        IntegrationKind::Runner => &[StorageRole {
            name: "workspace",
            requires: StorageCapability::PosixFs,
            mount_path: "/workspace",
        }],
    }
}

/// Looks up a kind's storage role by its token; unknown roles are `None`.
pub fn storage_role(kind: IntegrationKind, name: &str) -> Option<&'static StorageRole> {
    storage_roles(kind).iter().find(|role| role.name == name)
}

/// The capability gate: whether `role` of `kind` may bind to a `drive_kind` Drive.
///
/// Rejects an unknown role for the kind, and a Drive whose advertised
/// capabilities ([`DriveKind::capabilities`]) do not satisfy the role's
/// requirement, each with a user-facing message the handler returns as a `400`.
///
/// # Errors
/// Returns the rejection message when the binding is ineligible.
pub fn check_binding_capability(
    kind: IntegrationKind,
    role: &str,
    drive_kind: DriveKind,
) -> Result<(), String> {
    let Some(role) = storage_role(kind, role) else {
        return Err(format!(
            "unknown storage role {role:?} for a {} integration",
            kind.slug()
        ));
    };
    if !drive_kind.capabilities().contains(&role.requires) {
        return Err(format!(
            "role {:?} requires the {:?} capability, which a {drive_kind:?} Drive does not advertise",
            role.name, role.requires,
        ));
    }
    Ok(())
}

/// Whether a binding prefix is a safe relative path within its Drive.
///
/// Empty is valid (the reserved `.elysium/<kind>/<role>` area is used instead).
/// Otherwise the prefix must be relative, forward-slash separated, and free of
/// empty, `.`, and `..` segments, matching the storage module's traversal rules.
fn valid_binding_prefix(prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    !prefix.starts_with('/')
        && !prefix.contains('\\')
        && prefix
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

// --- Sealed credentials ------------------------------------------------------

/// Seals an integration credential for storage in `integrations.secret`.
///
/// Bound to the integration's id as associated data, so a database dump cannot
/// replay one integration's credential under another. The id is generated before
/// insert so the seal happens exactly once.
pub fn seal_secret(crypto: &Crypto, integration_id: Uuid, secret: &[u8]) -> Vec<u8> {
    crypto.seal(secret, integration_id.as_bytes())
}

/// Opens a sealed integration credential written by [`seal_secret`].
///
/// # Errors
/// Errors when the blob fails to authenticate: a wrong key, tampering, or an id
/// that does not match the one the credential was sealed under.
pub fn open_secret(crypto: &Crypto, integration_id: Uuid, blob: &[u8]) -> eyre::Result<Vec<u8>> {
    Ok(crypto.open(blob, integration_id.as_bytes())?.plaintext)
}

// --- Access resolution -------------------------------------------------------

/// Whether `user_id` may see and use `integration`, the single access rule.
///
/// - **Personal** scope: only the owner. A member's linked Gmail is theirs alone.
/// - **Project** scope: mirrors Drive access. Project access is required
///   ([`resolve_project_access`]); admins and owners (org admins bypass) reach
///   every integration, a member reaches an org-wide one, and a restricted one
///   needs an `integration_access` grant held directly or through a team.
/// - **Org** scope: org owners/admins always; a full member reaches an org-wide
///   one, and a restricted one needs a grant. Guests and suspended members never
///   reach org-scoped integrations.
///
/// # Errors
/// Returns an error if any underlying query fails.
pub async fn can_access_integration(
    conn: &mut PgConnection,
    user_id: Uuid,
    integration: &IntegrationRow,
) -> sqlx::Result<bool> {
    match integration.owner_scope {
        OwnerScope::Personal => Ok(integration.owner_id == user_id),
        OwnerScope::Project => {
            let Some(project_id) = integration.project_id else {
                // A project-scoped row always carries a project (DB CHECK); a row
                // without one is corrupt, so fail closed.
                return Ok(false);
            };
            let Some(role) = resolve_project_access(conn, user_id, project_id).await? else {
                return Ok(false);
            };
            if role >= ProjectRole::Admin
                || integration.visibility == IntegrationVisibility::OrgWide
            {
                return Ok(true);
            }
            has_integration_grant(conn, integration.id, user_id).await
        }
        OwnerScope::Org => {
            let role = match authz::membership(&mut *conn, user_id, integration.org_id).await? {
                Some(entry) if !entry.suspended => entry.role,
                _ => return Ok(false),
            };
            match role {
                OrgRole::Owner | OrgRole::Admin => Ok(true),
                OrgRole::Member if integration.visibility == IntegrationVisibility::OrgWide => {
                    Ok(true)
                }
                OrgRole::Member => has_integration_grant(conn, integration.id, user_id).await,
                OrgRole::Guest => Ok(false),
            }
        }
    }
}

/// Whether a restricted integration grants `user_id` access directly or via a team.
async fn has_integration_grant(
    executor: impl sqlx::PgExecutor<'_>,
    integration_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM integration_access ia
              WHERE ia.integration_id = $1
                AND (
                    (ia.principal = $2 AND ia.principal_id = $4)
                    OR (ia.principal = $3 AND ia.principal_id IN (
                        SELECT team_id FROM team_members WHERE user_id = $4))
                )
         )",
    )
    .bind(integration_id)
    .bind(PRINCIPAL_USER)
    .bind(PRINCIPAL_TEAM)
    .bind(user_id)
    .fetch_one(executor)
    .await
}

/// Whether `user_id` may manage integrations at a scope (the `manage_integrations` gate).
///
/// For a **project-scoped** target (`project_id` is `Some`): a project owner or
/// admin (org owners/admins bypass through [`has_project_access`]) qualifies, as
/// does an ordinary org member with a delegated org-level `manage_integrations`
/// grant. For an **org-scoped** target: an org owner/admin, or a member with the
/// delegated grant. Guests and suspended members never qualify. This mirrors the
/// storage module's `can_manage_storage` exactly.
///
/// # Errors
/// Returns an error if any underlying query fails.
pub async fn can_manage_integrations(
    conn: &mut PgConnection,
    user_id: Uuid,
    org_id: Uuid,
    project_id: Option<Uuid>,
) -> sqlx::Result<bool> {
    if let Some(project_id) = project_id {
        if has_project_access(conn, user_id, project_id, ProjectRole::Admin).await? {
            return Ok(true);
        }
    } else if let Some(entry) = authz::membership(&mut *conn, user_id, org_id).await? {
        if !entry.suspended && entry.role >= OrgRole::Admin {
            return Ok(true);
        }
    }
    // Otherwise only a non-suspended member with the delegated grant qualifies.
    match authz::membership(&mut *conn, user_id, org_id).await? {
        Some(entry) if !entry.suspended && entry.role == OrgRole::Member => {
            has_manage_integrations_grant(&mut *conn, user_id, org_id).await
        }
        _ => Ok(false),
    }
}

/// Whether `user_id` holds a delegated org-level `manage_integrations` grant.
async fn has_manage_integrations_grant(
    executor: impl sqlx::PgExecutor<'_>,
    user_id: Uuid,
    org_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM org_admin_grants
                         WHERE org_id = $1 AND user_id = $2 AND capability = $3)",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(Capability::ManageIntegrations.as_str())
    .fetch_one(executor)
    .await
}

/// Whether `user_id` may manage one specific integration.
///
/// A personal integration is managed only by its owner; org- and project-scoped
/// integrations resolve through [`can_manage_integrations`].
///
/// # Errors
/// Returns an error if any underlying query fails.
pub async fn can_manage_integration(
    conn: &mut PgConnection,
    user_id: Uuid,
    integration: &IntegrationRow,
) -> sqlx::Result<bool> {
    match integration.owner_scope {
        OwnerScope::Personal => Ok(integration.owner_id == user_id),
        OwnerScope::Org | OwnerScope::Project => {
            can_manage_integrations(conn, user_id, integration.org_id, integration.project_id).await
        }
    }
}

/// Whether org policy lets a member link a personal integration of `kind`.
///
/// The enforcement seam every personal-linked creation routes through. It reads
/// the org's per-kind `integration_policies` toggle set
/// ([`crate::orchestration::integration_policies`]); a kind without a row is the
/// documented default: personal linking is off until an admin enables it, per
/// kind (`docs/integrations.md`).
///
/// # Errors
/// Returns an error if the policy lookup fails.
pub async fn member_linking_permitted(
    conn: &mut PgConnection,
    org_id: Uuid,
    kind: IntegrationKind,
) -> sqlx::Result<bool> {
    integration_policies::members_may_link(&mut *conn, org_id, kind).await
}

// --- Model helpers -----------------------------------------------------------

/// The fields of a new integration, grouped so [`insert_integration`] stays tidy.
#[derive(Debug)]
pub struct NewIntegration<'a> {
    pub org_id: Uuid,
    pub project_id: Option<Uuid>,
    pub kind: IntegrationKind,
    pub mode: IntegrationMode,
    pub owner_scope: OwnerScope,
    pub owner_id: Uuid,
    pub name: &'a str,
    pub visibility: IntegrationVisibility,
    pub config: serde_json::Value,
    /// The write-only credential, sealed against the generated id before insert.
    pub credential: Option<&'a str>,
    pub created_by: Uuid,
}

/// Inserts an integration, sealing its credential against the generated id.
///
/// The id is minted here (not by the database default) so the credential can be
/// sealed with the id as associated data in the same statement. New rows start
/// `desired = running`, `status = pending`; the reconciler takes it from there.
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn insert_integration(
    conn: &mut PgConnection,
    crypto: &Crypto,
    new: NewIntegration<'_>,
) -> sqlx::Result<IntegrationRow> {
    let id = Uuid::new_v4();
    let secret = new
        .credential
        .filter(|credential| !credential.is_empty())
        .map(|credential| seal_secret(crypto, id, credential.as_bytes()));

    sqlx::query_as::<_, IntegrationRow>(
        "INSERT INTO integrations
             (id, org_id, project_id, kind, mode, owner_scope, owner_id, name,
              visibility, config, secret, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING *",
    )
    .bind(id)
    .bind(new.org_id)
    .bind(new.project_id)
    .bind(new.kind.as_db())
    .bind(new.mode.as_db())
    .bind(new.owner_scope.as_db())
    .bind(new.owner_id)
    .bind(new.name)
    .bind(new.visibility.as_db())
    .bind(new.config)
    .bind(secret)
    .bind(new.created_by)
    .fetch_one(conn)
    .await
}

/// The storage-role bindings of one integration, ordered by role.
pub async fn bindings_for(
    executor: impl sqlx::PgExecutor<'_>,
    integration_id: Uuid,
) -> sqlx::Result<Vec<BindingRow>> {
    sqlx::query_as::<_, BindingRow>(
        "SELECT integration_id, role, drive_id, prefix
           FROM integration_storage_bindings
          WHERE integration_id = $1
          ORDER BY role ASC",
    )
    .bind(integration_id)
    .fetch_all(executor)
    .await
}

/// The bindings of many integrations at once, for the list endpoint.
async fn bindings_for_many(
    executor: impl sqlx::PgExecutor<'_>,
    integration_ids: &[Uuid],
) -> sqlx::Result<Vec<BindingRow>> {
    sqlx::query_as::<_, BindingRow>(
        "SELECT integration_id, role, drive_id, prefix
           FROM integration_storage_bindings
          WHERE integration_id = ANY($1)
          ORDER BY role ASC",
    )
    .bind(integration_ids)
    .fetch_all(executor)
    .await
}

// --- Management API ----------------------------------------------------------

/// Query for [`list`]: an optional project filter.
#[derive(Deserialize)]
pub struct ListIntegrationsQuery {
    project_id: Option<Uuid>,
}

/// `GET /api/v1/integrations?project_id=`: the integrations the caller can access.
///
/// With `project_id`, the project's integrations (the caller needs project
/// access, so an outsider cannot enumerate them). Without it, every integration
/// in the caller's active org across all owner scopes: org-shared, the projects
/// they reach, and their own personal links. Each row is filtered through
/// [`can_access_integration`], and live `status` + `status_detail` are included.
pub async fn list(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Query(query): Query<ListIntegrationsQuery>,
) -> Response {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "integration list user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "integration list connection acquire failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let rows = match query.project_id {
        Some(project_id) => {
            // No access to the project means no view of its integrations at all.
            match resolve_project_access(&mut conn, user_id, project_id).await {
                Ok(Some(_)) => {}
                Ok(None) => return StatusCode::FORBIDDEN.into_response(),
                Err(error) => {
                    tracing::error!(error = %error, "integration list access resolution failed");
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
            }
            sqlx::query_as::<_, IntegrationRow>(
                "SELECT * FROM integrations WHERE project_id = $1 ORDER BY name ASC",
            )
            .bind(project_id)
            .fetch_all(&state.pool)
            .await
        }
        None => {
            let Some(org_id) = principal.active_org_id else {
                return (StatusCode::BAD_REQUEST, "no active organization").into_response();
            };
            sqlx::query_as::<_, IntegrationRow>(
                "SELECT * FROM integrations WHERE org_id = $1 ORDER BY name ASC",
            )
            .bind(org_id)
            .fetch_all(&state.pool)
            .await
        }
    };
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(error = %error, "integration list query failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let mut visible = Vec::new();
    for row in rows {
        match can_access_integration(&mut conn, user_id, &row).await {
            Ok(true) => visible.push(row),
            Ok(false) => {}
            Err(error) => {
                tracing::error!(error = %error, "integration access resolution failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
    }

    let ids: Vec<Uuid> = visible.iter().map(|row| row.id).collect();
    let bindings = match bindings_for_many(&state.pool, &ids).await {
        Ok(bindings) => bindings,
        Err(error) => {
            tracing::error!(error = %error, "integration binding query failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let integrations = visible
        .iter()
        .map(|row| {
            let own: Vec<&BindingRow> = bindings
                .iter()
                .filter(|binding| binding.integration_id == row.id)
                .collect();
            integration_to_proto(row, &own)
        })
        .collect();
    Protobuf(ListIntegrationsResponse { integrations }).into_response()
}

/// `POST /api/v1/integrations`: create a managed or linked integration.
///
/// Gates by mode and scope: **managed** and **org- or project-linked** creation
/// require [`can_manage_integrations`] at the owning tier; **personal-linked**
/// creation is open to full org members whose org enabled the kind's
/// `members_may_link` toggle, enforced through [`member_linking_permitted`]. A
/// managed personal integration is refused: provisioning is never personal.
/// A managed creation reconciles immediately, so the response carries live status.
pub async fn create(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Protobuf(request): Protobuf<CreateIntegrationRequest>,
) -> Response {
    let name = request.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    let Some(kind) = parse_kind(request.kind) else {
        return (StatusCode::BAD_REQUEST, "kind is required").into_response();
    };
    let Some(mode) = parse_mode(request.mode) else {
        return (StatusCode::BAD_REQUEST, "mode is required").into_response();
    };
    let Some(owner_scope) = parse_scope(request.owner_scope) else {
        return (StatusCode::BAD_REQUEST, "owner_scope is required").into_response();
    };
    // An unset visibility defaults to org-wide, matching Drives.
    let visibility = parse_visibility(request.visibility).unwrap_or(IntegrationVisibility::OrgWide);
    let config = match parse_config_json(&request.config_json) {
        Ok(config) => config,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    if mode == IntegrationMode::Managed && owner_scope == OwnerScope::Personal {
        return (
            StatusCode::BAD_REQUEST,
            "a managed integration is admin infrastructure and cannot be personal",
        )
            .into_response();
    }
    let project_id = request.project_id.as_ref().and_then(parse_uuid);
    if (owner_scope == OwnerScope::Project) != project_id.is_some() {
        return (
            StatusCode::BAD_REQUEST,
            "project_id is required for project scope and must be unset otherwise",
        )
            .into_response();
    }

    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "integration create user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "integration create connection acquire failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Resolve the owning tier and enforce its gate.
    let (org_id, owner_id) = match owner_scope {
        OwnerScope::Project => {
            let project_id = project_id.expect("validated above");
            let org_id = match org_for_project(&state, project_id).await {
                Ok(org_id) => org_id,
                Err(response) => return response,
            };
            match can_manage_integrations(&mut conn, user_id, org_id, Some(project_id)).await {
                Ok(true) => {}
                Ok(false) => return StatusCode::FORBIDDEN.into_response(),
                Err(error) => {
                    tracing::error!(error = %error, "integration authz resolution failed");
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
            }
            (org_id, project_id)
        }
        OwnerScope::Org => {
            let Some(org_id) = principal.active_org_id else {
                return (StatusCode::BAD_REQUEST, "no active organization").into_response();
            };
            match can_manage_integrations(&mut conn, user_id, org_id, None).await {
                Ok(true) => {}
                Ok(false) => return StatusCode::FORBIDDEN.into_response(),
                Err(error) => {
                    tracing::error!(error = %error, "integration authz resolution failed");
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
            }
            (org_id, org_id)
        }
        OwnerScope::Personal => {
            let Some(org_id) = principal.active_org_id else {
                return (StatusCode::BAD_REQUEST, "no active organization").into_response();
            };
            // Linking a personal account requires full membership; guests are
            // scoped visitors and never bring their own services into an org.
            match authz::role_in_org(&state.pool, user_id, org_id).await {
                Ok(Some(role)) if role >= OrgRole::Member => {}
                Ok(_) => return StatusCode::FORBIDDEN.into_response(),
                Err(error) => {
                    tracing::error!(error = %error, "integration authz role lookup failed");
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
            }
            match member_linking_permitted(&mut conn, org_id, kind).await {
                Ok(true) => {}
                Ok(false) => {
                    return (
                        StatusCode::FORBIDDEN,
                        "linking personal integrations of this kind is disabled by org policy",
                    )
                        .into_response()
                }
                Err(error) => {
                    tracing::error!(error = %error, "integration policy lookup failed");
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
            }
            (org_id, user_id)
        }
    };

    let row = match insert_integration(
        &mut conn,
        &state.crypto,
        NewIntegration {
            org_id,
            project_id,
            kind,
            mode,
            owner_scope,
            owner_id,
            name,
            visibility,
            config,
            credential: Some(&request.credential),
            created_by: user_id,
        },
    )
    .await
    {
        Ok(row) => row,
        Err(error) => {
            tracing::error!(error = %error, "integration creation failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    reconcile_and_respond(&state, &mut conn, row.id).await
}

/// `PATCH /api/v1/integrations/{id}`: update name, visibility, config, or secret.
///
/// Kind, mode, scope, project, and org are immutable. An empty field leaves its
/// value unchanged; an empty `credential` keeps the existing sealed secret. A
/// managed integration reconciles after the update so the config change (once the
/// per-kind specs consume it) converges immediately.
pub async fn update(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(integration_id): Path<Uuid>,
    Protobuf(request): Protobuf<UpdateIntegrationRequest>,
) -> Response {
    let (row, mut conn) = match require_manage_target(&state, &principal, integration_id).await {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };

    let name = {
        let trimmed = request.name.trim();
        if trimmed.is_empty() {
            row.name.clone()
        } else {
            trimmed.to_string()
        }
    };
    let visibility = parse_visibility(request.visibility).unwrap_or(row.visibility);
    let config = if request.config_json.trim().is_empty() {
        row.config.clone()
    } else {
        match parse_config_json(&request.config_json) {
            Ok(config) => config,
            Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
        }
    };
    let secret = if request.credential.is_empty() {
        row.secret.clone()
    } else {
        Some(seal_secret(
            &state.crypto,
            row.id,
            request.credential.as_bytes(),
        ))
    };

    let updated = sqlx::query(
        "UPDATE integrations
            SET name = $1, visibility = $2, config = $3, secret = $4, updated_at = now()
          WHERE id = $5",
    )
    .bind(name)
    .bind(visibility.as_db())
    .bind(config)
    .bind(secret)
    .bind(integration_id)
    .execute(&state.pool)
    .await;
    if let Err(error) = updated {
        tracing::error!(error = %error, "integration update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    reconcile_and_respond(&state, &mut conn, integration_id).await
}

/// `DELETE /api/v1/integrations/{id}`: remove an integration.
///
/// For a managed integration the container is removed from the provisioner
/// first; the record is only deleted once the engine no longer runs it, so a
/// failed removal never strands an orphaned container behind a vanished row.
pub async fn delete(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(integration_id): Path<Uuid>,
) -> Response {
    let (row, _conn) = match require_manage_target(&state, &principal, integration_id).await {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };

    if row.mode == IntegrationMode::Managed {
        if let Err(error) = state
            .provisioner
            .remove(&reconcile::service_name(&row))
            .await
        {
            tracing::error!(error = %error, "managed container removal failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    match sqlx::query("DELETE FROM integrations WHERE id = $1")
        .bind(integration_id)
        .execute(&state.pool)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "integration deletion failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /api/v1/integrations/{id}/start`: set desired state to running.
///
/// Managed only. The reconciler converges immediately; the response carries the
/// resulting live status.
pub async fn start(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(integration_id): Path<Uuid>,
) -> Response {
    set_desired(&state, &principal, integration_id, DesiredState::Running).await
}

/// `POST /api/v1/integrations/{id}/stop`: set desired state to stopped.
///
/// Managed only. The container stops but stays provisioned, so a later start is
/// fast and loses nothing.
pub async fn stop(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(integration_id): Path<Uuid>,
) -> Response {
    set_desired(&state, &principal, integration_id, DesiredState::Stopped).await
}

/// Shared start/stop body: gate, persist `desired`, reconcile, respond.
async fn set_desired(
    state: &AppState,
    principal: &Principal,
    integration_id: Uuid,
    desired: DesiredState,
) -> Response {
    let (row, mut conn) = match require_manage_target(state, principal, integration_id).await {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    if row.mode != IntegrationMode::Managed {
        return (
            StatusCode::BAD_REQUEST,
            "only a managed integration has a container lifecycle",
        )
            .into_response();
    }

    let updated =
        sqlx::query("UPDATE integrations SET desired = $1, updated_at = now() WHERE id = $2")
            .bind(desired.as_db())
            .bind(integration_id)
            .execute(&state.pool)
            .await;
    if let Err(error) = updated {
        tracing::error!(error = %error, "integration desired-state update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    reconcile_and_respond(state, &mut conn, integration_id).await
}

/// `PUT /api/v1/integrations/{id}/bindings`: replace the storage-role bindings.
///
/// Managed only. Every binding passes the **capability gate**
/// ([`check_binding_capability`]): the role must exist for the integration's kind
/// and the Drive must advertise the role's required capability. The Drive must
/// also live in the same org (and, for a project-scoped integration, the same
/// project) and be accessible to the caller. The set replaces atomically, then
/// the reconciler converges, so a newly-complete binding set brings the service up.
pub async fn set_bindings(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(integration_id): Path<Uuid>,
    Protobuf(request): Protobuf<SetIntegrationBindingsRequest>,
) -> Response {
    let (row, mut conn) = match require_manage_target(&state, &principal, integration_id).await {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    if row.mode != IntegrationMode::Managed {
        return (
            StatusCode::BAD_REQUEST,
            "only a managed integration binds storage roles",
        )
            .into_response();
    }
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "integration bindings user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Parse and validate every binding up front so a bad one is a clean 400
    // before any write.
    let mut bindings: Vec<(String, Uuid, String)> = Vec::with_capacity(request.bindings.len());
    for binding in &request.bindings {
        let role = binding.role.trim();
        let Some(drive_id) = binding.drive_id.as_ref().and_then(parse_uuid) else {
            return (StatusCode::BAD_REQUEST, "invalid binding drive_id").into_response();
        };
        let prefix = binding.prefix.trim();
        if !valid_binding_prefix(prefix) {
            return (
                StatusCode::BAD_REQUEST,
                "prefix must be a relative path without empty, '.', or '..' segments",
            )
                .into_response();
        }
        if bindings.iter().any(|(existing, ..)| existing == role) {
            return (StatusCode::BAD_REQUEST, "duplicate binding role").into_response();
        }
        bindings.push((role.to_string(), drive_id, prefix.to_string()));
    }

    // The capability gate, plus tenancy and access checks per Drive.
    for (role, drive_id, _) in &bindings {
        let drive = match sqlx::query_as::<_, DriveRow>("SELECT * FROM drives WHERE id = $1")
            .bind(drive_id)
            .fetch_optional(&state.pool)
            .await
        {
            Ok(Some(drive)) => drive,
            Ok(None) => return (StatusCode::BAD_REQUEST, "unknown drive").into_response(),
            Err(error) => {
                tracing::error!(error = %error, "binding drive load failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        };
        if drive.org_id != row.org_id {
            return (
                StatusCode::BAD_REQUEST,
                "drive belongs to another organization",
            )
                .into_response();
        }
        // A project's services persist onto the project's own Drives (#52).
        if let Some(project_id) = row.project_id {
            if drive.project_id != project_id {
                return (StatusCode::BAD_REQUEST, "drive belongs to another project")
                    .into_response();
            }
        }
        match can_access_drive(&mut conn, user_id, &drive).await {
            Ok(true) => {}
            Ok(false) => return StatusCode::FORBIDDEN.into_response(),
            Err(error) => {
                tracing::error!(error = %error, "binding drive access resolution failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
        if let Err(message) = check_binding_capability(row.kind, role, drive.kind) {
            return (StatusCode::BAD_REQUEST, message).into_response();
        }
    }

    // Replace the whole set atomically.
    let outcome = async {
        let mut tx = state.pool.begin().await?;
        sqlx::query("DELETE FROM integration_storage_bindings WHERE integration_id = $1")
            .bind(integration_id)
            .execute(&mut *tx)
            .await?;
        for (role, drive_id, prefix) in &bindings {
            sqlx::query(
                "INSERT INTO integration_storage_bindings (integration_id, role, drive_id, prefix)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(integration_id)
            .bind(role)
            .bind(drive_id)
            .bind(prefix)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
    .await;
    if let Err(error) = outcome {
        tracing::error!(error = %error, "integration binding update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    reconcile_and_respond(&state, &mut conn, integration_id).await
}

// --- Authorization + loading helpers -----------------------------------------

/// Loads an integration and requires the caller may manage it.
///
/// The shared front half of every item-level handler: resolves the row, the
/// caller, and their authority, returning the row plus a pooled connection the
/// handler continues with (reconcile needs one anyway).
///
/// # Errors
/// Returns a ready `Response`: `404` for an unknown id, `401` when the session
/// does not resolve, `403` when the caller lacks authority (indistinguishable
/// from absence for non-members), and `503` on a database error.
async fn require_manage_target(
    state: &AppState,
    principal: &Principal,
    integration_id: Uuid,
) -> Result<(IntegrationRow, sqlx::pool::PoolConnection<sqlx::Postgres>), Response> {
    let row = load_integration(state, integration_id).await?;
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "integration authz user lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "integration authz connection acquire failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    match can_manage_integration(&mut conn, user_id, &row).await {
        Ok(true) => Ok((row, conn)),
        Ok(false) => Err(StatusCode::FORBIDDEN.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "integration authz resolution failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Loads an integration by id, or a ready `404`/`503` response.
async fn load_integration(
    state: &AppState,
    integration_id: Uuid,
) -> Result<IntegrationRow, Response> {
    match sqlx::query_as::<_, IntegrationRow>("SELECT * FROM integrations WHERE id = $1")
        .bind(integration_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(row)) => Ok(row),
        Ok(None) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "integration load failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Resolves a project's org, or a ready `404`/`503` response.
async fn org_for_project(state: &AppState, project_id: Uuid) -> Result<Uuid, Response> {
    match sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(org_id)) => Ok(org_id),
        Ok(None) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "project org lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Reconciles one integration, then answers with its refreshed proto form.
///
/// Every mutating handler funnels through here so the response always carries the
/// live post-change [`ReconcileStatus`]. A reconcile failure is logged but does
/// not fail the request: the change itself committed, and the row's FAILED status
/// carries the story to the console.
async fn reconcile_and_respond(
    state: &AppState,
    conn: &mut PgConnection,
    integration_id: Uuid,
) -> Response {
    if let Err(error) =
        reconcile::reconcile_integration(conn, &state.provisioner, integration_id).await
    {
        tracing::error!(error = %error, "integration reconcile failed");
    }

    let row = match load_integration(state, integration_id).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    let bindings = match bindings_for(&state.pool, integration_id).await {
        Ok(bindings) => bindings,
        Err(error) => {
            tracing::error!(error = %error, "integration binding query failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let refs: Vec<&BindingRow> = bindings.iter().collect();
    Protobuf(integration_to_proto(&row, &refs)).into_response()
}

// --- Proto conversions -------------------------------------------------------

/// Parses a request's `kind` field; `UNSPECIFIED`/unknown is `None`.
fn parse_kind(value: i32) -> Option<IntegrationKind> {
    ProtoKind::try_from(value)
        .ok()
        .and_then(IntegrationKind::from_proto)
}

/// Parses a request's `mode` field; `UNSPECIFIED`/unknown is `None`.
fn parse_mode(value: i32) -> Option<IntegrationMode> {
    ProtoMode::try_from(value)
        .ok()
        .and_then(IntegrationMode::from_proto)
}

/// Parses a request's `owner_scope` field; `UNSPECIFIED`/unknown is `None`.
fn parse_scope(value: i32) -> Option<OwnerScope> {
    ProtoScope::try_from(value)
        .ok()
        .and_then(OwnerScope::from_proto)
}

/// Parses a request's `visibility` field; unset is `None`.
fn parse_visibility(value: i32) -> Option<IntegrationVisibility> {
    ProtoVisibility::try_from(value)
        .ok()
        .and_then(IntegrationVisibility::from_proto)
}

/// Parses a proto [`ProtoUuid`] wrapper into a real [`Uuid`], or `None` when invalid.
fn parse_uuid(id: &ProtoUuid) -> Option<Uuid> {
    Uuid::parse_str(id.value.trim()).ok()
}

/// Parses a request's `config_json` into the stored JSONB value.
///
/// Empty means "no settings" and stores `{}`. Anything else must parse as a JSON
/// **object**: the per-kind config shapes are owned by the per-service
/// milestones, but the envelope is always an object so they can validate fields
/// without a schema migration.
///
/// # Errors
/// Returns a user-facing message when the JSON is malformed or not an object.
fn parse_config_json(raw: &str) -> Result<serde_json::Value, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) if value.is_object() => Ok(value),
        Ok(_) => Err("config_json must be a JSON object"),
        Err(_) => Err("config_json is not valid JSON"),
    }
}

/// Builds the protobuf [`Integration`], exposing `credential_set`, never the secret.
fn integration_to_proto(row: &IntegrationRow, bindings: &[&BindingRow]) -> Integration {
    Integration {
        id: Some(proto_uuid(row.id)),
        project_id: row.project_id.map(proto_uuid),
        org_id: Some(proto_uuid(row.org_id)),
        kind: row.kind.to_proto() as i32,
        mode: row.mode.to_proto() as i32,
        owner_scope: row.owner_scope.to_proto() as i32,
        name: row.name.clone(),
        visibility: row.visibility.to_proto() as i32,
        status: row.status.to_proto() as i32,
        status_detail: row.status_detail.clone(),
        credential_set: row.secret.is_some(),
        config_json: row.config.to_string(),
        bindings: bindings
            .iter()
            .map(|binding| ProtoBinding {
                role: binding.role.clone(),
                drive_id: Some(proto_uuid(binding.drive_id)),
                prefix: binding.prefix.clone(),
            })
            .collect(),
        created_at: Some(to_timestamp(row.created_at)),
        updated_at: Some(to_timestamp(row.updated_at)),
    }
}

/// Wraps a database `Uuid` in its proto string form.
fn proto_uuid(id: Uuid) -> ProtoUuid {
    ProtoUuid {
        value: id.to_string(),
    }
}

/// Converts a UTC instant into the canonical proto `Timestamp`.
fn to_timestamp(instant: DateTime<Utc>) -> Timestamp {
    Timestamp {
        epoch_seconds: instant.timestamp(),
        nanos: instant.timestamp_subsec_nanos(),
        timezone: "Etc/UTC".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    /// A fixed 32-byte key for deterministic crypto tests, as in `crypto.rs`.
    fn test_crypto() -> Crypto {
        Crypto::new(&[9u8; 32]).expect("32-byte key is valid")
    }

    /// Builds a minimal managed git `IntegrationRow` for pure-conversion tests.
    fn git_row(secret: Option<Vec<u8>>) -> IntegrationRow {
        IntegrationRow {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            kind: IntegrationKind::Git,
            mode: IntegrationMode::Managed,
            owner_scope: OwnerScope::Project,
            owner_id: Uuid::new_v4(),
            name: "Forge".to_string(),
            visibility: IntegrationVisibility::OrgWide,
            config: serde_json::json!({}),
            secret,
            desired: DesiredState::Running,
            status: ReconcileStatus::Pending,
            status_detail: String::new(),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // --- Enum wire mappings --------------------------------------------------

    #[test]
    fn integration_kind_db_values_match_the_proto_enum() {
        // 1=git, 2=mail, 3=registry, 4=perforce, 5=runner, mirroring the proto
        // minus its zero UNSPECIFIED sentinel.
        for kind in IntegrationKind::ALL {
            assert_eq!(kind.to_proto() as i32, i32::from(kind.as_db()));
            assert_eq!(IntegrationKind::from_db(kind.as_db()), Some(kind));
            assert_eq!(IntegrationKind::from_proto(kind.to_proto()), Some(kind));
        }
        assert_eq!(IntegrationKind::from_db(0), None);
        assert_eq!(IntegrationKind::from_db(6), None);
        assert_eq!(IntegrationKind::from_proto(ProtoKind::Unspecified), None);
    }

    #[test]
    fn integration_mode_matches_the_proto_enum() {
        for mode in [IntegrationMode::Managed, IntegrationMode::Linked] {
            assert_eq!(mode.to_proto() as i32, i32::from(mode.as_db()));
            assert_eq!(IntegrationMode::from_db(mode.as_db()), Some(mode));
            assert_eq!(IntegrationMode::from_proto(mode.to_proto()), Some(mode));
        }
        assert_eq!(IntegrationMode::from_db(0), None);
        assert_eq!(IntegrationMode::from_proto(ProtoMode::Unspecified), None);
    }

    #[test]
    fn owner_scope_matches_the_proto_enum() {
        // 1=org, 2=project, 3=personal: the project tier slots in as 2 per
        // docs/projects.md, and the DB smallint equals the wire value.
        for scope in [OwnerScope::Org, OwnerScope::Project, OwnerScope::Personal] {
            assert_eq!(scope.to_proto() as i32, i32::from(scope.as_db()));
            assert_eq!(OwnerScope::from_db(scope.as_db()), Some(scope));
            assert_eq!(OwnerScope::from_proto(scope.to_proto()), Some(scope));
        }
        assert_eq!(OwnerScope::from_db(0), None);
        assert_eq!(OwnerScope::from_db(4), None);
        assert_eq!(OwnerScope::from_proto(ProtoScope::Unspecified), None);
    }

    #[test]
    fn integration_visibility_matches_the_proto_enum() {
        for visibility in [
            IntegrationVisibility::OrgWide,
            IntegrationVisibility::Restricted,
        ] {
            assert_eq!(visibility.to_proto() as i32, i32::from(visibility.as_db()));
            assert_eq!(
                IntegrationVisibility::from_db(visibility.as_db()),
                Some(visibility)
            );
            assert_eq!(
                IntegrationVisibility::from_proto(visibility.to_proto()),
                Some(visibility)
            );
        }
        assert_eq!(IntegrationVisibility::from_db(0), None);
        assert_eq!(
            IntegrationVisibility::from_proto(ProtoVisibility::Unspecified),
            None
        );
    }

    #[test]
    fn reconcile_status_matches_the_proto_enum() {
        for status in [
            ReconcileStatus::Pending,
            ReconcileStatus::Provisioning,
            ReconcileStatus::Running,
            ReconcileStatus::Stopped,
            ReconcileStatus::Failed,
        ] {
            assert_eq!(status.to_proto() as i32, i32::from(status.as_db()));
            assert_eq!(ReconcileStatus::from_db(status.as_db()), Some(status));
            assert_eq!(ReconcileStatus::from_proto(status.to_proto()), Some(status));
        }
        assert_eq!(ReconcileStatus::from_db(0), None);
        assert_eq!(ReconcileStatus::from_db(6), None);
        assert_eq!(ReconcileStatus::from_proto(ProtoStatus::Unspecified), None);
    }

    #[test]
    fn desired_state_round_trips_through_the_db() {
        for desired in [DesiredState::Running, DesiredState::Stopped] {
            assert_eq!(DesiredState::from_db(desired.as_db()), Some(desired));
        }
        assert_eq!(DesiredState::from_db(0), None);
        assert_eq!(DesiredState::from_db(3), None);
    }

    // --- The role -> capability map ------------------------------------------

    #[test]
    fn storage_roles_follow_the_documented_map() {
        // git repos and Astraval metadata are POSIX_FS (local-FS-only); registry
        // blobs, mail bodies, and Astraval archives are OBJECT_STORE; runner
        // workspaces are POSIX_FS. See docs/storage.md.
        let expect = |kind, name, requires| {
            let role = storage_role(kind, name).expect("known role");
            assert_eq!(role.requires, requires, "{kind:?} {name}");
            assert!(role.mount_path.starts_with('/'), "mount path is absolute");
        };
        expect(IntegrationKind::Git, "repos", StorageCapability::PosixFs);
        expect(
            IntegrationKind::Mail,
            "bodies",
            StorageCapability::ObjectStore,
        );
        expect(
            IntegrationKind::Registry,
            "blobs",
            StorageCapability::ObjectStore,
        );
        expect(
            IntegrationKind::Perforce,
            "metadata",
            StorageCapability::PosixFs,
        );
        expect(
            IntegrationKind::Perforce,
            "archive",
            StorageCapability::ObjectStore,
        );
        expect(
            IntegrationKind::Runner,
            "workspace",
            StorageCapability::PosixFs,
        );

        // Every kind declares at least one role, and lookups are exact.
        for kind in IntegrationKind::ALL {
            assert!(!storage_roles(kind).is_empty());
            assert_eq!(storage_role(kind, "no-such-role"), None);
        }
    }

    #[test]
    fn capability_gate_admits_only_capable_drives_for_every_pair() {
        // Exhaustive: each (kind, role) x provider pair passes exactly when the
        // provider advertises the role's requirement.
        for kind in IntegrationKind::ALL {
            for role in storage_roles(kind) {
                for drive_kind in [DriveKind::Fs, DriveKind::S3, DriveKind::Sftp] {
                    let eligible = drive_kind.capabilities().contains(&role.requires);
                    let outcome = check_binding_capability(kind, role.name, drive_kind);
                    assert_eq!(
                        outcome.is_ok(),
                        eligible,
                        "{kind:?} {role:?} on {drive_kind:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn capability_gate_rejects_git_repos_on_s3_and_admits_registry_blobs() {
        // The two canonical cases from the issue: registry blobs -> S3 is fine,
        // git repos -> S3 is rejected because S3 lacks POSIX_FS.
        assert!(
            check_binding_capability(IntegrationKind::Registry, "blobs", DriveKind::S3).is_ok()
        );
        let error = check_binding_capability(IntegrationKind::Git, "repos", DriveKind::S3)
            .expect_err("git on S3 must be rejected");
        assert!(
            error.contains("PosixFs"),
            "names the missing capability: {error}"
        );
    }

    #[test]
    fn capability_gate_rejects_unknown_roles() {
        let error = check_binding_capability(IntegrationKind::Git, "blobs", DriveKind::Fs)
            .expect_err("a registry role on a git integration is unknown");
        assert!(error.contains("blobs"));
        assert!(error.contains("git"));
    }

    // --- Input validation ----------------------------------------------------

    #[test]
    fn binding_prefixes_reject_traversal_and_absolute_paths() {
        assert!(valid_binding_prefix(""));
        assert!(valid_binding_prefix("team/git"));
        assert!(valid_binding_prefix("a/b/c"));
        assert!(!valid_binding_prefix("/absolute"));
        assert!(!valid_binding_prefix("../escape"));
        assert!(!valid_binding_prefix("a/../b"));
        assert!(!valid_binding_prefix("a//b"));
        assert!(!valid_binding_prefix("a/./b"));
        assert!(!valid_binding_prefix("windows\\path"));
    }

    #[test]
    fn config_json_must_be_an_object_or_empty() {
        assert_eq!(parse_config_json(""), Ok(serde_json::json!({})));
        assert_eq!(parse_config_json("   "), Ok(serde_json::json!({})));
        assert_eq!(
            parse_config_json(r#"{"domain":"navarrocity.com"}"#),
            Ok(serde_json::json!({ "domain": "navarrocity.com" }))
        );
        assert!(parse_config_json("[1,2]").is_err());
        assert!(parse_config_json("\"text\"").is_err());
        assert!(parse_config_json("{not json").is_err());
    }

    // --- Response shape + secret safety --------------------------------------

    #[test]
    fn integration_to_proto_reports_status_and_credential_flag() {
        let mut row = git_row(None);
        row.status = ReconcileStatus::Failed;
        row.status_detail = "engine unreachable".to_string();

        let proto = integration_to_proto(&row, &[]);
        assert_eq!(proto.kind, ProtoKind::Git as i32);
        assert_eq!(proto.mode, ProtoMode::Managed as i32);
        assert_eq!(proto.owner_scope, ProtoScope::Project as i32);
        assert_eq!(proto.status, ProtoStatus::Failed as i32);
        assert_eq!(proto.status_detail, "engine unreachable");
        assert!(!proto.credential_set, "no secret means credential unset");
        assert!(
            proto.project_id.is_some(),
            "project scope carries its project"
        );
        assert_eq!(proto.config_json, "{}");
    }

    #[test]
    fn integration_to_proto_carries_its_bindings() {
        let row = git_row(None);
        let binding = BindingRow {
            integration_id: row.id,
            role: "repos".to_string(),
            drive_id: Uuid::new_v4(),
            prefix: "forge".to_string(),
        };
        let proto = integration_to_proto(&row, &[&binding]);
        assert_eq!(proto.bindings.len(), 1);
        assert_eq!(proto.bindings[0].role, "repos");
        assert_eq!(proto.bindings[0].prefix, "forge");
    }

    #[test]
    fn a_sealed_secret_never_appears_in_the_encoded_integration() {
        let crypto = test_crypto();
        let id = Uuid::new_v4();
        let plaintext = b"github-personal-access-token";
        let sealed = seal_secret(&crypto, id, plaintext);

        let mut row = git_row(Some(sealed.clone()));
        row.id = id;

        let proto = integration_to_proto(&row, &[]);
        assert!(proto.credential_set, "a sealed secret sets the flag");

        let mut buffer = Vec::new();
        proto.encode(&mut buffer).expect("encode Integration");
        assert!(
            !contains_subslice(&buffer, plaintext),
            "the plaintext secret must not be in the response",
        );
        assert!(
            !contains_subslice(&buffer, &sealed),
            "the sealed secret blob must not be in the response",
        );
    }

    #[test]
    fn integration_secret_seal_then_open_round_trips() {
        let crypto = test_crypto();
        let id = Uuid::new_v4();
        let sealed = seal_secret(&crypto, id, b"api-token");
        assert_eq!(
            open_secret(&crypto, id, &sealed).expect("opens"),
            b"api-token"
        );
        // Sealed for one integration, never openable as another's.
        assert!(open_secret(&crypto, Uuid::new_v4(), &sealed).is_err());
    }

    /// Whether `haystack` contains `needle` as a contiguous subslice.
    fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    // --- DB-backed round-trip + authz matrix ---------------------------------
    //
    // Ignored by default (needs a reachable postgres), like `tests/migrations.rs`
    // and the storage/projects matrices. Run with `cargo test -- --ignored` and a
    // DATABASE_URL. Everything happens inside a rolled-back transaction.

    use crate::projects::add_member;

    async fn connect_test_pool() -> sqlx::PgPool {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");
        pool
    }

    async fn insert_user(conn: &mut PgConnection, tag: &str) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("{tag}-{unique}"))
        .bind(format!("{tag}-{unique}@example.test"))
        .fetch_one(conn)
        .await
        .expect("insert user")
    }

    async fn insert_org(conn: &mut PgConnection, tag: &str) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>("INSERT INTO orgs (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(format!("{tag} Org"))
            .bind(format!("{tag}-{unique}"))
            .fetch_one(conn)
            .await
            .expect("insert org")
    }

    async fn add_org_member(conn: &mut PgConnection, org: Uuid, user: Uuid, role: &str) {
        sqlx::query("INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, $3)")
            .bind(user)
            .bind(org)
            .bind(role)
            .execute(conn)
            .await
            .expect("add org member");
    }

    async fn insert_project(conn: &mut PgConnection, org: Uuid, slug: &str) -> Uuid {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO projects (org_id, name, slug) VALUES ($1, $2, $2) RETURNING id",
        )
        .bind(org)
        .bind(slug)
        .fetch_one(conn)
        .await
        .expect("insert project")
    }

    /// Inserts a project-scoped managed integration through the real write path.
    async fn insert_test_integration(
        conn: &mut PgConnection,
        org: Uuid,
        project: Uuid,
        creator: Uuid,
        name: &str,
        visibility: IntegrationVisibility,
    ) -> IntegrationRow {
        insert_integration(
            conn,
            &test_crypto(),
            NewIntegration {
                org_id: org,
                project_id: Some(project),
                kind: IntegrationKind::Git,
                mode: IntegrationMode::Managed,
                owner_scope: OwnerScope::Project,
                owner_id: project,
                name,
                visibility,
                config: serde_json::json!({}),
                credential: Some("admin-credential"),
                created_by: creator,
            },
        )
        .await
        .expect("insert integration")
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn integration_rows_round_trip_with_status_and_sealed_secret() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "orch").await;
        let project = insert_project(&mut tx, org, "proj").await;
        let admin = insert_user(&mut tx, "admin").await;
        add_org_member(&mut tx, org, admin, "admin").await;

        let row = insert_test_integration(
            &mut tx,
            org,
            project,
            admin,
            "Forge",
            IntegrationVisibility::OrgWide,
        )
        .await;

        // Fresh rows carry the reconciler's starting point: desired running,
        // status pending, and a sealed (never plaintext) credential.
        assert_eq!(row.kind, IntegrationKind::Git);
        assert_eq!(row.mode, IntegrationMode::Managed);
        assert_eq!(row.owner_scope, OwnerScope::Project);
        assert_eq!(row.desired, DesiredState::Running);
        assert_eq!(row.status, ReconcileStatus::Pending);
        assert_eq!(row.status_detail, "");
        let sealed = row.secret.as_deref().expect("credential sealed");
        assert_ne!(sealed, b"admin-credential");
        assert_eq!(
            open_secret(&test_crypto(), row.id, sealed).expect("opens"),
            b"admin-credential",
        );

        // The stored row decodes back through the typed enums identically.
        let loaded =
            sqlx::query_as::<_, IntegrationRow>("SELECT * FROM integrations WHERE id = $1")
                .bind(row.id)
                .fetch_one(&mut *tx)
                .await
                .expect("reload integration");
        assert_eq!(loaded.kind, row.kind);
        assert_eq!(loaded.status, row.status);
        assert_eq!(loaded.name, "Forge");
        assert_eq!(loaded.project_id, Some(project));

        tx.rollback().await.expect("rollback transaction");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn access_covers_every_scope_visibility_and_tier() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "acc").await;
        let project = insert_project(&mut tx, org, "proj").await;
        let admin = insert_user(&mut tx, "admin").await;
        add_org_member(&mut tx, org, admin, "admin").await;
        let member = insert_user(&mut tx, "member").await;
        add_org_member(&mut tx, org, member, "member").await;

        // Project-scoped, org-wide: the admin bypasses, the member cascades in.
        let open = insert_test_integration(
            &mut tx,
            org,
            project,
            admin,
            "open",
            IntegrationVisibility::OrgWide,
        )
        .await;
        assert!(can_access_integration(&mut tx, admin, &open).await.unwrap());
        assert!(can_access_integration(&mut tx, member, &open)
            .await
            .unwrap());

        // Restricted: closed to a plain member until a direct grant opens it.
        let locked = insert_test_integration(
            &mut tx,
            org,
            project,
            admin,
            "locked",
            IntegrationVisibility::Restricted,
        )
        .await;
        assert!(can_access_integration(&mut tx, admin, &locked)
            .await
            .unwrap());
        assert!(!can_access_integration(&mut tx, member, &locked)
            .await
            .unwrap());
        sqlx::query(
            "INSERT INTO integration_access (integration_id, principal, principal_id)
             VALUES ($1, $2, $3)",
        )
        .bind(locked.id)
        .bind(PRINCIPAL_USER)
        .bind(member)
        .execute(&mut *tx)
        .await
        .expect("grant access");
        assert!(can_access_integration(&mut tx, member, &locked)
            .await
            .unwrap());

        // Personal scope: the owner alone, even against an org admin.
        let personal = insert_integration(
            &mut tx,
            &test_crypto(),
            NewIntegration {
                org_id: org,
                project_id: None,
                kind: IntegrationKind::Mail,
                mode: IntegrationMode::Linked,
                owner_scope: OwnerScope::Personal,
                owner_id: member,
                name: "My Gmail",
                visibility: IntegrationVisibility::OrgWide,
                config: serde_json::json!({}),
                credential: None,
                created_by: member,
            },
        )
        .await
        .expect("insert personal integration");
        assert!(can_access_integration(&mut tx, member, &personal)
            .await
            .unwrap());
        assert!(!can_access_integration(&mut tx, admin, &personal)
            .await
            .unwrap());

        // A guest with no project grant reaches nothing, even org-wide.
        let guest = insert_user(&mut tx, "guest").await;
        add_org_member(&mut tx, org, guest, "guest").await;
        assert!(!can_access_integration(&mut tx, guest, &open).await.unwrap());

        tx.rollback().await.expect("rollback transaction");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn manage_integrations_gates_on_admin_or_delegation() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "gate").await;
        let project = insert_project(&mut tx, org, "proj").await;

        // A plain org member may not create managed integrations, at either the
        // project or the org tier: this is the "member denied managed create" gate.
        let plain = insert_user(&mut tx, "plain").await;
        add_org_member(&mut tx, org, plain, "member").await;
        assert!(!can_manage_integrations(&mut tx, plain, org, Some(project))
            .await
            .unwrap());
        assert!(!can_manage_integrations(&mut tx, plain, org, None)
            .await
            .unwrap());

        // The same member with a delegated org-level grant may.
        sqlx::query(
            "INSERT INTO org_admin_grants (org_id, user_id, capability)
             VALUES ($1, $2, 'manage_integrations')",
        )
        .bind(org)
        .bind(plain)
        .execute(&mut *tx)
        .await
        .expect("delegate manage_integrations");
        assert!(can_manage_integrations(&mut tx, plain, org, Some(project))
            .await
            .unwrap());
        assert!(can_manage_integrations(&mut tx, plain, org, None)
            .await
            .unwrap());

        // A project admin (a scoped guest raised to Admin) manages their
        // project's integrations, but not org-scoped ones.
        let project_admin = insert_user(&mut tx, "padmin").await;
        add_org_member(&mut tx, org, project_admin, "guest").await;
        add_member(&mut tx, project, project_admin, ProjectRole::Admin)
            .await
            .unwrap();
        assert!(
            can_manage_integrations(&mut tx, project_admin, org, Some(project))
                .await
                .unwrap()
        );
        assert!(!can_manage_integrations(&mut tx, project_admin, org, None)
            .await
            .unwrap());

        // An org admin always may; a guest never may.
        let org_admin = insert_user(&mut tx, "oadmin").await;
        add_org_member(&mut tx, org, org_admin, "admin").await;
        assert!(
            can_manage_integrations(&mut tx, org_admin, org, Some(project))
                .await
                .unwrap()
        );
        assert!(can_manage_integrations(&mut tx, org_admin, org, None)
            .await
            .unwrap());

        let guest = insert_user(&mut tx, "guest").await;
        add_org_member(&mut tx, org, guest, "guest").await;
        assert!(!can_manage_integrations(&mut tx, guest, org, Some(project))
            .await
            .unwrap());

        tx.rollback().await.expect("rollback transaction");
    }
}
