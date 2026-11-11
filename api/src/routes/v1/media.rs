// Copyright © 2026 Jalapeno Labs

//! Streaming a recording or a clip from its storage location to a player, the part it asks for
//! at a time, so a two hour recording seeks without being downloaded whole.
//!
//! Each route that plays media checks who may read it and finds where it is kept, then answers
//! through [`stream`]: `206 Partial Content` for a request with one `Range`, `416` for one that
//! covers none of the file, and the file whole otherwise, each saying `Accept-Ranges: bytes` so
//! the browser knows it can seek. No provider URL or credential reaches the browser.

use anyhow::Context;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::errors::ApiError;
use crate::models::storage_location::StorageLocation;
use crate::state::AppState;
use crate::storage::range::ByteRange;
use crate::storage::{Download, RangedDownload};

/// A recording and a clip are each written once, at a path that names them, and never again, so
/// what a browser holds of one is right for as long as it keeps it, privately.
///
/// Write-once is also why [`stream`] can ignore `If-Range`: the parts a browser put together
/// always come from the same bytes. A file that could change under its address would need a
/// validator (an `ETag`) and `If-Range` honored before its ranges could be served.
const CACHE_CONTROL: &str = "private, max-age=31536000, immutable";

/// What a browser is told the file is. Only audio and video are named as themselves: anything
/// else a source called its recording is sent as bytes, which no page renders.
fn media_type(content_type: &str) -> HeaderValue {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let is_media = essence.starts_with("audio/") || essence.starts_with("video/");
    is_media
        .then(|| HeaderValue::from_str(&essence).ok())
        .flatten()
        .unwrap_or(HeaderValue::from_static("application/octet-stream"))
}

/// Answers the file at `path` in `location`, which is `content_type`, as the request's headers
/// ask: the part one `Range` names, or the whole file.
///
/// # Errors
/// [`ApiError::NotFound`] when nothing is stored there, and [`ApiError::BadGateway`] when the
/// provider refuses or cannot be reached.
pub async fn stream(
    state: &AppState,
    location: &StorageLocation,
    path: &str,
    content_type: &str,
    request: &HeaderMap,
) -> Result<Response, ApiError> {
    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;
    let range = request
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(ByteRange::parse);

    let (status, download, content_range) = match range {
        None => {
            let download = state.storage.download(location, &access_key, path).await?;
            (StatusCode::OK, download, None)
        }
        Some(range) => {
            match state
                .storage
                .download_range(location, &access_key, path, range)
                .await?
            {
                RangedDownload::Part {
                    span,
                    total,
                    download,
                } => {
                    let total = total.map_or_else(|| "*".to_owned(), |total| total.to_string());
                    let content_range = format!("bytes {}-{}/{total}", span.start, span.end);
                    (StatusCode::PARTIAL_CONTENT, download, Some(content_range))
                }
                RangedDownload::Whole(download) => (StatusCode::OK, download, None),
                RangedDownload::Unsatisfiable { total } => return Ok(unsatisfiable(total)),
            }
        }
    };
    Ok(answer(status, download, content_range, content_type))
}

/// The bytes a provider sent, with the headers a player reads.
fn answer(
    status: StatusCode,
    download: Download,
    content_range: Option<String>,
    content_type: &str,
) -> Response {
    let mut response = (status, Body::from_stream(download.body)).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, media_type(content_type));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_CONTROL),
    );
    // nginx would otherwise read a recording ahead from storage, as fast as storage sends it,
    // for every seek a player makes; unbuffered, the API reads only as fast as the browser plays.
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    if let Some(length) = download.content_length {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    }
    if let Some(content_range) = content_range
        && let Ok(value) = HeaderValue::from_str(&content_range)
    {
        headers.insert(header::CONTENT_RANGE, value);
    }
    response
}

/// `416 Range Not Satisfiable`, saying how long the file is when the provider said.
fn unsatisfiable(total: Option<u64>) -> Response {
    let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
    let headers = response.headers_mut();
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some(total) = total
        && let Ok(value) = HeaderValue::from_str(&format!("bytes */{total}"))
    {
        headers.insert(header::CONTENT_RANGE, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_audio_and_video_are_named_as_themselves() {
        assert_eq!(media_type("video/mp4"), "video/mp4");
        assert_eq!(media_type("Audio/MPEG; charset=binary"), "audio/mpeg");
        for other in ["text/html", "image/svg+xml", "", "application/javascript"] {
            assert_eq!(media_type(other), "application/octet-stream", "{other:?}");
        }
    }
}
