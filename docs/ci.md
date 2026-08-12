# CI

CI runs on GitHub Actions using Jalapeno Labs self-hosted runners, targeted with the labels
`[self-hosted, rocky9, earthly, docker]`. The runners persist the cargo registry, the target
directory, and the yarn cache between runs, so the workflows carry no cache save or restore steps.

## Workflow

`.github/workflows/ci.yml` runs on pushes and pull requests to `main` and `develop`:

- **Rust job**: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` against a Postgres service container. It also regenerates
  `frontend/src/roles.generated.ts` from `config/roles.yml` and fails on drift, because a stale
  permissions module means the UI offers controls the backend rejects.
- **Frontend job**: `yarn install --immutable`, then typecheck, lint, test, and build.

The two jobs are independent and run in parallel. Nothing here is allowed to be advisory: a warning
that does not fail the build is a warning nobody reads.

## Version pinning

Every toolchain version is pinned and the pins must stay in sync across the places they appear:

| Tool | Version | Pinned in |
|---|---|---|
| Rust | 1.96.0 | `rust-toolchain.toml`, and the `dtolnay/rust-toolchain` tag in the workflow |
| Node | 24.13.0 | the workflow's `setup-node` step |
| Yarn | 4.17.0 | `packageManager` in `package.json`, and an explicit `corepack prepare` in the workflow |
| Postgres | 17.6 | `compose.yaml` and the workflow's service container |
| Anubis | an exact git revision | `backend/Cargo.toml` and `frontend/package.json` |

No floating tags, anywhere, including `latest` variants of docker images. The Anubis pin is two
files that must carry the same revision; see [anubis.md](anubis.md).

## The framework CLI in CI

The roles drift check needs the `anubis` binary, which is not published to crates.io yet (Anubis
#56), so CI installs it from git at the same revision the backend depends on. When the crate
publishes this becomes a version, and the step gets faster.

## Earthly

The runners carry the `earthly` label and Earthfiles remain welcome tooling, but `api.earthly.dev`
is offline: the hosted Earthly service is sunsetted. Never rely on Earthly-hosted secrets or
authentication. Connection errors to `api.earthly.dev` at the start of a run are benign.
