// Copyright © 2026 Jalapeno Labs

//! Resolves when the process is asked to stop.

use tracing::{Level, event};

/// Completes on `SIGINT` (Ctrl-C) or, on Unix, `SIGTERM` (what Docker sends first).
///
/// # Panics
/// Panics if the runtime refuses to install a signal handler, which only happens
/// when the process has no signal support at all.
pub async fn signal() {
    let interrupt = async {
        tokio::signal::ctrl_c()
            .await
            .expect("SIGINT handler can be installed");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        signal(SignalKind::terminate())
            .expect("SIGTERM handler can be installed")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => {
            event!(name: "shutdown.signal.received", Level::INFO, signal = "SIGINT", "shutdown signal received, draining");
        }
        () = terminate => {
            event!(name: "shutdown.signal.received", Level::INFO, signal = "SIGTERM", "shutdown signal received, draining");
        }
    }
}
