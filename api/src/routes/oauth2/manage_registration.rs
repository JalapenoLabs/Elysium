// Copyright © 2026 Jalapeno Labs

//! `GET`, `PUT`, and `DELETE /oauth2/register/{client_id}`: a client reads, updates, or deletes
//! its own registration (RFC 7592) with its registration access token, through Hydra, which
//! checks the token.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method};
use axum::response::Response;
use bytes::Bytes;

use crate::errors::ApiError;
use crate::oauth::registration::Registration;

pub async fn handle(
    State(registration): State<Registration>,
    method: Method,
    path: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let Path(client_id) = path?;
    Ok(registration
        .relay(method, Some(&client_id), &headers, body)
        .await?)
}
