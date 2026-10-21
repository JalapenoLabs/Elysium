// Copyright © 2026 Jalapeno Labs

//! Studio: single assets made by agents. See `docs/studio.md`.
//!
//! A Studio item runs on one session (`crate::sessions`). This module holds what Studio adds
//! to it: what its agents are told ([`instructions`]), which image an item's tile shows
//! ([`thumbnail`]), and how an agent's deliverables reach the item's storage location after
//! every turn ([`artifacts`]).

pub mod artifacts;
pub mod instructions;
pub mod thumbnail;
