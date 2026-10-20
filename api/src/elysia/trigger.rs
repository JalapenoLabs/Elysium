// Copyright © 2026 Jalapeno Labs

//! What started a turn, as `elysia_turns.trigger_kind` stores it.
//!
//! Kinds are text in the database so later sources add kinds without a migration; these are
//! the ones this version queues. A person's kinds ([`is_from_person`]) start whatever her hours
//! say; the rest wait for them.

/// A person wrote to her.
pub const MESSAGE: &str = "message";

/// A person answered her question.
pub const ANSWER: &str = "answer";

/// Changes in the workspace or a provider, batched by the intake.
pub const EVENT: &str = "event";

/// Nothing was queued, so she revisits her own work.
pub const IDLE: &str = "idle";

/// Whether a turn of `kind` was started by a person.
pub fn is_from_person(kind: &str) -> bool {
    kind == MESSAGE || kind == ANSWER
}
