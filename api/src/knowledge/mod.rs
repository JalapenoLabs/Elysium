// Copyright © 2026 Jalapeno Labs

//! The knowledge base: everything the company said, decided, and learned, indexed for search.
//!
//! `docs/knowledge.md` is the design. Every source (a timeline record, a learning, and a section
//! of the company doc) becomes one document of ordered passages through
//! [`crate::models::knowledge_document::upsert`], inside the transaction that changes the
//! source, so it is searchable the moment that commits. This module holds the pure parts:
//!
//! - [`passages`]: how a source's text is cut into the passages the index searches and the
//!   embedding model embeds;
//! - [`records`]: the document each timeline record becomes;
//! - [`search`]: the one search of the index, by words and by meaning, ranked together;
//!
//! and the loops it runs in the engine:
//!
//! - [`embedder`]: gives every passage its embedding as it arrives, through the embedding model
//!   (`crate::embeddings`), and again when the model changes;
//! - [`backfill`]: indexes the records stored before records were indexed as they are stored,
//!   and every record again when how a record is indexed changes.

pub mod backfill;
pub mod embedder;
pub mod passages;
pub mod records;
pub mod search;
