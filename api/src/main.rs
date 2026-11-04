// Copyright © 2026 Jalapeno Labs

//! `elysium-api`: serves the web app, its `/api`, and MCP clients, behind nginx.
//!
//! One binary, three jobs: serve HTTP (the default), manage database migrations, and generate
//! encryption keys. See `elysium-api --help`, and `elysium_api::run_api`.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    elysium_api::run_api().await
}
