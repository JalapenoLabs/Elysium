// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/studio-items/{id}/turns`: send a prompt, with or without a drawing.

/// The largest request body: a drawing and its clean view, each at most
/// [`MAX_IMAGE_BYTES`], plus the text fields.
pub const MAX_BODY_BYTES: usize = 2 * MAX_IMAGE_BYTES + 1024 * 1024;

/// The largest image a drawn prompt may carry. A 4K PNG capture of a viewer runs to a few
/// megabytes; this leaves room without letting one request carry an arbitrary blob.
pub const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;

pub async fn handle() {}
