// Copyright © 2026 Jalapeno Labs

//! `elysium-engine`: does all of Elysium's background work, beside the API. See
//! `docs/engine.md`, and `elysium_api::run_engine`.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    elysium_api::run_engine().await
}
