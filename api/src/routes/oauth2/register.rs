// Copyright © 2026 Jalapeno Labs

//! `POST /oauth2/register`: an MCP client registers itself (RFC 7591), through Hydra. Answers
//! Hydra's status and body, without the empty fields Hydra fills that body with.

use axum::extract::State;
use axum::http::{HeaderMap, Method};
use axum::response::Response;
use bytes::Bytes;

use crate::errors::ApiError;
use crate::oauth::registration::Registration;

pub async fn handle(
    State(registration): State<Registration>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    Ok(registration
        .relay(Method::POST, None, &headers, body)
        .await?)
}
