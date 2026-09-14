// Copyright © 2026 Jalapeno Labs

//! Git metadata captured at compile time and served by `/api/version`.
//!
//! This module is shared between `build.rs` (which fills it in from the repository)
//! and the binary (which embeds the serialized result), so it must depend on
//! nothing but `serde`.

use serde::{Deserialize, Serialize};

/// One commit from the repository history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    /// Committer date in strict ISO 8601, straight from `git log --format=%cI`.
    pub date: String,
    pub subject: String,
}

/// Snapshot of the repository at build time.
///
/// Every field is empty when the build ran outside a git checkout or before the
/// first commit; the build never fails for lack of git metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub branch: String,
    pub commit: Option<GitCommit>,
    /// Most recent commits first, `HEAD` included.
    pub history: Vec<GitCommit>,
}
