// Copyright © 2026 Jalapeno Labs

//! Elysium API: an axum server fronted by nginx, backed by Postgres and Redis.
//!
//! One binary, three jobs: serve HTTP (the default), manage database migrations,
//! and generate encryption keys. See `elysium-api --help`.

mod cli;
mod config;
mod connections;
mod crypto;
mod database;
mod errors;
mod git_info;
mod middleware;
mod models;
mod routes;
mod server;
mod shutdown;
mod state;
#[cfg(test)]
mod test_support;
mod version;

use anyhow::Result;
use clap::Parser;
use mimalloc::MiMalloc;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<()> {
    // A missing .env is the normal state inside a container; compose injects the environment directly.
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();
    init_tracing();

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => server::serve().await,
        Command::Migrate { action } => {
            let database_url = config::required_secret("DATABASE_URL")?;
            database::migrations::execute(database_url, action.into()).await
        }
        Command::GenerateEncryptionKey => {
            println!("{}", crypto::generate_key());
            Ok(())
        }
    }
}

/// Structured logs to stdout. `RUST_LOG` filters; `LOG_FORMAT=json` switches to
/// one JSON object per line for log shippers.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,tower_governor=warn"));
    let json = std::env::var("LOG_FORMAT").is_ok_and(|format| format.eq_ignore_ascii_case("json"));

    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    if json {
        builder.json().init();
    } else {
        builder.compact().init();
    }
}
