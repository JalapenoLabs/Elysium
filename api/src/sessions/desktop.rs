// Copyright © 2026 Jalapeno Labs

//! Every session turn's desktop: a screen of its own that the agent drives with mouse, keyboard,
//! and screenshots, for what no web page or API covers. See `docs/coding.md`, The desktop.
//!
//! The desktop is the `desktop` MCP server, `elysium-desktop-mcp` in the workspace image, a stdio
//! launcher Claude Code starts with the turn. It brings up a virtual display, a window manager,
//! Firefox, and a view-only VNC server on the session's [`socket`], then runs computer-use-mcp
//! against the display. The runner stops the turn's process group when the turn ends, which takes
//! the desktop with it.
//!
//! A session's turns run one after another, so the session's socket path names one desktop at a
//! time, and the live view finds it there: [`relay`] carries the VNC protocol between a viewer's
//! WebSocket and the socket, byte for byte. The VNC server is view-only, so whatever a viewer
//! sends cannot move the agent's mouse or type into its screen.

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt as _, StreamExt as _};
use secrecy::SecretString;
use tokio::io::AsyncWriteExt as _;
use tracing::{Level, event};

use crate::runner::socket::SocketConnection;
use crate::runner::{EnvironmentEntry, McpServer, WorkspacePath};

/// The turn's MCP server name, so its tools read as `mcp__desktop__computer`.
pub const SERVER_NAME: &str = "desktop";

/// The launcher the workspace image installs (`workspace/desktop/`).
const COMMAND: &str = "/opt/elysium/bin/elysium-desktop-mcp";

/// Where session `session_id`'s desktop serves its view, inside the workspace.
///
/// # Panics
/// Never: the path is built from a number and fixed components.
pub fn socket(session_id: i64) -> WorkspacePath {
    WorkspacePath::new(format!(".elysium/desktops/session-{session_id}.sock"))
        .expect("a fixed relative path")
}

/// Session `session_id`'s desktop, for its turn's MCP servers.
pub fn server(session_id: i64) -> McpServer {
    McpServer::Stdio {
        name: SERVER_NAME.to_owned(),
        command: COMMAND.to_owned(),
        arguments: Vec::new(),
        environment: vec![EnvironmentEntry {
            key: "ELYSIUM_DESKTOP_SOCKET".to_owned(),
            value: SecretString::from(socket(session_id).absolute()),
        }],
    }
}

/// Relays a desktop's VNC stream between `viewer` and `connection` until either closes or
/// `until` completes. Binary messages are VNC bytes in both directions; anything else a viewer
/// sends is ignored.
pub async fn relay(
    viewer: WebSocket,
    connection: SocketConnection,
    until: impl Future<Output = ()> + Send,
) {
    let (mut to_viewer, mut from_viewer) = viewer.split();
    let SocketConnection {
        mut output,
        mut input,
    } = connection;

    let to_viewer_side = async {
        while let Some(chunk) = output.next().await {
            match chunk {
                Ok(bytes) => {
                    if to_viewer.send(Message::Binary(bytes)).await.is_err() {
                        return;
                    }
                }
                Err(error) => {
                    event!(
                        name: "sessions.desktop.stream_failure",
                        Level::WARN,
                        error.message = %error,
                        "a desktop's stream broke",
                    );
                    break;
                }
            }
        }
        // The desktop went away, as it does when its turn ends.
        let _closed = to_viewer.send(Message::Close(None)).await;
    };
    let to_desktop_side = async {
        while let Some(message) = from_viewer.next().await {
            match message {
                Ok(Message::Binary(bytes)) => {
                    if input.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_)) => {}
            }
        }
        let _closed = input.shutdown().await;
    };

    tokio::select! {
        () = to_viewer_side => {}
        () = to_desktop_side => {}
        () = until => {}
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn each_session_has_its_own_desktop_socket() {
        assert_eq!(
            socket(12).absolute(),
            "/workspace/.elysium/desktops/session-12.sock"
        );
        let McpServer::Stdio {
            name, environment, ..
        } = server(12)
        else {
            panic!("the desktop is a stdio server");
        };
        assert_eq!(name, "desktop");
        let [entry] = environment.as_slice() else {
            panic!("one variable");
        };
        assert_eq!(entry.key, "ELYSIUM_DESKTOP_SOCKET");
        assert_eq!(
            entry.value.expose_secret(),
            "/workspace/.elysium/desktops/session-12.sock"
        );
    }
}
