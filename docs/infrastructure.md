# Infrastructure

`compose.yml` runs the whole stack. Non-secret configuration, such as `CORS_ALLOWED_ORIGINS`, is written inline.
`.env` supplies the bootstrap credentials: `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB`, `REDIS_PASSWORD`,
and `ELYSIUM_ENCRYPTION_KEY`. It may also supply `RUST_LOG`. Compose refuses to start when a required value is
missing, and `.env.example` is the template.

`WEB_PORT` may be set to move nginx off the default port 4000. `WEB_BIND_ADDRESS` publishes it on an address other
than the default `127.0.0.1`, such as `0.0.0.0` for a trusted LAN. Until the API has authentication, that hands
everyone who can reach the address the stored credentials and control of the Docker host; see `docs/security.md`.
`SMTP_PORT`, `SUBMISSIONS_PORT`, and `IMAPS_PORT`
move the host side of the mail ports off 25, 465, and 993.

Postgres applies its credentials only when the data volume is first created. After changing them, recreate the
volume with `docker compose down --volumes`, which destroys the data.

| Service    | Image / build              | Container name     | Notes                                   |
|------------|----------------------------|--------------------|-----------------------------------------|
| `nginx`    | `nginx:1.30.4-alpine`      | `elysium-nginx`    | The only published ports: web on `127.0.0.1:4000` by default, mail on 25, 465, 993 |
| `frontend` | `frontend/Dockerfile`      | `elysium-frontend` | Vite dev server, source bind-mounted    |
| `migrate`  | `api/Dockerfile`           | `elysium-migrate`  | One-shot `migrate run`, then exits      |
| `api`      | `api/Dockerfile`           | none               | One replica; reachable only via nginx   |
| `postgres` | `postgres:18.6-alpine3.23` | `elysium-postgres` | Volume `postgres-data`, `timezone=UTC`  |
| `redis`    | `redis:8.10.1-alpine`      | `elysium-redis`    | Password required, AOF on, `redis-data` |
| `docker-proxy` | `tecnativa/docker-socket-proxy:v0.5.0` | `elysium-docker-proxy` | The API's filtered Docker API |

The `docker-control` network is internal and joins only the API and `docker-proxy`. The `elysium-mail` network
(subnet `172.29.53.0/24`) joins nginx at the fixed address `172.29.53.2`, the API, and the Stalwart container; every
other address comes from `172.29.53.128/25`. Change the subnet, the range, and `x-mail-ingress-address` together if
the subnet collides with one on the host.

The mail server is not a compose service. The API creates its container (`elysium-stalwart`), volumes, and network
through `docker-proxy` when the mail server is created on the Email settings page, and they live outside the
compose project. See `docs/mail.md`.

`oauth-broker/` is a separate deployable with its own `compose.yml` and .env; it is not part of this
stack.

## Routing

nginx proxies `/api/` to `api:8080` unchanged and everything else to `frontend:5173`, including the HMR websocket
upgrade. `/api/v1/events` has its own location with buffering off and a one-hour read timeout, so server-sent
events arrive immediately and idle streams stay open.

The frontend's HMR client is told nginx's published port through `VITE_HMR_CLIENT_PORT`.

nginx runs its own main configuration, `nginx/nginx.conf`: the image's default plus a `stream` block, since raw TCP
can only be proxied from the main context. `nginx/default.conf` holds the HTTP routing above, and `nginx/mail.conf`
passes the mail ports to Stalwart with the PROXY protocol; see `docs/mail.md`.

The API has no container name so it could scale, but it must run as one replica today: its event bus is
in-process (see `docs/realtime.md`).

## Startup order

`migrate` waits for `postgres` health. `api` waits for `migrate` to exit successfully and for `redis` health.
`nginx` waits for `api` health. The API also retries its own connections, so a store restart mid-run does not
require restarting the API.

`migrate` and `api` share the `elysium-api` image, which has the migrations compiled in.

## API image

Built from the repository root so `.git` can be copied into the builder for `/api/version`. `api/migrations` is
copied in as well, because the binary embeds the SQL. `cargo-chef` caches compiled dependencies in their own
layer. Source and `.git` are copied afterwards, so a commit only rebuilds the crate itself. The runtime image is
`debian:trixie-slim` with `ca-certificates` and `curl` for the healthcheck, running as a non-root user.

The `arsox-sdk` dependency comes from git, so the build stages install `git` and copy `api/.cargo/config.toml`,
which makes Cargo fetch through the git CLI.

## Frontend image

`node:22.23.2-trixie-slim` with Yarn 4.17.0 activated through corepack. Dependencies install into the image.
Compose bind-mounts `src/`, `public/`, `index.html`, and `vite.config.ts`, so edits hot-reload without rebuilding.
A dependency change needs `docker compose up --build frontend`.

## Time

Postgres runs with `timezone=UTC`, and every server container sets `TZ=UTC`.

## Persistence

Postgres 18 images place the cluster under `/var/lib/postgresql/<major>/docker`, so the named volume mounts
`/var/lib/postgresql`, not the older `.../data` path.

## Automated pull request review

Every pull request is reviewed by Claude and Codex through `.github/workflows/pull-review.yml`, a thin wrapper around
the org's shared `JalapenoLabs/github-actions` `call-review-pr.yml` workflow. The review logic, models, and runner
live there, not here; see that repository's `docs/review-pr/guide.md`. Drafts are skipped until marked ready, pure
base-branch merges are skipped, and a head commit whose subject starts with `[REVIEW]` forces a review.

Only pull requests from branches of this repository are reviewed. Reviews run on a self-hosted runner, and a fork's
code must never execute there.

## Roadmap

- Production frontend image: static `vite build` output served by nginx directly.
- Redis pub/sub for the event bus, so the API can run more than one replica.
- CI pipeline building both images and running clippy, typecheck, lint, and `api/scripts/verify-migrations.sh`.
