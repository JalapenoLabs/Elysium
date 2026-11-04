// Copyright © 2026 Jalapeno Labs

//! The knowledge base: everything the company said, decided, and learned, indexed for search.
//!
//! `docs/knowledge.md` is the design. Every source (a learning today, timeline records and the
//! company doc as they are indexed) becomes one document of ordered passages through
//! [`crate::models::knowledge_document::upsert`], inside the transaction that changes the
//! source, so it is searchable the moment that commits. This module holds the pure parts:
//!
//! - [`passages`]: how a source's text is cut into the passages the index searches and the
//!   embedding model embeds.

pub mod passages;
