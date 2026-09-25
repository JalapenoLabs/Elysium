// Copyright © 2026 Jalapeno Labs

//! Blender on every satellite, so coding agents can 3D model whenever a task calls for it.
//!
//! Two halves, both Arsox features rather than anything baked into the satellite image:
//!
//! - **The setup script** installs Blender, Blender Lab's MCP server, and its bridge extension.
//!   The fleet's satellite watcher hands it to every satellite whose reported script differs,
//!   and the satellite runs it as root at every container start and whenever it changes.
//! - **Two thread services** run per turn, on ports the satellite assigns: the bridge (headless
//!   Blender with the extension listening) and the MCP server that talks to it. The thread's
//!   `blender` MCP server targets the second, so every thread models in a Blender of its own and
//!   no two threads ever share a scene.
//!
//! See `docs/coding.md`.

use std::sync::LazyLock;

use arsox_sdk::proto::settings::v1::{McpServer, Service, ServiceEndpoint};
use sha2::{Digest as _, Sha256};

/// The setup script as the satellite receives it.
///
/// `setup.sh` ends inside the heredoc that writes the MCP server's hash-locked requirements, so the
/// lock can live as its own file, regenerated with `uv` exactly as its header says, instead of being
/// pasted into a shell script. The literal closes that heredoc and runs the script.
pub const SETUP_SCRIPT: &str = concat!(
    include_str!("setup.sh"),
    include_str!("requirements.txt"),
    "REQUIREMENTS\n}\n\nmain\n",
);

/// The script's SHA-256 in lowercase hex, which is how a satellite reports the script it holds.
/// A satellite reporting anything else is handed [`SETUP_SCRIPT`].
pub static SETUP_SCRIPT_SHA256: LazyLock<String> =
    LazyLock::new(|| hex::encode(Sha256::digest(SETUP_SCRIPT.as_bytes())));

/// The service names double as the `ARSOX_SERVICE_<NAME>_*` variables the satellite sets, which
/// is how the MCP server finds its thread's bridge.
const BRIDGE_SERVICE: &str = "blender-bridge";
const MCP_SERVICE: &str = "blender-mcp";

/// The MCP server's name as the agent sees it: its tools arrive as `mcp__blender__<tool>`.
const MCP_SERVER_NAME: &str = "blender";

/// The thread's Blender services, in start order: the bridge first, because the MCP server is
/// handed its port. Both are plain TCP listeners (the bridge speaks its own protocol, and the MCP
/// server answers only POST), so readiness is the satellite's default TCP probe.
pub fn services() -> Vec<Service> {
    vec![
        Service {
            name: BRIDGE_SERVICE.to_owned(),
            command: "exec blender --background --online-mode --command blender_mcp \
                      --host 127.0.0.1 --port \"$PORT\""
                .to_owned(),
            ..Service::default()
        },
        Service {
            name: MCP_SERVICE.to_owned(),
            command: "BLENDER_PATH=/opt/blender/blender BLENDER_MCP_HOST=127.0.0.1 \
                      BLENDER_MCP_PORT=\"$ARSOX_SERVICE_BLENDER_BRIDGE_PORT\" \
                      exec blender-mcp --transport http --host 127.0.0.1 --port \"$PORT\""
                .to_owned(),
            ..Service::default()
        },
    ]
}

/// The thread's `blender` MCP server, pointed at its own MCP service. The server answers at `/`.
pub fn mcp_server() -> McpServer {
    McpServer {
        name: MCP_SERVER_NAME.to_owned(),
        service: Some(ServiceEndpoint {
            service: MCP_SERVICE.to_owned(),
            path: "/".to_owned(),
        }),
        ..McpServer::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setup_script_closes_its_requirements_heredoc_and_runs() {
        assert!(SETUP_SCRIPT.starts_with("#!/bin/sh\n"));
        assert!(SETUP_SCRIPT.contains("cat > \"$1\" <<'REQUIREMENTS'\n"));
        assert!(SETUP_SCRIPT.contains("--hash=sha256:"));
        assert!(SETUP_SCRIPT.ends_with("\nREQUIREMENTS\n}\n\nmain\n"));
        assert_eq!(SETUP_SCRIPT_SHA256.len(), 64);
    }

    #[test]
    fn the_mcp_server_targets_the_mcp_service_which_is_handed_the_bridge_port() {
        let services = services();
        let names: Vec<&str> = services
            .iter()
            .map(|service| service.name.as_str())
            .collect();
        assert_eq!(names, [BRIDGE_SERVICE, MCP_SERVICE]);
        assert!(
            services[1]
                .command
                .contains("$ARSOX_SERVICE_BLENDER_BRIDGE_PORT")
        );

        let server = mcp_server();
        assert!(server.url.is_empty());
        let endpoint = server.service.expect("the server targets a service");
        assert_eq!(endpoint.service, MCP_SERVICE);
        assert_eq!(endpoint.path, "/");
    }
}
