// Copyright © 2026 Jalapeno Labs

//! Command line interface. With no subcommand the binary serves HTTP.

use clap::{Parser, Subcommand};

use crate::database::migrations::MigrationCommand;

/// Elysium API server and maintenance commands.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Serve the HTTP API (the default).
    Serve,
    /// Apply, roll back, or inspect database migrations. Reads `DATABASE_URL`.
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Print a new random key for `ELYSIUM_ENCRYPTION_KEY`.
    GenerateEncryptionKey,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum MigrateAction {
    /// Apply every pending migration.
    Run,
    /// Roll back the most recent migrations.
    Revert {
        /// How many migrations to roll back.
        #[arg(long, default_value_t = 1, conflicts_with = "all")]
        count: u32,
        /// Roll back every migration. Destroys all data in migrated tables.
        #[arg(long)]
        all: bool,
    },
    /// Roll back the latest migration and apply it again.
    Redo,
    /// List applied and pending migrations.
    Status,
}

impl From<MigrateAction> for MigrationCommand {
    fn from(action: MigrateAction) -> Self {
        match action {
            MigrateAction::Run => Self::Run,
            MigrateAction::Revert { all: true, .. } => Self::RevertAll,
            MigrateAction::Revert { count, all: false } => Self::Revert { count },
            MigrateAction::Redo => Self::Redo,
            MigrateAction::Status => Self::Status,
        }
    }
}
