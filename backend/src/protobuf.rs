//! Axum protobuf wrapper for both response bodies and request bodies.
//!
//! Mirrors the ergonomics of [`axum::Json`], but for binary protobuf with a
//! `application/x-protobuf` content type. As a response, a handler returns
//! `Protobuf(message)`. As an extractor, a handler takes `Protobuf(message)` and
//! the body is decoded from the wire against the generated type. This is the
//! server half of the typed contract; the TypeScript client encodes and decodes
//! the same messages from their generated schemas.

use axum::extract::{FromRequest, Request};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

/// MIME type for binary protobuf bodies, matched by the frontend client.
const PROTOBUF_CONTENT_TYPE: &str = "application/x-protobuf";

/// Wraps a protobuf message so it can be returned from, or extracted into, a handler.
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

impl<State, Message> FromRequest<State> for Protobuf<Message>
where
    State: Send + Sync,
    Message: prost::Message + Default,
{
    type Rejection = Response;

    async fn from_request(request: Request, state: &State) -> Result<Self, Self::Rejection> {
        let bytes = axum::body::Bytes::from_request(request, state)
            .await
            .map_err(|error| error.into_response())?;
        let message = Message::decode(bytes)
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid protobuf body").into_response())?;
        Ok(Protobuf(message))
    }
}
