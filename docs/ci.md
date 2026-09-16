# CI

GitHub Actions on the org's self-hosted runners. Every check runs on pull requests to `main`, in the merge queue, on
pushes to `main`, and on demand through `workflow_dispatch`. `.github/workflows/pull-review.yml` is the automated
review, described in `docs/infrastructure.md`, and is not part of this.

## Runners

Every job targets:

```yaml
runs-on: [ self-hosted, linux, rocky9, docker ]
```

Labels match as a set. `chipotle`, `chipotle2`, and `chipotle3` carry all four, so up to three jobs run at once. The
`flagship` label is left out on purpose: only `chipotle` carries it, and a runner takes one job at a time, so requiring
it would queue every job behind the one before.

**Nothing is cached with `actions/cache`.** The runners are persistent, so rustup toolchains, Cargo's registry, Yarn's
cache, and Docker's layer cache are already warm on the machine. Uploading and restoring them would be a cache of a
cache. The one thing a persistent runner does not keep on its own is Cargo's build output, because `actions/checkout`
runs `git clean -ffdx` and deletes a `target/` inside the checkout. The Rust jobs therefore set `CARGO_TARGET_DIR` to
`$RUNNER_WORKSPACE/cargo-target/<crate>`, beside the checkout, so each runner compiles only what changed. Each runner
has its own workspace, so no two jobs share a target directory at the same time. Deleting one is always safe.

Concurrent jobs on one Docker daemon never collide: image tags and container names carry the run id and attempt, and
every published port is picked by Docker (`--publish 127.0.0.1::<port>`) and read back with `docker port`.

## Required checks

Every workflow runs on every pull request, merge group, and push, with no path filters. GitHub never evaluates path
filters for `merge_group` events, and a required check whose workflow a path filter skipped on a pull request stays
pending forever. Running everything is the simple option that cannot stall the queue; warm runners keep an unchanged
lane short. The API image is the exception, since its release build recompiles the crate on every commit.

Job names are the check names. Require exactly these in the `main` ruleset:

| Check                | Workflow                 | Proves                                                          |
|----------------------|--------------------------|-----------------------------------------------------------------|
| `API format`         | `api.yml`                | `cargo fmt --check`                                             |
| `API clippy`         | `api.yml`                | `cargo clippy --all-targets --locked -- -D warnings`            |
| `API test`           | `api.yml`                | `cargo test --locked`, the hermetic suite                       |
| `API migrations`     | `api.yml`                | `api/scripts/verify-migrations.sh`                              |
| `API image`          | `api.yml`                | `api/Dockerfile` builds and its binary runs                     |
| `Broker format`      | `oauth-broker.yml`       | `cargo fmt --check`                                             |
| `Broker clippy`      | `oauth-broker.yml`       | `cargo clippy --all-targets --locked -- -D warnings`            |
| `Broker test`        | `oauth-broker.yml`       | `cargo test --locked`                                           |
| `Broker image`       | `oauth-broker.yml`       | `oauth-broker/Dockerfile` builds and the broker answers `/healthz` |
| `Frontend typecheck` | `frontend.yml`           | `yarn typecheck`                                                |
| `Frontend lint`      | `frontend.yml`           | `yarn lint`, the `@jalapenolabs/cli/eslint` ruleset, no warnings |
| `Frontend build`     | `frontend.yml`           | `yarn build`, the production bundle                             |
| `Frontend image`     | `frontend.yml`           | `frontend/Dockerfile` builds and the dev server answers         |
| `Compose config`     | `compose.yml`            | both compose files parse and interpolate                        |

Names are prefixed with their lane because a required check is matched by name alone, and two workflows each
reporting `Clippy` could not be told apart.

## Security

Elysium is public and the runners are self-hosted, so a workflow run is code execution on org hardware.

- The repository requires approval before any outside contributor's workflow runs.
- Workflows trigger on `pull_request`, never `pull_request_target`, so a pull request runs with a read-only token and
  no secrets.
- Every job carries the guard
  `github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository`. A pull
  request from a fork is skipped outright and never reaches a runner. Pushes, merge groups, and manual runs always come
  from this repository, so the guard never skips them. A skipped job counts as passing for a required check, which is
  safe because the merge queue runs every check again on the merge group, where nothing is skipped.
- Every workflow grants `permissions: contents: read` and nothing else, and checkout sets
  `persist-credentials: false`, so no token is left in the workspace or copied into the API image with `.git`.
- No workflow reads a secret. The placeholder values in the Compose and broker checks are not credentials.
- Third-party actions are pinned to a full commit SHA with the version in a comment. `actions/checkout` is the only
  one.

## Pins

Every tool is pinned to an exact version, in one place where the repository allows it, and verified.

| Tool                     | Pin                                         | Verified by                                              |
|--------------------------|---------------------------------------------|----------------------------------------------------------|
| Rust                     | `rust-toolchain.toml` in `api/` and `oauth-broker/` | rustup installs exactly that channel; each image job fails if its Dockerfile's `FROM rust:` differs |
| Node                     | `frontend/.nvmrc`                           | downloaded from nodejs.org, checked against `SHASUMS256.txt`, version asserted; the image job fails if `FROM node:` differs |
| Yarn                     | `packageManager` in `frontend/package.json`  | activated by corepack and asserted                       |
| Diesel CLI               | `DIESEL_VERSION` in `api.yml`, matching `verify-migrations.sh` | release binary checked against its published `.sha256`, version asserted |
| Postgres (migrations)    | `api/scripts/verify-migrations.sh`          | same image as compose                                    |
| `actions/checkout`       | commit SHA in each workflow                 |                                                          |

The runners carry their own Node, which is not the frontend's, so the pinned release is installed into the job's
temporary directory on every run. rustfmt and clippy are added with `rustup component add` rather than listed in
`rust-toolchain.toml`, because the images build from the minimal official Rust image and would otherwise download
both for nothing.

Two composite actions hold the setup the jobs share: `.github/actions/rust-toolchain` and
`.github/actions/frontend-dependencies`.

## API

`RUSTFLAGS=-D warnings` applies to every Cargo command in the API and broker workflows, so a compiler warning fails the
test build as well as clippy. `CARGO_INCREMENTAL=0` keeps the persistent target directories from filling with
incremental artifacts CI never reuses.

`API test` runs the hermetic suite. The database-backed tests are `#[ignore]`d there and run in `API migrations`, which
calls `api/scripts/verify-migrations.sh` exactly as a developer would (see `docs/database.md`): it starts a disposable
Postgres on a port Docker picks, checks `schema.rs` for drift, redoes every migration, and runs the whole suite with
`--include-ignored`. The script names its container after its own process id and removes it on exit. The workflow
records that process id, so a cancelled run, which can kill the script before its cleanup runs, still has the container
removed by an `always()` step. No `services:` container is used, because the script already owns its database.

The Diesel CLI is the release binary rather than `cargo install`. It is built with libpq bundled, so the runners need
no Postgres client library, and it needs glibc 2.34, which Rocky 9 ships.

`API image` builds `api/Dockerfile` from the repository root, as compose does, and runs `elysium-api --help` in it.
That proves the runtime stage links. Booting the server would need Postgres, Redis, and an encryption key, which the
checks above already cover.

## OAuth broker

The same format, clippy, and test checks as the API, against `oauth-broker/`. `Broker image` builds the image, boots
it with a sealing key generated for the run and placeholder Google credentials, and waits for `/healthz`.

## Frontend

Each job installs the pinned Node and Yarn and runs `yarn install --immutable`, which fails on a lockfile the install
would have changed. The org packages install from public GitHub repositories, so no token is needed. Typecheck, lint,
and build then run as separate jobs.

`Frontend image` builds `frontend/Dockerfile`, the Vite dev server compose runs, boots it, and waits for it to serve
`/`.

## Compose

`docker compose config --quiet` over `compose.yml` and `oauth-broker/compose.yml`. Compose refuses every command while
a required variable is unset and the checkout has no environment file, so the step sets placeholders for exactly the
required variables. Nothing is started. The images compose builds are built by the image checks instead of
`docker compose build`, whose fixed image names would race between concurrent runs on one daemon.

## Roadmap

- A production frontend image (static `vite build` output served by nginx), built and booted in `Frontend image`.
- A boot check for the API image against a disposable Postgres and Redis.
