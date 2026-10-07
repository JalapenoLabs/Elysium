// Copyright © 2026 Jalapeno Labs

//! Studio: single assets made by agents. See `docs/studio.md`.
//!
//! A Studio item runs on coding sessions (`crate::routes::v1::coding_sessions`). This module
//! holds what Studio adds to them: what its agents are told ([`instructions`]), which image
//! an item's tile shows ([`thumbnail`]), how an agent's deliverables reach the item's storage
//! location ([`artifacts`]), and how an item carries on in a new thread ([`continuation`]), and how a thread's harness
//! session is kept for that ([`transcripts`]).

pub mod artifacts;
pub mod continuation;
pub mod instructions;
pub mod thumbnail;
pub mod transcripts;
