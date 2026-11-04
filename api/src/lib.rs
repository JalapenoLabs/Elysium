// Copyright © 2026 Jalapeno Labs

//! Elysium's server: one crate, two processes, which share Postgres and the event bus.
//!
//! - **`elysium-api`** (`src/main.rs`) serves clients alone: the web app, its `/api`, and MCP
//!   clients. It also manages migrations and generates encryption keys. See [`run_api`].
//! - **`elysium-engine`** (`src/bin/elysium-engine.rs`) does all background work: every loop,
//!   schedule, and agent turn. See [`run_engine`] and `docs/engine.md`.
//!
//! Either process can crash and restart without the other. What one asks of the other travels
//! as rows in Postgres, which the other reads, and events on the bus (`crate::realtime`), which
//! every process hears. Both binaries are thin: everything they run is in this library, so
//! each module is compiled once and checked for dead code across both.

mod action_items;
mod activity;
mod agent_models;
mod auth;
mod backups;
mod browser;
mod claude_accounts;
mod cli;
mod company_doc;
mod config;
mod confluence;
mod connections;
mod connectors;
mod crypto;
mod database;
mod drafts;
mod elysia;
mod embeddings;
mod engine;
mod environment;
mod errors;
mod git_info;
mod github;
mod images;
mod issue_reports;
mod jira;
mod knowledge;
mod mail;
mod mcp;
mod meetings;
mod middleware;
mod models;
mod oauth;
mod passwords;
mod realtime;
mod routes;
mod runner;
mod search;
mod server;
mod services;
mod sessions;
mod shutdown;
mod state;
mod storage;
mod studio;
#[cfg(test)]
mod test_support;
mod tools;
mod updates;
mod version;
mod web_app;
mod whisper;
mod workflows;
mod youtrack;

use anyhow::Result;
use clap::Parser;
use mimalloc::MiMalloc;
use tracing::Level;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::prelude::*;

use crate::cli::{ApiCli, Command, EngineCli};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// Runs `elysium-api`: serves HTTP (the default), manages migrations, or generates a key.
///
/// # Errors
/// Whatever the chosen command fails with: invalid configuration, unreachable stores, a
/// listener that cannot bind, or a migration that does not apply.
pub async fn run_api() -> Result<()> {
    // A missing .env is the normal state inside a container; compose injects the environment directly.
    let _ = dotenvy::dotenv();

    let cli = ApiCli::parse();
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

/// Runs `elysium-engine` until a shutdown signal arrives.
///
/// # Errors
/// Fails on invalid configuration, an invalid encryption key, pending migrations, or
/// unreachable stores.
pub async fn run_engine() -> Result<()> {
    let _ = dotenvy::dotenv();

    EngineCli::parse();
    init_tracing();

    engine::run().await
}

/// Structured logs to stdout. `RUST_LOG` filters; `LOG_FORMAT=json` switches to
/// one JSON object per line for log shippers.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info"));
    let json = std::env::var("LOG_FORMAT").is_ok_and(|format| format.eq_ignore_ascii_case("json"));

    let format = tracing_subscriber::fmt::layer().with_target(false);
    let format = if json {
        format.json().boxed()
    } else {
        format.compact().boxed()
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(filter_fn(|metadata| {
            within_rmcp_ceiling(metadata.target(), *metadata.level())
        }))
        .with(format)
        .init();
}

/// Whether an event passes the ceiling on rmcp's own logging, which no `RUST_LOG` can lift.
///
/// rmcp logs every MCP request in full at debug and its messages at trace, arguments included,
/// and a tool's arguments can carry a secret (a storage location's file, a token). Every rmcp target,
/// its modules included, is therefore held at info whatever `RUST_LOG` names; everything else
/// is left to `RUST_LOG`.
fn within_rmcp_ceiling(target: &str, level: Level) -> bool {
    let is_rmcp = target == "rmcp" || target.starts_with("rmcp::");
    !is_rmcp || level <= Level::INFO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rmcp_logs_nothing_past_info_from_any_of_its_modules() {
        assert!(!within_rmcp_ceiling("rmcp", Level::DEBUG));
        assert!(!within_rmcp_ceiling("rmcp::service", Level::DEBUG));
        assert!(!within_rmcp_ceiling(
            "rmcp::transport::streamable_http_server::tower",
            Level::TRACE
        ));
        assert!(within_rmcp_ceiling("rmcp::service", Level::INFO));
        assert!(within_rmcp_ceiling("rmcp::service", Level::WARN));
    }

    #[test]
    fn naming_an_rmcp_module_at_debug_still_logs_none_of_its_requests() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct Captured(Arc<Mutex<Vec<u8>>>);
        impl Write for Captured {
            #[expect(
                clippy::renamed_function_params,
                reason = "`buf` is a shorthand name; the project spells names out"
            )]
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("lock").extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("info,rmcp::service=debug"))
            .with(filter_fn(|metadata| {
                within_rmcp_ceiling(metadata.target(), *metadata.level())
            }))
            .with(tracing_subscriber::fmt::layer().with_writer(move || writer.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "rmcp::service", "received request: secret=hunter2-bearer");
            tracing::info!(target: "rmcp::service", "Service initialized as server");
        });

        let output = String::from_utf8(captured.0.lock().expect("lock").clone()).expect("utf-8");
        assert!(!output.contains("hunter2-bearer"), "{output}");
        assert!(
            output.contains("Service initialized"),
            "info still logs: {output}"
        );
    }

    #[test]
    fn every_other_target_is_left_to_rust_log() {
        assert!(within_rmcp_ceiling("elysium_api::mcp", Level::TRACE));
        assert!(
            within_rmcp_ceiling("rmcpx", Level::DEBUG),
            "only rmcp's own targets are held"
        );
    }
}
