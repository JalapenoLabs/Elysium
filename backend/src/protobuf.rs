//! Axum response wrapper that serializes protobuf messages to the wire.
//!
//! Mirrors the ergonomics of [`axum::Json`], but encodes any [`prost::Message`]
//! as binary protobuf with a `application/x-protobuf` content type. This is the
//! server half of the typed contract: handlers return `Protobuf(message)` and
//! the TypeScript client decodes the same message from its generated schema.

use axum::http::header;
use axum::response::{IntoResponse, Response};

/// MIME type for binary protobuf bodies, matched by the frontend client.
const PROTOBUF_CONTENT_TYPE: &str = "application/x-protobuf";

/// Wraps a protobuf message so it can be returned directly from a handler.
#[derive(Debug, Clone, Copy)]
pub struct Protobuf<Message>(pub Message);

impl<Message> IntoResponse for Protobuf<Message>
where
    Message: prost::Message,
{
    fn into_response(self) -> Response {
        let mut buffer = Vec::with_capacity(self.0.encoded_len());

        // Encoding into a `Vec` only fails when the buffer lacks capacity, which
        // cannot happen for a growable `Vec`; a failure here is a prost bug.
        self.0
            .encode(&mut buffer)
            .expect("encoding a protobuf message into a Vec is infallible");

        ([(header::CONTENT_TYPE, PROTOBUF_CONTENT_TYPE)], buffer).into_response()
    }
}
