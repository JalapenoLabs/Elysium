# CI

`.github/workflows/ci.yml` is stamped by `anubis new` and runs on pushes to `main` and on
every pull request. Three jobs, independent, in parallel.

- **Rust**: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` against a Postgres service container, then three drift checks.
- **Frontend**: `yarn install --immutable`, then typecheck, lint, test, and build.
- **E2E**: Playwright against the real stack, Postgres and the backend and Vite, driven by
  Chromium.

Nothing here is allowed to be advisory. A warning that does not fail the build is a warning
nobody reads.

## The drift checks

Three files are generated and compared, so a change that was never regenerated fails here
rather than in review:

| Generated file | From | Why it matters |
|---|---|---|
| `frontend/src/roles.generated.ts` | `config/roles.yml` | A stale permissions module means the UI offers controls the backend rejects |
| `frontend/src/plans.generated.ts` | `config/billing.yml` | A stale plan catalog means the billing screen sells something that does not exist |
| `frontend/src/api/v1.generated.ts` | the OpenAPI document the backend exports | A stale client means the SPA calls a shape the API no longer serves |

The client check runs the application binary's own `openapi` export rather than the
framework's, because the application merges its models into the document and a version freezes
with the application rather than with the framework release it was generated against.

Never hand-edit a generated file.

## The framework CLI in CI

The drift checks need the `anubis` binary, which is not published to crates.io yet
([Anubis #81](https://github.com/JalapenoLabs/Anubis/issues/81)), so CI installs it from git.

**The revision is read out of `backend/Cargo.toml` rather than repeated in the workflow.**
Two places holding the same revision is two places to forget, and the failure is quiet: CI
would generate with one framework and compile against another, and the drift check would
report a difference nobody introduced. Bumping the pin moves one file.

## Version pinning

Every toolchain version is pinned and the pins must stay in sync across the places they
appear:

| Tool | Version | Pinned in |
|---|---|---|
| Rust | 1.96.0 | `rust-toolchain.toml`, and the `dtolnay/rust-toolchain` tag in the workflow |
| Node | 24.13.0 | the workflow's `setup-node` step |
| Yarn | 4.17.0 | `packageManager` in `package.json`, and an explicit `corepack prepare` in the workflow |
| Postgres | 17.6 | `compose.yaml` and the workflow's service containers |
| Anubis | an exact git revision | `backend/Cargo.toml` and `frontend/package.json`, read from the first by the workflow |

No floating tags anywhere, including `latest` variants of docker images. The Anubis pin is
two files that must carry the same revision; see [anubis.md](anubis.md).

## Monitoring

After every push, watch the resulting run in the background and keep working. Only block on a
run when the push is the end of the task. When working through several commits in a row, fold
a failure into the next wave rather than stopping for each one.

## Earthly

Earthfiles remain welcome tooling, but `api.earthly.dev` is offline: the hosted Earthly
service is sunsetted. Never rely on Earthly-hosted secrets or authentication. Connection
errors to `api.earthly.dev` at the start of a run are benign.
