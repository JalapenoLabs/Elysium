//! Compiles the shared protobuf contract into Rust.
//!
//! The single source of truth lives in the repository-root `proto/` tree (see
//! `~/.claude/docs/proto.md`). This script runs `prost` at build time so the
//! generated types always match the `.proto` files, and it re-runs whenever any
//! of them change. The output lands in `OUT_DIR` and is wired into the crate by
//! `src/proto.rs`.

use std::io::Result;

/// Proto files to compile, relative to the backend crate root.
const PROTO_FILES: &[&str] = &[
    "../proto/common.proto",
    "../proto/user.proto",
    "../proto/health.proto",
    "../proto/auth.proto",
    "../proto/realtime.proto",
];

/// Import root passed to `protoc` so `import "common.proto";` resolves.
const PROTO_INCLUDE_ROOT: &str = "../proto";

fn main() -> Result<()> {
    prost_build::compile_protos(PROTO_FILES, &[PROTO_INCLUDE_ROOT])?;

    for proto_file in PROTO_FILES {
        println!("cargo:rerun-if-changed={proto_file}");
    }

    Ok(())
}
