// Copyright © 2026 Jalapeno Labs

//! Build identity reported by `/api/version`.
//!
//! All values are baked in by `build.rs`, so the struct is constructed once at
//! startup and shared read-only for the life of the process.

use anyhow::{Context, Result};
use serde::Serialize;

use crate::git_info::GitInfo;

/// What is running: crate version, toolchain, build time, and git history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub profile: &'static str,
    pub rustc: &'static str,
    pub built_at: &'static str,
    pub git: GitInfo,
}

impl VersionInfo {
    /// Decodes the metadata that `build.rs` embedded into the binary.
    ///
    /// # Errors
    /// Fails only if the embedded git JSON does not match [`GitInfo`], which would
    /// mean `build.rs` and this module disagree on the schema.
    pub fn from_build() -> Result<Self> {
        let git = serde_json::from_str(env!("ELYSIUM_GIT_INFO"))
            .context("embedded git metadata does not match GitInfo")?;

        Ok(Self {
            name: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
            profile: env!("ELYSIUM_BUILD_PROFILE"),
            rustc: env!("ELYSIUM_RUSTC_VERSION"),
            built_at: env!("ELYSIUM_BUILT_AT"),
            git,
        })
    }
}
