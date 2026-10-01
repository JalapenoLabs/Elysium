# Infrastructure

Two stacks share one compose file:

- **Production**, `compose.yml` alone: runs the published `jalapenolabs/elysium-api` image, which serves the API and
  the web app's production build. `docker compose up --wait` pulls it.
- **Development**, `compose.yml` with `compose.dev.yml` layered over it: builds the API from the checkout and runs
  the web app on the Vite dev server with hot reload.
  `docker compose -f compose.yml -f compose.dev.yml up --build --wait`.

Either way the stack needs a checkout, or at least its `nginx/` and `kratos/` directories, because nginx's and Kratos's
configuration is bind-mounted from them.

`ELYSIUM_VERSION` in `.env` pins the release the production stack runs, such as `1.4.2`. Unset, it runs `latest`,
the newest release; pin it for a deployment that must not change under a `docker compose pull`.

Non-secret configuration, such as `CORS_ALLOWED_ORIGINS`, is written inline. `.env`
supplies the bootstrap credentials: `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB`, `REDIS_PASSWORD`, and
`ELYSIUM_ENCRYPTION_KEY`. It may also supply `RUST_LOG`, and `ELYSIUM_PUBLIC_URL`, the origin browsers reach Elysium at,
which defaults to `http://localhost:${WEB_PORT}` (see `docs/auth.md`). Compose refuses to start when a required value is
missing, and `.env.example` is the template.

`WEB_PORT` may be set to move nginx off the default port 4000. `WEB_BIND_ADDRESS` publishes it on an address other
than the default `127.0.0.1`, such as `0.0.0.0` for a LAN. Everyone who signs in shares the workspace, including its
stored credentials and, through the mail server, control of the Docker host; see `docs/security.md`. A deployment
reached from other machines also needs `ELYSIUM_PUBLIC_URL` set to an https host name, since browsers hold sessions
and passkeys only for such an origin.
`SMTP_PORT`, `SUBMISSIONS_PORT`, and `IMAPS_PORT` move the host side of the mail ports off 25, 465, and 993.

Postgres applies its credentials only when the data volume is first created. After changing them, recreate the
volume with `docker compose down --volumes`, which destroys the data.

| Service    | Image / build              | Container name     | Notes                                   |
|------------|----------------------------|--------------------|-----------------------------------------|
| `nginx`    | `nginx:1.30.4-alpine`      | `elysium-nginx`    | The only published ports: web on `127.0.0.1:4000` by default, mail on 25, 465, 993 |
| `frontend` | `frontend/Dockerfile`      | `elysium-frontend` | Development only: Vite dev server       |
| `migrate`  | the API image              | `elysium-migrate`  | One-shot `migrate run`, then exits; creates Kratos's database |
| `kratos-migrate` | `oryd/kratos:v26.2.0` | `elysium-kratos-migrate` | One-shot: Kratos's own migrations      |
| `kratos`   | `oryd/kratos:v26.2.0`      | `elysium-kratos`   | Identities and sessions; see `docs/auth.md` |
| `api`      | the API image              | none               | One replica; reachable only via nginx   |
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

In production nginx proxies `/api/identity/` to Kratos's public API (`kratos:4433`, prefix stripped) and every
other path to `api:8080` unchanged, except `/internal`, which it answers with a `404`: the API keeps those routes
for Kratos alone. The API answers `/api` itself and serves the web app's build at every other path.

In development nginx proxies `/api/identity/` the same way, the rest of `/api/` to the API, and everything else to
`frontend:5173`, including the HMR websocket upgrade; the frontend's HMR client is told nginx's published port
through `VITE_HMR_CLIENT_PORT`. Nothing outside `/api/` reaches the API there.

In both, POSTs to Kratos's sign-in, sign-up, and recovery endpoints are throttled per client address
(`docs/auth.md`).

In both, `/api/v1/events` has its own location with buffering off and a one-hour read timeout, so server-sent events
arrive immediately and idle streams stay open, and cover uploads have a larger body limit.

nginx runs its own main configuration, `nginx/nginx.conf`: the image's default plus a `stream` block, since raw TCP
can only be proxied from the main context, and the zone that throttles Kratos submissions. `nginx/default.conf` holds
the production HTTP routing and `nginx/development.conf` the development routing, which `compose.dev.yml` mounts in
its place. Both include `nginx/locations.conf`, the locations they route alike (cover uploads, Kratos, the event
stream), so each file holds only what differs. `nginx/mail.conf` passes the mail ports to Stalwart with the PROXY
protocol; see `docs/mail.md`.

The API has no container name so it could scale, but it must run as one replica today: its event bus is
in-process (see `docs/realtime.md`).

## Startup order

`migrate` waits for `postgres` health. `api` and `kratos-migrate` wait for `migrate` to exit successfully; `api` also
waits for `redis` health, and `kratos` for `kratos-migrate`. `nginx` waits for `api` and `kratos` health. The API also
retries its own connections, so a store restart mid-run does not require restarting the API.

`migrate` and `api` share one image, which has the migrations compiled in: `jalapenolabs/elysium-api` in production,
and `elysium-api`, built from the checkout, in development.

## API image

Built from the repository root so `.git` can be copied into the builder for `/api/version`. `api/migrations` is
copied in as well, because the binary embeds the SQL. `cargo-chef` caches compiled dependencies in their own
layer. Source and `.git` are copied afterwards, so a commit only rebuilds the crate itself. The runtime image is
`debian:trixie-slim` with `ca-certificates` and `curl` for the healthcheck, running as a non-root user.

The `arsox-sdk` dependency comes from git, so the build stages install `git` and copy `api/.cargo/config.toml`,
which makes Cargo fetch through the git CLI.

`api/Dockerfile` ends in two stages:

- `server`, the API alone. The development stack builds this one, so a frontend edit never rebuilds the API.
- `app`, the default and the published image: `server` plus the web app. A `frontend` stage runs `yarn build` on
  `node:22.23.2-trixie-slim`, the same Node as `frontend/.nvmrc`, alongside the Rust stages, and `app` copies its
  `dist/` to a fixed directory named by `FRONTEND_DIR`. The `frontend` stage also writes `.br` and `.gz` copies of
  every text asset at the highest settings, so the API sends compressed assets without compressing per request. The
  API serves it at every path outside `/api`, and refuses to start when that directory has no `index.html`. See
  `api/src/web_app.rs`.

## Frontend image

`frontend/Dockerfile` is the development stack's Vite dev server: `node:22.23.2-trixie-slim` with Yarn 4.17.0
activated through corepack. Dependencies install into the image. `compose.dev.yml` bind-mounts `src/`, `public/`,
`index.html`, and `vite.config.ts`, so edits hot-reload without rebuilding. A dependency change needs
`docker compose -f compose.yml -f compose.dev.yml up --build frontend`. It is never published; production serves
the build inside the API image.

## Published images

`.github/workflows/publish.yml` pushes two images to Docker Hub under the `jalapenolabs` organization:

| Image                                  | Built from                         | Runs                         |
|----------------------------------------|------------------------------------|------------------------------|
| `jalapenolabs/elysium-api`             | `api/Dockerfile`, stage `app`      | The API and the web app      |
| `jalapenolabs/elysium-oauth-broker`    | `oauth-broker/Dockerfile`          | The OAuth broker, standalone |

Every push to `main` publishes `main` and `sha-<commit>`. A tag `vX.Y.Z` publishes `X.Y.Z`, and moves `X.Y` and
`latest` when it is the newest release; a pre-release tag such as `vX.Y.Z-rc.1` publishes only its own version. Each
image is labelled with the commit, the version, and this repository. The workflow is described in `docs/ci.md`.

Both are `linux/amd64` only, since the runners are x86_64; an arm64 host cannot run them. Postgres, Redis, nginx,
Kratos, and the socket proxy are upstream images pinned in `compose.yml`.

## Time

Postgres runs with `timezone=UTC`, and every server container sets `TZ=UTC`.

## Persistence

Postgres 18 images place the cluster under `/var/lib/postgresql/<major>/docker`, so the named volume mounts
`/var/lib/postgresql`, not the older `.../data` path.

## Automated pull request review

Every pull request from a branch of this repository is reviewed by Claude and Codex through
`.github/workflows/pull-review.yml`, the org's standard consumer file, identical in every JalapenoLabs repository. It
runs on the self-hosted `reviewer` runner, checks out the private `JalapenoLabs/github-actions` repository into
`.reviewer/` for the length of the job, and runs its `review-pr` composite. The pipeline never lands in this
repository; see that repository's `docs/review-pr/guide.md`.

The workflow triggers on `pull_request_target`, so the copy on `main` always runs and a pull request cannot change how
it is reviewed. Pull requests from forks are never reviewed. Drafts are skipped until marked ready, pure base-branch
merges are skipped, and a head commit whose subject starts with `[REVIEW]` forces a review.

## CI

Formatting, lints, tests, migrations, and every image are checked on each pull request, merge group, and push to
`main`, and the images are published after a push to `main` or a version tag. See `docs/ci.md`.

## Roadmap

- Redis pub/sub for the event bus, so the API can run more than one replica.
