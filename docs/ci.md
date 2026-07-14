# Continuous integration

Two workflows under `.github/workflows/`:

- **CI** (`ci.yml`): every push to `main`, every pull request targeting it, and
  `workflow_dispatch`.
- **Audit** (`audit.yml`): weekly (Mondays 09:00 UTC) and `workflow_dispatch`.

Dependabot (`.github/dependabot.yml`) opens weekly update PRs for npm
(frontend and desktop), cargo, github-actions, and both Dockerfiles.

`FORCE_COLOR` and `CARGO_TERM_COLOR` are set workflow-wide so tool output
keeps its colors in CI logs. A concurrency group per ref cancels superseded
in-flight runs. Jobs are deliberately independent and nothing is fail-fast:
one red check never masks the others.

## CI jobs

| Job           | What it does                                                     |
| ------------- | ---------------------------------------------------------------- |
| `proto`       | buf lint + format check over the shared proto tree               |
| `backend`     | fmt, clippy (`-D warnings`), build, tests, migration tests       |
| `frontend`    | install, lint, typecheck, vitest, proto sync, build, runtime load check, prod audit |
| `integration` | full Compose stack up; probes `/`, `/ok`, `/health`, protobuf endpoints, db health, Keycloak realm |
| `migrations`  | applies every SQL file to a throwaway postgres via psql          |
| `actionlint`  | lints the workflow files themselves (expressions, schema, shellcheck) |
| `desktop`     | Earthly packaging matrix (deb, rpm, AppImage, Windows) on PRs    |

Custom runner labels are declared in `.github/actionlint.yaml` so actionlint
can validate `runs-on` entries.

## Migration verification

Two complementary layers:

1. The `migrations` job applies each checked-in SQL file to a fresh throwaway
   postgres with psql, catching broken SQL and version-prefix collisions.
2. The `backend` job runs `cargo test -- --ignored`
   (`backend/tests/migrations.rs`) against a postgres service container on a
   runner-assigned ephemeral port: it exercises the *embedded* migrator the
   running service actually uses (`sqlx::migrate!`), proves a second run is a
   clean no-op (service restarts), and asserts the `_sqlx_migrations` ledger
   matches the embedded set exactly.

## Frontend runtime load check

After the build, `frontend/scripts/runtime-smoke.mjs` serves `dist/` on a
kernel-assigned ephemeral port and loads it in headless chromium (playwright)
with the API stubbed at the network layer (the session probe answers 401, the
normal anonymous path). It waits for the sign-in view to actually paint and
fails on any uncaught page error or console error, proving the production
bundle boots.

## Tests

- Backend: unit tests live in `#[cfg(test)]` modules beside the code (route
  responses via `tower::ServiceExt::oneshot`); postgres-backed integration
  tests live in `backend/tests/` marked `#[ignore]` and run explicitly with
  `cargo test -- --ignored`.
- Frontend: vitest + happy-dom + testing-library, colocated as
  `src/<name>.test.ts(x)`. Run with `yarn test` (or `yarn test:dev` to watch).

## Known wrinkle

`.mjs` files are excluded from `yarn lint` (`--ignore-pattern` in the lint
scripts) because the shared `@jalapenolabs/cli` eslint config only registers
its plugins for `ts/js/cjs/tsx/jsx` globs. The durable fix is adding `mjs` to
the shared config's file globs in the CLI repo.
