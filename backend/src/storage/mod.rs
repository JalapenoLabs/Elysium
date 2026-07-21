//! Storage domain: the project-scoped Drive gateway over many providers.
//!
//! Storage is a gateway, not a sync engine and not a filesystem re-implementation.
//! A **Drive** is one [OpenDAL](https://opendal.apache.org/) operator rooted where
//! the org points it, plus Elysium metadata; the provider is always the source of
//! truth. Drives are project-scoped (see `docs/storage.md`, `docs/projects.md`):
//! every Drive carries a `project_id` and access resolves through the
//! org -> project -> team hierarchy.
//!
//! This module holds the persistence contract the operator layer and the HTTP
//! surface (#21) build on: the [`DriveRow`] record type, the [`DriveKind`],
//! [`DriveVisibility`], and [`DrivePrincipal`] enums that mirror the
//! `elysium.storage.v1` proto wire values, per-kind [`DriveConfig`] validation, and
//! the [`seal_secret`]/[`open_secret`] helpers that seal a Drive's provider
//! credential with the same [`crate::crypto`] scheme as TOTP secrets.
//!
//! The [`provider`] submodule turns a validated [`DriveConfig`] plus its opened
//! secret into a live [`opendal::Operator`] and tests a connection before it is
//! saved. Nothing here is referenced by the binary yet; #21 wires it into the API.
//! The module-wide `dead_code` allowance keeps the schema and its helpers landed and
//! unit-tested ahead of those consumers, without warning on a binary build that does
//! not call them yet.
#![allow(dead_code)]

pub mod branding;
pub mod defaults;
pub mod drives;
pub mod files;
pub mod provider;
pub mod search;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::Crypto;
use crate::proto::storage::v1::{
    drive_access::Principal as ProtoPrincipal, DriveKind as ProtoDriveKind,
    DriveVisibility as ProtoVisibility, StorageCapability,
};

/// The default per-file upload cap, in bytes (5 GiB), mirrored by the migration.
///
/// A DB-backed org storage setting, not a memory limit: transfers stream through the
/// backend, so the cap is a policy choice an admin can raise (see `docs/storage.md`).
pub const DEFAULT_MAX_UPLOAD_BYTES: i64 = 5_368_709_120;

/// The conventional SFTP port, used when a Drive's config omits one.
const DEFAULT_SFTP_PORT: u16 = 22;

/// A stored smallint fell outside a storage enum's known set of values.
///
/// Surfaced when decoding a `drives` row whose `kind` or `visibility` column, or a
/// `drive_access` row whose `principal` column, holds a value no current enum
/// variant maps to. A `CHECK` constraint makes this unreachable for rows this code
/// writes; it exists so a corrupt or hand-edited row fails loudly rather than
/// silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownDiscriminant {
    /// The enum that rejected the value, e.g. `"DriveKind"`.
    enum_name: &'static str,
    /// The offending stored value.
    value: i16,
}

impl UnknownDiscriminant {
    /// Records that `enum_name` rejected the stored `value`.
    ///
    /// Shared with the orchestration enums (`crate::orchestration::integrations`),
    /// whose smallint columns follow the same decode-or-fail-loudly rule.
    pub(crate) fn new(enum_name: &'static str, value: i16) -> Self {
        Self { enum_name, value }
    }
}

impl std::fmt::Display for UnknownDiscriminant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown {} discriminant: {}", self.enum_name, self.value)
    }
}

impl std::error::Error for UnknownDiscriminant {}

/// The provider a Drive is backed by, mirroring `elysium.storage.v1.DriveKind`.
///
/// Persisted as the smallint from [`DriveKind::as_db`]; the proto's zero
/// `UNSPECIFIED` value is never stored. The kind is immutable for the life of a
/// Drive and determines both the OpenDAL service and the capabilities the provider
/// advertises (see [`DriveKind::capabilities`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveKind {
    /// A directory on the host's local filesystem (`services-fs`).
    Fs,
    /// Any S3-compatible bucket (`services-s3`).
    S3,
    /// A path on a remote machine reached over SSH (`services-sftp`).
    Sftp,
}

impl DriveKind {
    /// The smallint stored in `drives.kind` for this kind.
    pub fn as_db(self) -> i16 {
        match self {
            Self::Fs => 1,
            Self::S3 => 2,
            Self::Sftp => 3,
        }
    }

    /// Parses a stored `drives.kind` smallint back into a kind.
    ///
    /// Returns `None` for any value outside the known set, including the proto's zero
    /// `UNSPECIFIED` sentinel, which is never persisted.
    pub fn from_db(value: i16) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// The wire enum for this kind.
    pub fn to_proto(self) -> ProtoDriveKind {
        match self {
            Self::Fs => ProtoDriveKind::Fs,
            Self::S3 => ProtoDriveKind::S3,
            Self::Sftp => ProtoDriveKind::Sftp,
        }
    }

    /// Parses a wire enum into a kind; the `UNSPECIFIED` sentinel is `None`.
    pub fn from_proto(value: ProtoDriveKind) -> Option<Self> {
        match value {
            ProtoDriveKind::Fs => Some(Self::Fs),
            ProtoDriveKind::S3 => Some(Self::S3),
            ProtoDriveKind::Sftp => Some(Self::Sftp),
            ProtoDriveKind::Unspecified => None,
        }
    }

    /// The capabilities this kind's provider advertises, per the `docs/storage.md`
    /// capability table.
    ///
    /// Every provider serves `PLAIN_FILES`; only FS and S3 are content-addressed
    /// object stores; only local FS is a real POSIX filesystem (which is why git and
    /// Astraval metadata are local-FS-only). Consumers bind a storage role only to a
    /// Drive whose kind advertises the role's required capability.
    pub fn capabilities(self) -> &'static [StorageCapability] {
        match self {
            Self::Fs => &[
                StorageCapability::PlainFiles,
                StorageCapability::ObjectStore,
                StorageCapability::PosixFs,
            ],
            Self::S3 => &[
                StorageCapability::PlainFiles,
                StorageCapability::ObjectStore,
            ],
            Self::Sftp => &[StorageCapability::PlainFiles],
        }
    }
}

impl TryFrom<i16> for DriveKind {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Fs),
            2 => Ok(Self::S3),
            3 => Ok(Self::Sftp),
            other => Err(UnknownDiscriminant {
                enum_name: "DriveKind",
                value: other,
            }),
        }
    }
}

/// Who inside the org may see and use a Drive, mirroring `DriveVisibility`.
///
/// The org boundary is absolute; visibility is only ever a choice within the org.
/// Admins and up always have access, enforced in the authz check and never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveVisibility {
    /// Every member of the org can see and use the Drive.
    OrgWide,
    /// Access is granted to a chosen mix of teams and users via `drive_access` rows.
    Restricted,
}

impl DriveVisibility {
    /// The smallint stored in `drives.visibility` for this visibility.
    pub fn as_db(self) -> i16 {
        match self {
            Self::OrgWide => 1,
            Self::Restricted => 2,
        }
    }

    /// Parses a stored `drives.visibility` smallint; unknown values are `None`.
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

impl TryFrom<i16> for DriveVisibility {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::OrgWide),
            2 => Ok(Self::Restricted),
            other => Err(UnknownDiscriminant {
                enum_name: "DriveVisibility",
                value: other,
            }),
        }
    }
}

/// The kind of principal a `drive_access` grant targets, mirroring
/// `DriveAccess.Principal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrivePrincipal {
    /// The grant targets a team; every member inherits access.
    Team,
    /// The grant targets a single user.
    User,
}

impl DrivePrincipal {
    /// The smallint stored in `drive_access.principal` for this principal.
    pub fn as_db(self) -> i16 {
        match self {
            Self::Team => 1,
            Self::User => 2,
        }
    }

    /// Parses a stored `drive_access.principal` smallint; unknown values are `None`.
    pub fn from_db(value: i16) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// The wire enum for this principal.
    pub fn to_proto(self) -> ProtoPrincipal {
        match self {
            Self::Team => ProtoPrincipal::Team,
            Self::User => ProtoPrincipal::User,
        }
    }

    /// Parses a wire enum; the `UNSPECIFIED` sentinel is `None`.
    pub fn from_proto(value: ProtoPrincipal) -> Option<Self> {
        match value {
            ProtoPrincipal::Team => Some(Self::Team),
            ProtoPrincipal::User => Some(Self::User),
            ProtoPrincipal::Unspecified => None,
        }
    }
}

impl TryFrom<i16> for DrivePrincipal {
    type Error = UnknownDiscriminant;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Team),
            2 => Ok(Self::User),
            other => Err(UnknownDiscriminant {
                enum_name: "DriveAccess.Principal",
                value: other,
            }),
        }
    }
}

/// A `drives` row exactly as stored.
///
/// The strongly typed `kind` and `visibility` fields decode straight from their
/// smallint columns via [`DriveKind`]/[`DriveVisibility`]; a value outside the known
/// set fails the row decode with [`UnknownDiscriminant`]. `config` is the raw
/// non-secret JSONB, parsed on demand with [`DriveConfig::parse`]. `secret` is the
/// sealed credential blob (`None` for an FS Drive), opened with [`open_secret`].
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DriveRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    #[sqlx(try_from = "i16")]
    pub kind: DriveKind,
    pub config: serde_json::Value,
    pub secret: Option<Vec<u8>>,
    #[sqlx(try_from = "i16")]
    pub visibility: DriveVisibility,
    pub is_default: bool,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DriveRow {
    /// Parses this row's non-secret `config` JSONB against its declared [`kind`].
    ///
    /// [`kind`]: DriveRow::kind
    ///
    /// # Errors
    /// Returns a [`ConfigError`] when the stored JSON does not match the kind's shape
    /// or fails validation.
    pub fn parse_config(&self) -> Result<DriveConfig, ConfigError> {
        DriveConfig::parse(self.kind, &self.config)
    }
}

/// Local-filesystem connection settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FsConfig {
    /// Absolute host path the operator is rooted at.
    pub root: String,
}

/// S3-compatible bucket connection settings.
///
/// Only `bucket` is required; an empty `endpoint`/`region` uses the provider default,
/// and an empty `prefix` roots the Drive at the bucket root. None of these are
/// secret: the access key and secret key are sealed separately (see [`seal_secret`]).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct S3Config {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub region: String,
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    /// Request server-side encryption (SSE). A non-secret provider setting.
    #[serde(default)]
    pub sse: bool,
}

/// SFTP-over-SSH connection settings.
///
/// The password or private key is never modeled here; it is sealed separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SftpConfig {
    /// Remote host name or address.
    pub host: String,
    /// TCP port; defaults to [`DEFAULT_SFTP_PORT`] when the stored config omits one.
    #[serde(default = "default_sftp_port")]
    pub port: u16,
    /// SSH user name.
    pub user: String,
    /// Remote path the operator is rooted at.
    pub base_path: String,
}

/// Serde default for [`SftpConfig::port`].
fn default_sftp_port() -> u16 {
    DEFAULT_SFTP_PORT
}

/// A Drive's validated, non-secret connection settings, one variant per kind.
///
/// Stored as the `drives.config` JSONB column, shaped as a flat object of the active
/// variant's fields (no discriminant: the row's `kind` column selects the variant).
/// [`DriveConfig::parse`] is the single validation gate every write goes through
/// before an OpenDAL operator is built from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveConfig {
    Fs(FsConfig),
    S3(S3Config),
    Sftp(SftpConfig),
}

impl DriveConfig {
    /// The kind this config belongs to.
    pub fn kind(&self) -> DriveKind {
        match self {
            Self::Fs(_) => DriveKind::Fs,
            Self::S3(_) => DriveKind::S3,
            Self::Sftp(_) => DriveKind::Sftp,
        }
    }

    /// Parses and validates non-secret config JSON against a Drive's `kind`.
    ///
    /// The JSON is the flat per-kind object stored in `drives.config`. Parsing fails
    /// when the JSON does not deserialize into the kind's shape, and validation fails
    /// when a required field is empty or a value is out of range (for example an FS
    /// Drive without an absolute `root`, an S3 Drive without a `bucket`, or an SFTP
    /// Drive without a `host`, `user`, `base_path`, or a usable `port`).
    ///
    /// # Errors
    /// Returns [`ConfigError::Malformed`] when the JSON shape is wrong and
    /// [`ConfigError::Invalid`] when a value fails validation.
    pub fn parse(kind: DriveKind, value: &serde_json::Value) -> Result<Self, ConfigError> {
        match kind {
            DriveKind::Fs => {
                let config: FsConfig = deserialize(kind, value)?;
                require_absolute(kind, "root", &config.root)?;
                Ok(Self::Fs(config))
            }
            DriveKind::S3 => {
                let config: S3Config = deserialize(kind, value)?;
                require_present(kind, "bucket", &config.bucket)?;
                Ok(Self::S3(config))
            }
            DriveKind::Sftp => {
                let config: SftpConfig = deserialize(kind, value)?;
                require_present(kind, "host", &config.host)?;
                require_present(kind, "user", &config.user)?;
                require_present(kind, "base_path", &config.base_path)?;
                if config.port == 0 {
                    return Err(ConfigError::Invalid {
                        kind,
                        message: "port must be between 1 and 65535".to_string(),
                    });
                }
                Ok(Self::Sftp(config))
            }
        }
    }

    /// Renders this config as the flat JSON object stored in `drives.config`.
    ///
    /// # Panics
    /// Never in practice: these plain structs always serialize to a JSON object.
    pub fn to_json(&self) -> serde_json::Value {
        let value = match self {
            Self::Fs(config) => serde_json::to_value(config),
            Self::S3(config) => serde_json::to_value(config),
            Self::Sftp(config) => serde_json::to_value(config),
        };
        value.expect("Drive config structs always serialize to JSON")
    }
}

/// Deserializes a JSON value into a per-kind config struct, mapping the error.
fn deserialize<T: for<'de> Deserialize<'de>>(
    kind: DriveKind,
    value: &serde_json::Value,
) -> Result<T, ConfigError> {
    serde_json::from_value(value.clone()).map_err(|source| ConfigError::Malformed { kind, source })
}

/// Rejects an empty required string field.
fn require_present(kind: DriveKind, field: &'static str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::Invalid {
            kind,
            message: format!("{field} is required"),
        });
    }
    Ok(())
}

/// Rejects a required path that is empty or not an absolute POSIX path.
///
/// Drives root on the deployment host's POSIX filesystem, so an absolute path is one
/// beginning with `/`. Checking the leading slash keeps validation deterministic
/// across host platforms rather than deferring to `Path::is_absolute`, whose meaning
/// differs on Windows.
fn require_absolute(kind: DriveKind, field: &'static str, value: &str) -> Result<(), ConfigError> {
    require_present(kind, field, value)?;
    if !value.starts_with('/') {
        return Err(ConfigError::Invalid {
            kind,
            message: format!("{field} must be an absolute path"),
        });
    }
    Ok(())
}

/// Why a Drive's non-secret config failed [`DriveConfig::parse`].
///
/// Both variants are user-facing: the HTTP surface (#21) turns them into a `400` with
/// the [`Display`] message. [`ConfigError::Malformed`] means the JSON did not fit the
/// kind's shape; [`ConfigError::Invalid`] means it fit but a value was empty or out of
/// range.
///
/// [`Display`]: std::fmt::Display
#[derive(Debug)]
pub enum ConfigError {
    /// The JSON did not deserialize into the kind's config shape.
    Malformed {
        kind: DriveKind,
        source: serde_json::Error,
    },
    /// The config parsed but a value failed validation.
    Invalid { kind: DriveKind, message: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed { kind, source } => {
                write!(f, "malformed {kind:?} Drive config: {source}")
            }
            Self::Invalid { kind, message } => {
                write!(f, "invalid {kind:?} Drive config: {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed { source, .. } => Some(source),
            Self::Invalid { .. } => None,
        }
    }
}

/// Seals a Drive's provider credential for storage in `drives.secret`.
///
/// Mirrors the TOTP-secret pattern (see [`crate::crypto`]): the ciphertext is bound
/// to the Drive's `project_id` as associated data, so a database dump cannot replay
/// one project's credential under another project's Drive. FS Drives have no
/// credential and store `NULL` instead of calling this.
///
/// The same `project_id` must be supplied to [`open_secret`].
pub fn seal_secret(crypto: &Crypto, project_id: Uuid, secret: &[u8]) -> Vec<u8> {
    crypto.seal(secret, project_id.as_bytes())
}

/// Opens a sealed Drive credential written by [`seal_secret`].
///
/// # Errors
/// Errors when the blob fails to authenticate, which means a wrong key, tampering, or
/// a `project_id` that does not match the one the credential was sealed under.
pub fn open_secret(crypto: &Crypto, project_id: Uuid, blob: &[u8]) -> eyre::Result<Vec<u8>> {
    Ok(crypto.open(blob, project_id.as_bytes())?.plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A fixed 32-byte key for deterministic crypto tests, as in `crypto.rs`.
    fn test_crypto() -> Crypto {
        Crypto::new(&[9u8; 32]).expect("32-byte key is valid")
    }

    // --- Enum wire mappings -------------------------------------------------

    #[test]
    fn drive_kind_db_values_match_the_proto_enum() {
        // 1=FS, 2=S3, 3=SFTP, mirroring elysium.storage.v1.DriveKind minus its zero
        // UNSPECIFIED sentinel.
        assert_eq!(DriveKind::Fs.as_db(), 1);
        assert_eq!(DriveKind::S3.as_db(), 2);
        assert_eq!(DriveKind::Sftp.as_db(), 3);
        for kind in [DriveKind::Fs, DriveKind::S3, DriveKind::Sftp] {
            assert_eq!(kind.to_proto() as i32, i32::from(kind.as_db()));
        }
    }

    #[test]
    fn drive_kind_parses_known_values_only() {
        assert_eq!(DriveKind::from_db(1), Some(DriveKind::Fs));
        assert_eq!(DriveKind::from_db(2), Some(DriveKind::S3));
        assert_eq!(DriveKind::from_db(3), Some(DriveKind::Sftp));
        // The proto UNSPECIFIED sentinel and any unknown value are rejected.
        assert_eq!(DriveKind::from_db(0), None);
        assert_eq!(DriveKind::from_db(4), None);
        assert!(DriveKind::try_from(9).is_err());
    }

    #[test]
    fn drive_kind_round_trips_through_db_and_proto() {
        for kind in [DriveKind::Fs, DriveKind::S3, DriveKind::Sftp] {
            assert_eq!(DriveKind::from_db(kind.as_db()), Some(kind));
            assert_eq!(DriveKind::from_proto(kind.to_proto()), Some(kind));
        }
        assert_eq!(DriveKind::from_proto(ProtoDriveKind::Unspecified), None);
    }

    #[test]
    fn drive_visibility_matches_the_proto_enum() {
        assert_eq!(DriveVisibility::OrgWide.as_db(), 1);
        assert_eq!(DriveVisibility::Restricted.as_db(), 2);
        for visibility in [DriveVisibility::OrgWide, DriveVisibility::Restricted] {
            assert_eq!(visibility.to_proto() as i32, i32::from(visibility.as_db()));
            assert_eq!(
                DriveVisibility::from_db(visibility.as_db()),
                Some(visibility)
            );
            assert_eq!(
                DriveVisibility::from_proto(visibility.to_proto()),
                Some(visibility)
            );
        }
        assert_eq!(DriveVisibility::from_db(0), None);
        assert_eq!(
            DriveVisibility::from_proto(ProtoVisibility::Unspecified),
            None
        );
    }

    #[test]
    fn drive_principal_matches_the_proto_enum() {
        assert_eq!(DrivePrincipal::Team.as_db(), 1);
        assert_eq!(DrivePrincipal::User.as_db(), 2);
        for principal in [DrivePrincipal::Team, DrivePrincipal::User] {
            assert_eq!(principal.to_proto() as i32, i32::from(principal.as_db()));
            assert_eq!(DrivePrincipal::from_db(principal.as_db()), Some(principal));
            assert_eq!(
                DrivePrincipal::from_proto(principal.to_proto()),
                Some(principal)
            );
        }
        assert_eq!(DrivePrincipal::from_db(0), None);
        assert_eq!(
            DrivePrincipal::from_proto(ProtoPrincipal::Unspecified),
            None
        );
    }

    #[test]
    fn capabilities_follow_the_docs_table() {
        // Every provider serves plain files; only FS and S3 are object stores; only
        // FS is a real POSIX filesystem (docs/storage.md).
        assert_eq!(
            DriveKind::Fs.capabilities(),
            &[
                StorageCapability::PlainFiles,
                StorageCapability::ObjectStore,
                StorageCapability::PosixFs,
            ]
        );
        assert_eq!(
            DriveKind::S3.capabilities(),
            &[
                StorageCapability::PlainFiles,
                StorageCapability::ObjectStore
            ]
        );
        assert_eq!(
            DriveKind::Sftp.capabilities(),
            &[StorageCapability::PlainFiles]
        );
    }

    // --- Per-kind config validation -----------------------------------------

    #[test]
    fn valid_fs_config_parses_and_round_trips() {
        let value = json!({ "root": "/srv/elysium/data" });
        let config = DriveConfig::parse(DriveKind::Fs, &value).expect("valid FS config");
        assert_eq!(config.kind(), DriveKind::Fs);
        assert_eq!(
            config,
            DriveConfig::Fs(FsConfig {
                root: "/srv/elysium/data".to_string(),
            })
        );
        // Serializing back yields the same flat object it parsed from.
        assert_eq!(
            DriveConfig::parse(DriveKind::Fs, &config.to_json()).expect("re-parses"),
            config
        );
    }

    #[test]
    fn fs_config_requires_an_absolute_root() {
        // Empty and relative roots are both rejected; the message names the field.
        for bad in [json!({ "root": "" }), json!({ "root": "relative/path" })] {
            let error = DriveConfig::parse(DriveKind::Fs, &bad).expect_err("rejected");
            assert!(matches!(error, ConfigError::Invalid { .. }));
        }
    }

    #[test]
    fn valid_s3_config_parses_with_optional_fields_defaulted() {
        // Only the bucket is required; the rest default.
        let value = json!({ "bucket": "elysium-prod" });
        let config = DriveConfig::parse(DriveKind::S3, &value).expect("valid S3 config");
        assert_eq!(
            config,
            DriveConfig::S3(S3Config {
                bucket: "elysium-prod".to_string(),
                ..S3Config::default()
            })
        );
    }

    #[test]
    fn s3_config_requires_a_bucket() {
        let error = DriveConfig::parse(DriveKind::S3, &json!({ "region": "us-east-1" }))
            .expect_err("no bucket");
        assert!(matches!(error, ConfigError::Malformed { .. }));
    }

    #[test]
    fn valid_sftp_config_parses_and_defaults_the_port() {
        let value = json!({ "host": "files.example.com", "user": "elysium", "base_path": "/data" });
        let config = DriveConfig::parse(DriveKind::Sftp, &value).expect("valid SFTP config");
        assert_eq!(
            config,
            DriveConfig::Sftp(SftpConfig {
                host: "files.example.com".to_string(),
                port: DEFAULT_SFTP_PORT,
                user: "elysium".to_string(),
                base_path: "/data".to_string(),
            })
        );
    }

    #[test]
    fn sftp_config_rejects_missing_fields_and_a_zero_port() {
        // A missing required field fails the shape decode.
        let missing = json!({ "host": "files.example.com" });
        assert!(matches!(
            DriveConfig::parse(DriveKind::Sftp, &missing),
            Err(ConfigError::Malformed { .. })
        ));

        // A present-but-empty field fails validation, and port 0 is out of range.
        let empty_host = json!({ "host": "", "user": "elysium", "base_path": "/data" });
        assert!(matches!(
            DriveConfig::parse(DriveKind::Sftp, &empty_host),
            Err(ConfigError::Invalid { .. })
        ));
        let zero_port = json!({ "host": "h", "user": "u", "base_path": "/data", "port": 0 });
        assert!(matches!(
            DriveConfig::parse(DriveKind::Sftp, &zero_port),
            Err(ConfigError::Invalid { .. })
        ));
    }

    // --- Sealed Drive credential --------------------------------------------

    #[test]
    fn drive_secret_seal_then_open_round_trips() {
        let crypto = test_crypto();
        let project_id = Uuid::new_v4();
        let secret = b"s3-secret-access-key";

        let sealed = seal_secret(&crypto, project_id, secret);
        // The sealed blob is not the plaintext, and it opens back to the original.
        assert_ne!(sealed.as_slice(), secret.as_slice());
        let opened = open_secret(&crypto, project_id, &sealed).expect("opens");
        assert_eq!(opened, secret);
    }

    #[test]
    fn drive_secret_bound_to_a_different_project_fails_to_open() {
        let crypto = test_crypto();
        let sealed = seal_secret(&crypto, Uuid::new_v4(), b"secret");
        // A credential sealed for one project cannot be opened as another's.
        assert!(open_secret(&crypto, Uuid::new_v4(), &sealed).is_err());
    }

    #[test]
    fn unknown_discriminant_reports_the_enum_and_value() {
        let error = DriveKind::try_from(7).expect_err("out of range");
        let rendered = error.to_string();
        assert!(rendered.contains("DriveKind"));
        assert!(rendered.contains('7'));
    }
}
