# Transport

Elysium's wire contract is **protobuf, everywhere**. Every service and client speaks the
same generated types, so a timestamp, a coordinate, or a user is defined exactly once.

## Source of truth: `proto/`

A single root `proto/` tree is the only place messages are defined. Files are flat at the
root, one per bounded domain, each with its own versioned package:

| File | Package | Contents |
|------|---------|----------|
| `common.proto` | `elysium.common.v1` | Shared vocabulary: `Timestamp`, `Duration`, `GeoPoint`, `Coordinate`, `Money`, `Color`, `Uuid`, `PageRequest`/`PageResponse` |
| `user.proto` | `elysium.user.v1` | Identity: `User`, `Org`, `Team`, `TeamMembership`, `UserStatus`, `TeamRole` |
| `health.proto` | `elysium.health.v1` | Liveness, greeting, and database-reachability responses |

The full standards (nanosecond UTC-canonical timestamps, WGS84 coordinates, naming, field
numbering) live in `~/.claude/docs/proto.md`. New domains add a new `<domain>.proto` here.

## Codegen

Generation is deterministic and happens at build time, never at runtime.

- **Rust (backend)**: `prost` via `backend/build.rs`, compiling `../proto/*.proto` into
  `OUT_DIR` on every `cargo build`. `backend/src/proto.rs` re-exports the generated modules
  as `proto::common::v1`, `proto::user::v1`, `proto::health::v1`. `protoc` is a build
  prerequisite (installed in the backend image; present on the host for local builds).
- **TypeScript (frontend)**: `buf generate` with `protoc-gen-es`, driven by
  `proto/buf.gen.yaml`, emitting `frontend/src/proto/generated/*_pb.ts`. Run with
  `yarn generate:proto` from `frontend/`. The generated output is committed and CI fails if
  it drifts from the `.proto` files.

`buf lint` and `buf format` guard the contract. The flat layout intentionally relaxes the
`PACKAGE_DIRECTORY_MATCH` and `DIRECTORY_SAME_PACKAGE` lint rules (see `proto/buf.yaml`);
every other STANDARD rule stays on.

## HTTP encoding

Two kinds of endpoint, deliberately encoded differently:

- **Infrastructure endpoints** are plain, the shapes load balancers, uptime monitors, and
  `curl` expect: `GET /` returns `200` with the text `pong`, and `GET /health` returns `200`
  with a small JSON health body. These are intentionally not part of the protobuf contract.
- **Versioned API endpoints** (`/api/v1/*`) are sent as **binary protobuf** with
  `Content-Type: application/x-protobuf`. On the backend, handlers return the reusable
  `Protobuf<Message>` responder (`backend/src/protobuf.rs`), which encodes any `prost::Message`.
  On the frontend, the `getProto(path, schema)` helper (`frontend/src/api/index.ts`) fetches
  the raw bytes with Ky and decodes them against the generated schema via `@bufbuild/protobuf`'s
  `fromBinary`.

## Native helpers

Generated messages are plain data. Ergonomic wrappers live alongside the generated code and
provide idiomatic conversions. Today: `frontend/src/proto/helpers/timestamp.ts` wraps
`Timestamp` with `fromDate`/`toDate`/`fromMoment`/`toMoment`/`format` over `moment-timezone`,
honoring the UTC-canonical standard. New shared shapes (`GeoPoint`, `Money`, ...) get the
same treatment as they are used.

## Roadmap

- Request bodies over protobuf (currently only responses are exercised).
- Shared Rust `proto` crate once workers are introduced, so backend and workers depend on one
  generated crate rather than each regenerating.
- Evaluate the Connect protocol for streaming once real-time surfaces (chat, presence) land.
