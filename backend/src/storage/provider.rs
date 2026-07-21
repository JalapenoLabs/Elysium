//! Turns a validated Drive config plus its opened secret into a live operator.
//!
//! A Drive is one [OpenDAL](https://opendal.apache.org/) operator rooted where the
//! org pointed it (see `docs/storage.md`). This module is the single place that maps
//! a [`DriveConfig`] and its decrypted provider credential onto an
//! [`opendal::Operator`], advertises what each provider can do, and probes a
//! connection before a Drive is saved. The HTTP surface (#21) calls [`operator`] to
//! serve file operations and [`test_connection`] to back `POST /drives/{id}/test`;
//! consumers (git, registry, Astraval) gate role bindings on
//! [`DriveKind::capabilities`].
//!
//! # Credential encoding
//!
//! [`DriveConfig`] holds only non-secret settings; the provider credential travels
//! separately as the sealed `drives.secret` blob (see [`super::seal_secret`]). Once
//! opened, that blob is the UTF-8 JSON of a per-kind struct:
//!
//! - **FS** has no credential; the secret is `None`.
//! - **S3** is [`S3Secret`]: `{"access_key_id": "...", "secret_access_key": "..."}`.
//! - **SFTP** is [`SftpSecret`]: `{"kind": "private_key", "pem": "..."}` or
//!   `{"kind": "password", "password": "..."}`.
//!
//! # Provider notes
//!
//! The S3 transport rides `rustls` (opendal's `reqwest-rustls-tls`), matching the
//! sqlx/reqwest stack so no OpenSSL enters the build. OpenDAL's SFTP backend
//! authenticates through `openssh` from a private-key *file*, so [`operator`]
//! materializes an [`SftpSecret::PrivateKey`] to a `0600` temp file whose lifetime the
//! returned [`DriveOperator`] owns.
//!
//! # SFTP password authentication is blocked (see `docs/storage.md`)
//!
//! Password auth for SFTP is **not implementable cleanly on the current stack**, so an
//! [`SftpSecret::Password`] is rejected with a clear error rather than hacked in:
//!
//! - OpenDAL 0.57's `services-sftp` runs on the `openssh` crate, which drives the host
//!   `ssh` client and authenticates only from a private-key file or an agent. Its
//!   builder ([`opendal::services::Sftp`]) exposes `endpoint`/`user`/`root`/`key` but
//!   **no `password` method** (verified against the crate), so there is no supported
//!   way to pass a password through the operator.
//! - No later OpenDAL minor adds it, because the limitation is the `openssh` transport,
//!   not OpenDAL's surface.
//! - The only alternatives both break a load-bearing invariant: a libssh2-based crate
//!   (`ssh2`) links system OpenSSL, violating the no-OpenSSL rule; and bolting a
//!   `russh`/`russh-sftp` (pure-Rust, rustls) client alongside OpenDAL would fork the
//!   "every Drive is one OpenDAL operator" model into a parallel transport to maintain.
//!
//! **What would unblock it:** OpenDAL adding SFTP password auth upstream (an `openssh`
//! capability or a new SFTP backend), or Elysium adopting a `russh`-backed
//! implementation of OpenDAL's `Access` trait so password Drives stay one `Operator`
//! without OpenSSL. Until then, key-based auth is the only SFTP credential in v1.

use std::ops::Deref;

use eyre::{eyre, Context};
use opendal::{services, Operator};
use serde::{Deserialize, Serialize};
use tempfile::TempPath;

use super::{DriveConfig, DriveKind, FsConfig, S3Config, SftpConfig};

/// A live Drive operator plus any on-disk material the provider depends on.
///
/// For FS and S3 this is just the [`Operator`]. SFTP additionally needs its private
/// key on disk for the `openssh` transport, so the materialized keyfile is held here
/// and deleted when the `DriveOperator` drops. Callers hold a `DriveOperator` for as
/// long as they use the operator and reach the [`Operator`] through [`Deref`] or
/// [`DriveOperator::operator`].
#[derive(Debug)]
pub struct DriveOperator {
    operator: Operator,
    /// Keeps a materialized SFTP keyfile alive; `None` for FS and S3.
    _keyfile: Option<TempPath>,
}

impl DriveOperator {
    /// Borrows the underlying OpenDAL operator.
    pub fn operator(&self) -> &Operator {
        &self.operator
    }
}

impl Deref for DriveOperator {
    type Target = Operator;

    fn deref(&self) -> &Self::Target {
        &self.operator
    }
}

/// The decrypted S3 credential, encoded as JSON inside the sealed `drives.secret`.
///
/// Sealed and opened with [`super::seal_secret`]/[`super::open_secret`]; never
/// returned to a client. Its [`Debug`] redacts the secret access key.
#[derive(Clone, Serialize, Deserialize)]
pub struct S3Secret {
    /// The S3 access key id (an identifier, like a user name).
    pub access_key_id: String,
    /// The S3 secret access key (the sensitive half of the credential).
    pub secret_access_key: String,
}

impl S3Secret {
    /// Encodes this credential as the JSON bytes stored (sealed) in `drives.secret`.
    ///
    /// # Panics
    /// Never in practice: a two-string struct always serializes to JSON.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("S3Secret always serializes to JSON")
    }

    /// Decodes an opened `drives.secret` blob into an S3 credential.
    ///
    /// # Errors
    /// Returns an error when the bytes are not the expected JSON object.
    pub fn from_bytes(bytes: &[u8]) -> eyre::Result<Self> {
        serde_json::from_slice(bytes).wrap_err("malformed S3 Drive credential")
    }
}

// Redacts the secret access key so it never reaches a log or panic message; the
// access key id stays visible to aid debugging (see `s3_secret_debug_redacts_key`).
impl std::fmt::Debug for S3Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Secret")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .finish()
    }
}

/// The decrypted SFTP credential, encoded as JSON inside the sealed `drives.secret`.
///
/// Sealed and opened with [`super::seal_secret`]/[`super::open_secret`]; never
/// returned to a client. Its [`Debug`] redacts the key and password material.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SftpSecret {
    /// A PEM-encoded private key, materialized to a keyfile for `openssh`.
    PrivateKey {
        /// The PEM text of the private key.
        pem: String,
    },
    /// A password. Rejected in v1: OpenDAL's `openssh` SFTP backend has no password
    /// path (see the module docs for why this is blocked and what would unblock it).
    Password {
        /// The account password.
        password: String,
    },
}

impl SftpSecret {
    /// Encodes this credential as the JSON bytes stored (sealed) in `drives.secret`.
    ///
    /// # Panics
    /// Never in practice: these string variants always serialize to JSON.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("SftpSecret always serializes to JSON")
    }

    /// Decodes an opened `drives.secret` blob into an SFTP credential.
    ///
    /// # Errors
    /// Returns an error when the bytes are not the expected tagged JSON object.
    pub fn from_bytes(bytes: &[u8]) -> eyre::Result<Self> {
        serde_json::from_slice(bytes).wrap_err("malformed SFTP Drive credential")
    }
}

// Redacts the key and password so neither reaches a log or panic message (see
// `sftp_secret_debug_redacts_material`).
impl std::fmt::Debug for SftpSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrivateKey { .. } => f.write_str("SftpSecret::PrivateKey(<redacted>)"),
            Self::Password { .. } => f.write_str("SftpSecret::Password(<redacted>)"),
        }
    }
}

/// Builds a live OpenDAL operator for a Drive from its config and opened secret.
///
/// `secret` is the decrypted `drives.secret` blob (see the module docs for its JSON
/// shape): `None` for FS, and required for S3 and SFTP. The returned [`DriveOperator`]
/// owns any keyfile the provider needs, so keep it alive for as long as you use the
/// operator. Building is offline; providers connect lazily on the first operation.
///
/// # Errors
/// Returns an error when a required credential is missing or malformed, when an SFTP
/// keyfile cannot be written, when an SFTP password is supplied (unsupported by the
/// backend), or when OpenDAL rejects the assembled configuration.
pub fn operator(config: &DriveConfig, secret: Option<&[u8]>) -> eyre::Result<DriveOperator> {
    let kind = config.kind();
    match config {
        DriveConfig::Fs(fs) => fs_operator(fs),
        DriveConfig::S3(s3) => s3_operator(s3, secret),
        DriveConfig::Sftp(sftp) => sftp_operator(sftp, secret),
    }
    .inspect_err(|error| {
        tracing::warn!(drive_kind = ?kind, error = %error, "failed to build Drive operator");
    })
}

/// Builds an FS operator rooted at [`FsConfig::root`], ignoring any secret.
fn fs_operator(config: &FsConfig) -> eyre::Result<DriveOperator> {
    let builder = services::Fs::default().root(&config.root);
    let operator = Operator::new(builder)
        .wrap_err("invalid FS Drive configuration")?
        .finish();
    Ok(DriveOperator {
        operator,
        _keyfile: None,
    })
}

/// Builds an S3 operator from the bucket settings and access/secret key material.
///
/// The optional `endpoint`, `region`, and `prefix` are applied only when set, so an
/// empty value falls back to the provider default (prefix maps to the operator root).
/// [`S3Config::sse`] requests SSE-S3 (AES256 with S3-managed keys).
fn s3_operator(config: &S3Config, secret: Option<&[u8]>) -> eyre::Result<DriveOperator> {
    let credential = S3Secret::from_bytes(require_secret(DriveKind::S3, secret)?)?;

    let mut builder = services::S3::default()
        .bucket(&config.bucket)
        .access_key_id(&credential.access_key_id)
        .secret_access_key(&credential.secret_access_key);
    if !config.endpoint.is_empty() {
        builder = builder.endpoint(&config.endpoint);
    }
    if !config.region.is_empty() {
        builder = builder.region(&config.region);
    }
    if !config.prefix.is_empty() {
        builder = builder.root(&config.prefix);
    }
    if config.sse {
        builder = builder.server_side_encryption_with_s3_key();
    }

    let operator = Operator::new(builder)
        .wrap_err("invalid S3 Drive configuration")?
        .finish();
    Ok(DriveOperator {
        operator,
        _keyfile: None,
    })
}

/// Builds an SFTP operator, materializing a private key to a keyfile for `openssh`.
///
/// The backend authenticates from a key *file*, so an [`SftpSecret::PrivateKey`] is
/// written to a `0600` temp file held by the returned [`DriveOperator`]. Password
/// auth is unsupported and rejected.
fn sftp_operator(config: &SftpConfig, secret: Option<&[u8]>) -> eyre::Result<DriveOperator> {
    let credential = SftpSecret::from_bytes(require_secret(DriveKind::Sftp, secret)?)?;
    let pem = match credential {
        SftpSecret::PrivateKey { pem } => pem,
        SftpSecret::Password { .. } => {
            return Err(eyre!(
                "SFTP password authentication is not supported; supply a private key"
            ));
        }
    };

    let keyfile = write_keyfile(pem.as_bytes())?;
    let key_path = keyfile
        .to_str()
        .ok_or_else(|| eyre!("SFTP keyfile path is not valid UTF-8"))?;

    // OpenDAL takes host and port together as the `openssh` endpoint.
    let endpoint = format!("{}:{}", config.host, config.port);
    let builder = services::Sftp::default()
        .endpoint(&endpoint)
        .user(&config.user)
        .root(&config.base_path)
        .key(key_path);

    let operator = Operator::new(builder)
        .wrap_err("invalid SFTP Drive configuration")?
        .finish();
    Ok(DriveOperator {
        operator,
        _keyfile: Some(keyfile),
    })
}

/// Writes private-key bytes to a fresh temp file and returns its owning path.
///
/// The file is created with the platform's private temp-file permissions (`0600` on
/// Unix) and is removed when the returned [`TempPath`] drops.
fn write_keyfile(pem: &[u8]) -> eyre::Result<TempPath> {
    use std::io::Write;

    let mut file = tempfile::NamedTempFile::new().wrap_err("failed to create SFTP keyfile")?;
    file.write_all(pem)
        .wrap_err("failed to write SFTP keyfile")?;
    file.flush().wrap_err("failed to flush SFTP keyfile")?;
    Ok(file.into_temp_path())
}

/// Returns the secret bytes a credentialed provider requires, or a clear error.
fn require_secret(kind: DriveKind, secret: Option<&[u8]>) -> eyre::Result<&[u8]> {
    secret.ok_or_else(|| eyre!("{kind:?} Drive requires a provider credential"))
}

/// Probes a Drive by building its operator and listing one entry at the root.
///
/// Backs `POST /drives/{id}/test`: it validates that the configuration and credential
/// reach a live provider before a Drive is saved. A single-entry list is the cheapest
/// cross-provider probe that touches real state: it issues one bounded S3
/// `ListObjects` and forces the lazy SFTP connection.
///
/// FS gets an explicit directory check first, because OpenDAL's FS backend is lenient:
/// it lists a missing root as empty and creates directories on write, so the list
/// probe alone would pass a misconfigured root. The root is a local path we own, so a
/// `metadata` call is the honest reachability test.
///
/// # Errors
/// Returns an error when [`operator`] fails, when an FS root is missing or is not a
/// directory, or when the provider cannot list its root (unreachable host, bad
/// credential, denied access).
pub async fn test_connection(config: &DriveConfig, secret: Option<&[u8]>) -> eyre::Result<()> {
    let kind = config.kind();

    if let DriveConfig::Fs(fs) = config {
        let metadata = tokio::fs::metadata(&fs.root)
            .await
            .wrap_err_with(|| format!("could not reach the {kind:?} Drive root {}", fs.root))?;
        if !metadata.is_dir() {
            return Err(eyre!(
                "the {kind:?} Drive root {} is not a directory",
                fs.root
            ));
        }
    }

    let drive = operator(config, secret)?;
    drive
        .list_with("/")
        .limit(1)
        .await
        .map(|_| ())
        .wrap_err_with(|| format!("could not reach the {kind:?} Drive"))
        .inspect_err(|error| {
            tracing::warn!(drive_kind = ?kind, error = %error, "Drive connection test failed");
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A syntactically plausible private key for construction tests; never used to
    /// connect, so its contents do not need to be a real key.
    const SAMPLE_PEM: &str =
        "-----BEGIN OPENSSH PRIVATE KEY-----\nzzzSECRETKEYMATERIALzzz\n-----END OPENSSH PRIVATE KEY-----\n";

    fn fs_config(root: &str) -> DriveConfig {
        DriveConfig::Fs(FsConfig {
            root: root.to_string(),
        })
    }

    // --- FS operator round trip ---------------------------------------------

    #[tokio::test]
    async fn fs_operator_round_trips_a_file_byte_for_byte() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_str().expect("utf-8 temp path");
        let drive = operator(&fs_config(root), None).expect("build FS operator");

        let path = "notes/hello.txt";
        let body = b"byte-for-byte payload".to_vec();

        // write -> read is byte-for-byte.
        drive.write(path, body.clone()).await.expect("write");
        let read = drive.read(path).await.expect("read").to_vec();
        assert_eq!(read, body);

        // list surfaces the written file under its folder.
        let entries = drive.list("notes/").await.expect("list");
        assert!(entries.iter().any(|entry| entry.name() == "hello.txt"));

        // delete removes it.
        drive.delete(path).await.expect("delete");
        assert!(!drive.exists(path).await.expect("exists check"));
    }

    // --- Connection test ----------------------------------------------------

    #[tokio::test]
    async fn test_connection_ok_for_reachable_fs_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_str().expect("utf-8 temp path");
        test_connection(&fs_config(root), None)
            .await
            .expect("existing root is reachable");
    }

    #[tokio::test]
    async fn test_connection_errs_for_missing_fs_root() {
        // A child of a real tempdir that we never create: absent on disk, and the
        // probe returns before it could be created, so nothing pollutes the tree.
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("absent-root");
        let config = fs_config(missing.to_str().expect("utf-8 temp path"));

        let error = test_connection(&config, None)
            .await
            .expect_err("missing root fails");
        // The error names the Drive kind so an operator can act on it.
        assert!(error.to_string().contains("Fs"));
        assert!(!missing.exists(), "the probe must not create the root");
    }

    // --- Construction from representative S3/SFTP config ---------------------

    #[test]
    fn s3_operator_builds_from_representative_config() {
        let config = DriveConfig::S3(S3Config {
            endpoint: "https://s3.example.com".to_string(),
            region: "us-east-1".to_string(),
            bucket: "elysium-prod".to_string(),
            prefix: "team/".to_string(),
            sse: true,
        });
        let secret = S3Secret {
            access_key_id: "AKIAEXAMPLE".to_string(),
            secret_access_key: "example-secret".to_string(),
        }
        .to_bytes();

        operator(&config, Some(&secret)).expect("build S3 operator");
    }

    #[test]
    fn s3_operator_requires_a_credential() {
        let config = DriveConfig::S3(S3Config {
            bucket: "elysium-prod".to_string(),
            ..S3Config::default()
        });
        let error = operator(&config, None).expect_err("missing credential");
        assert!(error.to_string().contains("requires a provider credential"));
    }

    #[test]
    fn sftp_operator_builds_and_materializes_a_keyfile() {
        let config = DriveConfig::Sftp(SftpConfig {
            host: "files.example.com".to_string(),
            port: 22,
            user: "elysium".to_string(),
            base_path: "/data".to_string(),
        });
        let secret = SftpSecret::PrivateKey {
            pem: SAMPLE_PEM.to_string(),
        }
        .to_bytes();

        let drive = operator(&config, Some(&secret)).expect("build SFTP operator");
        // The keyfile exists while the guard lives, then is removed on drop.
        let key_path = drive
            ._keyfile
            .as_ref()
            .expect("SFTP keeps a keyfile")
            .to_path_buf();
        assert!(key_path.exists());
        drop(drive);
        assert!(!key_path.exists());
    }

    #[test]
    fn sftp_password_credential_is_rejected() {
        let config = DriveConfig::Sftp(SftpConfig {
            host: "files.example.com".to_string(),
            port: 22,
            user: "elysium".to_string(),
            base_path: "/data".to_string(),
        });
        let secret = SftpSecret::Password {
            password: "hunter2".to_string(),
        }
        .to_bytes();

        let error = operator(&config, Some(&secret)).expect_err("password unsupported");
        assert!(error.to_string().to_lowercase().contains("password"));
    }

    // --- Credential encoding + redaction ------------------------------------

    #[test]
    fn s3_secret_round_trips_through_json_bytes() {
        let secret = S3Secret {
            access_key_id: "AKIAEXAMPLE".to_string(),
            secret_access_key: "example-secret".to_string(),
        };
        let decoded = S3Secret::from_bytes(&secret.to_bytes()).expect("round trip");
        assert_eq!(decoded.access_key_id, secret.access_key_id);
        assert_eq!(decoded.secret_access_key, secret.secret_access_key);
    }

    #[test]
    fn sftp_secret_round_trips_through_tagged_json() {
        let secret = SftpSecret::PrivateKey {
            pem: SAMPLE_PEM.to_string(),
        };
        // The tagged JSON is the documented on-blob shape.
        assert_eq!(
            secret.to_bytes(),
            serde_json::to_vec(&serde_json::json!({ "kind": "private_key", "pem": SAMPLE_PEM }))
                .unwrap()
        );
        match SftpSecret::from_bytes(&secret.to_bytes()).expect("round trip") {
            SftpSecret::PrivateKey { pem } => assert_eq!(pem, SAMPLE_PEM),
            SftpSecret::Password { .. } => panic!("decoded the wrong variant"),
        }
    }

    #[test]
    fn s3_secret_debug_redacts_key() {
        let secret = S3Secret {
            access_key_id: "AKIAEXAMPLE".to_string(),
            secret_access_key: "example-secret".to_string(),
        };
        let rendered = format!("{secret:?}");
        assert!(rendered.contains("AKIAEXAMPLE"));
        assert!(!rendered.contains("example-secret"));
    }

    #[test]
    fn sftp_secret_debug_redacts_material() {
        let key = SftpSecret::PrivateKey {
            pem: SAMPLE_PEM.to_string(),
        };
        let password = SftpSecret::Password {
            password: "hunter2".to_string(),
        };
        assert!(!format!("{key:?}").contains("SECRETKEYMATERIAL"));
        assert!(!format!("{password:?}").contains("hunter2"));
    }
}
