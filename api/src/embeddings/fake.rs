// Copyright © 2026 Jalapeno Labs

//! A local fake of the embedding service, for tests of the client and of the embedder.
//!
//! It answers text-embeddings-inference's two routes Elysium calls: `GET /info`, naming the
//! model it serves, and `POST /embed`, giving each input a vector of its own made from the
//! input's bytes, so equal texts embed equally and different ones differently. Every input is
//! recorded as it arrived, prefix included, so a test asserts on what left the process. A test
//! can make it serve another model, answer vectors of the wrong size, or refuse every request.

use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use url::Url;

use super::{DIMENSIONS, Embeddings, MODEL};

#[derive(Debug)]
struct FakeState {
    serving: String,
    dimensions: usize,
    refusing: bool,
    received: Vec<String>,
}

/// The fake, running on a free local port until the test's runtime ends.
#[derive(Debug, Clone)]
pub(crate) struct FakeEmbeddings {
    state: Arc<Mutex<FakeState>>,
    pub(crate) embeddings: Embeddings,
}

#[derive(Deserialize)]
struct EmbedBody {
    inputs: Vec<String>,
}

impl FakeEmbeddings {
    /// Starts the fake, serving [`MODEL`] at its size.
    pub(crate) async fn start() -> Self {
        let state = Arc::new(Mutex::new(FakeState {
            serving: MODEL.to_owned(),
            dimensions: DIMENSIONS,
            refusing: false,
            received: Vec::new(),
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let base_url = Url::parse(&format!(
            "http://{}/",
            listener.local_addr().expect("address")
        ))
        .expect("a local URL parses");
        let router = Router::new()
            .route("/info", get(info))
            .route("/embed", post(embed))
            .with_state(Arc::clone(&state));
        tokio::spawn(async move { axum::serve(listener, router).await });
        Self {
            state,
            embeddings: Embeddings::new(reqwest::Client::new(), base_url),
        }
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state
            .lock()
            .expect("the fake's state is never poisoned")
    }

    /// Serves `model` from now on, as a service built from another image would.
    pub(crate) fn serve(&self, model: &str) {
        model.clone_into(&mut self.lock().serving);
    }

    /// Answers vectors of `dimensions` from now on.
    pub(crate) fn answer_dimensions(&self, dimensions: usize) {
        self.lock().dimensions = dimensions;
    }

    /// Refuses every request from now on, as an overloaded service does.
    pub(crate) fn refuse(&self, refusing: bool) {
        self.lock().refusing = refusing;
    }

    /// Every text `/embed` received, in order, as it arrived.
    pub(crate) fn received(&self) -> Vec<String> {
        self.lock().received.clone()
    }

    /// The vector the fake gives `input`: its bytes spread over the dimensions, so the same
    /// text always gives the same vector.
    pub(crate) fn vector_for(input: &str, dimensions: usize) -> Vec<f32> {
        let mut vector = vec![0.0_f32; dimensions];
        for (index, byte) in input.bytes().enumerate() {
            vector[index % dimensions] += f32::from(byte) / 255.0;
        }
        vector
    }
}

async fn info(State(state): State<Arc<Mutex<FakeState>>>) -> Response {
    let state = state.lock().expect("the fake's state is never poisoned");
    if state.refusing {
        return refusal();
    }
    Json(json!({ "model_id": "/opt/embeddings/model", "served_model_name": state.serving }))
        .into_response()
}

async fn embed(
    State(state): State<Arc<Mutex<FakeState>>>,
    Json(body): Json<EmbedBody>,
) -> Response {
    let mut state = state.lock().expect("the fake's state is never poisoned");
    if state.refusing {
        return refusal();
    }
    state.received.extend(body.inputs.iter().cloned());
    let vectors: Vec<Vec<f32>> = body
        .inputs
        .iter()
        .map(|input| FakeEmbeddings::vector_for(input, state.dimensions))
        .collect();
    Json(vectors).into_response()
}

/// What text-embeddings-inference answers when its queue is full.
fn refusal() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({ "error": "Model is overloaded", "error_type": "overloaded" })),
    )
        .into_response()
}
