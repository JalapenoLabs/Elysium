// Copyright © 2026 Jalapeno Labs

//! Byte streams to Unix sockets inside the workspace, for Elysium's own code: today, the live
//! view of a turn's desktop, whose VNC server listens on a socket only the workspace can reach.
//!
//! The API never mounts the workspace volume and nothing in the workspace listens on the
//! network, so a connection is a `docker exec` of `socat` as the agent account, bridging the
//! exec's standard input and output to the socket. Its output is the socket's bytes in order;
//! closing the input ends the connection.

use std::pin::Pin;

use bollard::container::LogOutput;
use bollard::exec::StartExecResults;
use bollard::models::ExecConfig;
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use tokio::io::AsyncWrite;

use super::WorkspacePath;
use super::workspace::{
    AGENT_USER, CONTAINER_NAME, SCRIPT_TIMEOUT, WorkspaceContainer, WorkspaceError,
};

/// `$1` is the socket. Exits 0 when something listens there.
const PROBE_SCRIPT: &str = r#"[ -S "$1" ]"#;

/// A connection to a socket in the workspace.
pub struct SocketConnection {
    /// What the socket sends, in order. Ends when the socket closes.
    pub output: BoxStream<'static, Result<Bytes, WorkspaceError>>,
    /// What to send the socket. Shutting it down closes the connection.
    pub input: Pin<Box<dyn AsyncWrite + Send>>,
}

impl std::fmt::Debug for SocketConnection {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SocketConnection")
            .finish_non_exhaustive()
    }
}

impl WorkspaceContainer {
    /// Whether a socket is listening at `path`.
    ///
    /// # Errors
    /// Propagates Docker's refusal to run the probe.
    pub async fn has_socket(&self, path: &WorkspacePath) -> Result<bool, WorkspaceError> {
        let absolute = path.absolute();
        let probe = self
            .run_script(
                AGENT_USER,
                PROBE_SCRIPT,
                &[&absolute],
                Vec::new(),
                SCRIPT_TIMEOUT,
            )
            .await?;
        Ok(probe.exit_code == Some(0))
    }

    /// Connects to the socket at `path`, as the agent account.
    ///
    /// # Errors
    /// Propagates Docker's refusal to start the exec.
    pub async fn connect_socket(
        &self,
        path: &WorkspacePath,
    ) -> Result<SocketConnection, WorkspaceError> {
        let exec = self
            .docker()
            .create_exec(
                CONTAINER_NAME,
                ExecConfig {
                    cmd: Some(vec![
                        "socat".to_owned(),
                        "-".to_owned(),
                        format!("UNIX-CONNECT:{}", path.absolute()),
                    ]),
                    user: Some(AGENT_USER.to_owned()),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await?;
        let StartExecResults::Attached { output, input } =
            self.docker().start_exec(&exec.id, None).await?
        else {
            return Err(WorkspaceError::Detached);
        };

        // Only standard output carries the socket's bytes; socat's complaints go to standard
        // error and are not part of the stream.
        let output = output
            .filter_map(|chunk| async move {
                match chunk {
                    Ok(LogOutput::StdOut { message }) => Some(Ok(message)),
                    Ok(
                        LogOutput::StdErr { .. }
                        | LogOutput::StdIn { .. }
                        | LogOutput::Console { .. },
                    ) => None,
                    Err(error) => Some(Err(WorkspaceError::from(error))),
                }
            })
            .boxed();
        Ok(SocketConnection { output, input })
    }
}
