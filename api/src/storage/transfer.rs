// Copyright © 2026 Jalapeno Labs

//! Streaming file bodies to and from providers without holding them in memory.
//!
//! A file can take far longer to move than any fixed deadline allows, so transfers are not
//! bounded by `reqwest`'s whole-request timeout. They are bounded by progress instead: a
//! provider must start answering within [`RESPONSE_TIMEOUT`], and a body that moves no
//! bytes for [`IDLE_TIMEOUT`] is abandoned as stalled, in either direction.

use std::time::Duration;

use bytes::Bytes;
use futures_util::stream::{self, BoxStream};
use futures_util::{Stream, StreamExt};
use reqwest::header::CONTENT_LENGTH;
use reqwest::{Body, RequestBuilder, Response};
use tokio::sync::watch;

use super::StorageError;

/// How long a provider may take to answer once a request, or an upload's last byte, is
/// sent. Providers answer in well under a second; a minute covers a slow link.
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a body may go without moving a byte before the transfer counts as stalled.
/// Long enough for a congested link to recover, short enough that a dead connection does
/// not hold a task open.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// A file's bytes as they arrive from the provider.
pub type ByteStream = BoxStream<'static, Result<Bytes, StorageError>>;

/// Sends a request whose response body is streamed, waiting at most [`RESPONSE_TIMEOUT`]
/// for the response to begin.
///
/// # Errors
/// Returns [`StorageError::Refused`] when the provider cannot be reached or does not
/// answer in time.
pub async fn send_for_body(
    request: RequestBuilder,
    provider: &'static str,
) -> Result<Response, StorageError> {
    match tokio::time::timeout(RESPONSE_TIMEOUT, request.send()).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(send_failure(provider, error)),
        Err(_elapsed) => Err(StorageError::Refused(format!(
            "{provider} did not answer within {} seconds",
            RESPONSE_TIMEOUT.as_secs()
        ))),
    }
}

/// Streams a successful response's body, ending with an error if it stalls.
pub fn body_of(response: Response, provider: &'static str) -> ByteStream {
    let chunks = Box::pin(response.bytes_stream());
    // The state is `None` once the stream has failed, so nothing is read after an error.
    stream::unfold(Some(chunks), move |chunks| async move {
        let mut chunks = chunks?;
        match tokio::time::timeout(IDLE_TIMEOUT, chunks.next()).await {
            Ok(Some(Ok(chunk))) => Some((Ok(chunk), Some(chunks))),
            Ok(None) => None,
            Ok(Some(Err(error))) => Some((
                Err(StorageError::Refused(format!(
                    "{provider}: the download failed: {}",
                    error.without_url()
                ))),
                None,
            )),
            Err(_elapsed) => Some((
                Err(StorageError::Refused(format!(
                    "{provider}: the download stalled, sending nothing for {} seconds",
                    IDLE_TIMEOUT.as_secs()
                ))),
                None,
            )),
        }
    })
    .boxed()
}

/// An upload's body as it is handed to the provider, counted against its declared length.
struct CountedBody<Chunks> {
    chunks: std::pin::Pin<Box<Chunks>>,
    sent_bytes: u64,
    content_length: u64,
    /// Announces each chunk to [`send_upload`]'s stall watch. Dropping it, when the body
    /// ends or fails, tells the watch that only the response is left to wait for.
    progress: watch::Sender<u64>,
}

/// Sends `request` with `chunks` as its body, which must be exactly `content_length` bytes.
///
/// The length is sent as `Content-Length`, so the body goes out unchunked: S3 refuses
/// chunked uploads, and a declared length lets every provider reject a truncated file.
///
/// # Errors
/// Returns [`StorageError::Invalid`] when the body is longer or shorter than
/// `content_length`, and [`StorageError::Refused`] when the body fails, the provider cannot
/// be reached, or the transfer stalls.
pub async fn send_upload<Chunks, ChunkError>(
    request: RequestBuilder,
    chunks: Chunks,
    content_length: u64,
    provider: &'static str,
) -> Result<Response, StorageError>
where
    Chunks: Stream<Item = Result<Bytes, ChunkError>> + Send + 'static,
    ChunkError: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (progress, mut watched) = watch::channel(0);
    let counted = CountedBody {
        chunks: Box::pin(chunks),
        sent_bytes: 0,
        content_length,
        progress,
    };
    let body = stream::unfold(Some(counted), |counted| async move {
        let mut counted = counted?;
        let Some(chunk) = counted.chunks.next().await else {
            if counted.sent_bytes < counted.content_length {
                let short = StorageError::Invalid(format!(
                    "the upload ended after {} of its {} bytes",
                    counted.sent_bytes, counted.content_length
                ));
                return Some((Err(short), None));
            }
            return None;
        };
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let failed =
                    StorageError::Refused(format!("the upload's body failed: {}", error.into()));
                return Some((Err(failed), None));
            }
        };

        counted.sent_bytes += chunk.len() as u64;
        if counted.sent_bytes > counted.content_length {
            let long = StorageError::Invalid(format!(
                "the upload is longer than its declared {} bytes",
                counted.content_length
            ));
            return Some((Err(long), None));
        }
        counted.progress.send_replace(counted.sent_bytes);
        Some((Ok(chunk), Some(counted)))
    });

    let response = request
        .header(CONTENT_LENGTH, content_length)
        .body(Body::wrap_stream(body))
        .send();
    tokio::pin!(response);

    // While the body is still being handed over, only a pause in its progress counts as a
    // stall; a large file on a slow link may take hours and still be healthy.
    loop {
        tokio::select! {
            sent = &mut response => {
                return sent.map_err(|error| send_failure(provider, error));
            }
            progressed = tokio::time::timeout(IDLE_TIMEOUT, watched.changed()) => match progressed {
                Ok(Ok(())) => {}
                Ok(Err(_body_finished)) => break,
                Err(_elapsed) => {
                    return Err(StorageError::Refused(format!(
                        "{provider}: the upload stalled, moving nothing for {} seconds",
                        IDLE_TIMEOUT.as_secs()
                    )));
                }
            },
        }
    }

    match tokio::time::timeout(RESPONSE_TIMEOUT, response).await {
        Ok(sent) => sent.map_err(|error| send_failure(provider, error)),
        Err(_elapsed) => Err(StorageError::Refused(format!(
            "{provider} did not confirm the upload within {} seconds",
            RESPONSE_TIMEOUT.as_secs()
        ))),
    }
}

/// Explains a request that failed before the provider answered.
///
/// When the failure started in an upload's own body, such as a length mismatch, that
/// [`StorageError`] travels inside `reqwest`'s error chain and is returned as it was
/// raised. Otherwise the error is described without its URL, which for presigned S3
/// requests carries the signature, followed by its root cause: `reqwest`'s own message
/// alone, such as "error sending request", does not say what went wrong.
pub fn send_failure(provider: &'static str, error: reqwest::Error) -> StorageError {
    let mut raised = None;
    let mut root_cause = None;
    let causes = std::iter::successors(std::error::Error::source(&error), |cause| cause.source());
    for cause in causes {
        if let Some(storage_error) = cause.downcast_ref::<StorageError>() {
            raised = Some(storage_error.clone());
            break;
        }
        root_cause = Some(cause.to_string());
    }
    if let Some(raised) = raised {
        return raised;
    }

    let error = error.without_url();
    match root_cause {
        Some(root_cause) => StorageError::Refused(format!("{provider}: {error}: {root_cause}")),
        None => StorageError::Refused(format!("{provider}: {error}")),
    }
}
