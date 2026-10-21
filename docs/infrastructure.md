# Infrastructure

Two stacks share one compose file:

- **Production**, `compose.yml` alone: runs the published `jalapenolabs/elysium-api` image, which serves the API and
  the web app's production build. `docker compose up --wait` pulls it.
- **Development**, `compose.yml` with `compose.dev.yml` layered over it: builds the API from the checkout and runs
  the web app on the Vite dev server with hot reload.
  `docker compose -f compose.yml -f compose.dev.yml up --build --wait`.

Either way the stack needs its configuration beside `compose.yml`: `nginx/`, `kratos/`, and `hydra/` are bind-mounted
from the deployment directory. A checkout has them, and so does every API image, which carries the configuration it
was released with (see Auto-update below).

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
| `nginx`    | `nginx:1.30.4-alpine`      | `elysium-nginx`    | The only published ports: web on `127.0.0.1:4000` by default, mail on 25, 465, 993, browser media on 59000-59007 (UDP and TCP) |
| `frontend` | `frontend/Dockerfile`      | `elysium-frontend` | Development only: Vite dev server       |
| `migrate`  | the API image              | `elysium-migrate`  | One-shot `migrate run`, then exits; creates Kratos's and Hydra's databases |
| `kratos-migrate` | `oryd/kratos:v26.2.0` | `elysium-kratos-migrate` | One-shot: Kratos's own migrations      |
| `kratos`   | `oryd/kratos:v26.2.0`      | `elysium-kratos`   | Identities and sessions; see `docs/auth.md` |
| `hydra-migrate` | `oryd/hydra:v26.2.0` | `elysium-hydra-migrate` | One-shot: Hydra's own migrations        |
| `hydra`    | `oryd/hydra:v26.2.0`       | `elysium-hydra`    | OAuth for MCP clients; see `docs/mcp.md` |
| `api`      | the API image              | none               | One replica; reachable only via nginx   |
| `postgres` | `postgres:18.6-alpine3.23` | `elysium-postgres` | Volume `postgres-data`, `timezone=UTC`  |
| `redis`    | `redis:8.10.1-alpine`      | `elysium-redis`    | Password required, AOF on, `redis-data` |
| `docker-proxy` | `tecnativa/docker-socket-proxy:v0.5.0` | `elysium-docker-proxy` | The API's filtered Docker API |
| `updater`  | `docker:29.9.1-cli`        | `elysium-updater`  | Profile `auto-update` only; holds the Docker socket |

The `docker-control` network is internal and joins only the API and `docker-proxy`. The `elysium-mail` network
(subnet `172.29.53.0/24`) joins nginx at the fixed address `172.29.53.2`, the API, and the Stalwart container; every
other address comes from `172.29.53.128/25`. Change the subnet, the range, and `x-mail-ingress-address` together if
the subnet collides with one on the host.

The mail server is not a compose service. The API creates its container (`elysium-stalwart`), volumes, and network
through `docker-proxy` when the mail server is created on the Email settings page, and they live outside the
compose project. See `docs/mail.md`.

Browsers are not compose services either. The API creates each one's container, profile volume, and network, all
named `elysium-browser-<id>`, through `docker-proxy`, and joins itself and nginx to that network. Compose drops
those joins whenever it recreates nginx or the API; the API's reconciler restores them within 10 seconds. The media
ports `59000-59007` are published on every interface, and a cloud host needs a firewall rule allowing them over UDP
and TCP. Browsers need Docker Engine 28.0 or later; see `docs/browser.md`.

The agent workspace is not a compose service either. The API creates its container (`elysium-workspace`) and volume
(`elysium-workspace`) through `docker-proxy` when it starts, from `WORKSPACE_IMAGE`, the `elysium-workspace` image of
the same release, and runs every agent turn in it by `docker exec`; `docker-proxy` allows exec for that. The
`elysium-agents` network joins only the API, which answers there as `api`, and the workspace. It is a plain bridge,
because agents need the internet, and nginx is not on it. The development stack builds the image as
`elysium-workspace:dev` (service `workspace-image`) before the API starts. See `docs/coding.md`.

The API keeps one volume of its own, `api-data` at `/var/lib/elysium`: the Bitwarden CLI's state, so it stays signed
in as the same device across restarts. Removing it only makes the CLI sign in again at the next start. See
`docs/passwords.md`.

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
arrive immediately and idle streams stay open, and cover uploads have a larger body limit. A browser's stream,
`/api/v1/browser-sessions/{id}/stream`, has its own location that passes the WebSocket upgrade.

In both, `/oauth2/` and Hydra's discovery documents (`/.well-known/openid-configuration`,
`/.well-known/oauth-authorization-server`, `/.well-known/jwks.json`) go to Hydra's public API (`hydra:4444`) unchanged,
with client registration throttled per client address. `/.well-known/oauth-protected-resource` goes to the API, which
describes its MCP server there (`docs/mcp.md`).

nginx runs its own main configuration, `nginx/nginx.conf`: the image's default plus a `stream` block, since raw TCP
can only be proxied from the main context, and the zones that throttle Kratos submissions and OAuth client registration. `nginx/default.conf` holds
the production HTTP routing and `nginx/development.conf` the development routing, which `compose.dev.yml` mounts in
its place. Both include `nginx/locations.conf`, the locations they route alike (cover uploads, Kratos, Hydra, the event
stream, a browser's stream), so each file holds only what differs. `nginx/mail.conf` passes the mail ports to
Stalwart with the PROXY protocol; see `docs/mail.md`. `nginx/browser.conf` passes each media port to the browser
that owns it; see `docs/browser.md`.

The API has no container name so it could scale, but it must run as one replica today: its event bus is
in-process (see `docs/realtime.md`).

## Startup order

`migrate` waits for `postgres` health. `api`, `kratos-migrate`, and `hydra-migrate` wait for `migrate` to exit
successfully; `api` also waits for `redis` health, `kratos` for `kratos-migrate`, and `hydra` for `hydra-migrate`.
`nginx` waits for `api`, `kratos`, and `hydra` health. The API also retries its own connections, so a store restart
mid-run does not require restarting the API.

`migrate` and `api` share one image, which has the migrations compiled in: `jalapenolabs/elysium-api` in production,
and `elysium-api`, built from the checkout, in development.

## API image

Built from the repository root so `.git` can be copied into the builder for `/api/version`. `api/migrations` is
copied in as well, because the binary embeds the SQL. `cargo-chef` caches compiled dependencies in their own
layer. Source and `.git` are copied afterwards, so a commit only rebuilds the crate itself. The runtime image is
`debian:trixie-slim` with `ca-certificates`, `curl` for the healthcheck, and the Bitwarden CLI (`bw`, from its own
stage, pinned and checked against its published digest; see `docs/passwords.md`), running as a non-root user.

`api/Dockerfile` ends in two stages:

- `server`, the API alone. The development stack builds this one, so a frontend edit never rebuilds the API.
  The build argument `ELYSIUM_RELEASE` names the release it is published as; build.rs bakes it in, and it is empty
  outside a release.
- `app`, the default and the published image: `server` plus the web app. A `frontend` stage runs `yarn build` on
  `node:22.23.2-trixie-slim`, the same Node as `frontend/.nvmrc`, alongside the Rust stages, and `app` copies its
  `dist/` to a fixed directory named by `FRONTEND_DIR`. The `frontend` stage also writes `.br` and `.gz` copies of
  every text asset at the highest settings, so the API sends compressed assets without compressing per request. The
  API serves it at every path outside `/api`, and refuses to start when that directory has no `index.html`. See
  `api/src/web_app.rs`. It also carries the deployment's configuration (`compose.yml`, `nginx/`, `kratos/`, `hydra/`,
  `updater/`) under `/usr/share/elysium/deployment`, so a release brings its own.

## Frontend image

`frontend/Dockerfile` is the development stack's Vite dev server: `node:22.23.2-trixie-slim` with Yarn 4.17.0
activated through corepack. Dependencies install into the image. `compose.dev.yml` bind-mounts `src/`, `public/`,
`index.html`, and `vite.config.ts`, so edits hot-reload without rebuilding. A dependency change needs
`docker compose -f compose.yml -f compose.dev.yml up --build frontend`. It is never published; production serves
the build inside the API image.

## Published images

`.github/workflows/publish.yml` pushes three images to Docker Hub under the `jalapenolabs` organization:

| Image                                  | Built from                         | Runs                                       |
|----------------------------------------|------------------------------------|--------------------------------------------|
| `jalapenolabs/elysium-api`             | `api/Dockerfile`, stage `app`      | The API and the web app                    |
| `jalapenolabs/elysium-workspace`       | `workspace/Dockerfile`             | Agent turns, in the container the API makes |
| `jalapenolabs/elysium-oauth-broker`    | `oauth-broker/Dockerfile`          | The OAuth broker, standalone               |

Every push to `main` publishes `main` and `sha-<commit>`. A tag `vX.Y.Z` publishes `X.Y.Z`, and moves `X.Y` and
`latest` when it is the newest release; a pre-release tag such as `vX.Y.Z-rc.1` publishes only its own version. Each
image is labelled with the commit, the version, and this repository. The workflow is described in `docs/ci.md`.

All are `linux/amd64` only, since the runners are x86_64; an arm64 host cannot run them. Postgres, Redis, nginx,
Kratos, and the socket proxy are upstream images pinned in `compose.yml`.

## Auto-update

A deployment that runs a release updates itself to each newer one. The API decides when (`api/src/updates/`), and
the updater, a compose service in the `auto-update` profile (`updater/updater.sh`), carries it out.

### Turning it on

The deployment directory needs only `.env` and what a release image carries, which it unpacks itself:

```sh
docker run --rm --entrypoint tar jalapenolabs/elysium-api:latest \
  -C /usr/share/elysium/deployment -cf - . | tar -xf - -C /srv/elysium
```

That writes `compose.yml`, `nginx/`, `kratos/`, `hydra/`, and `updater/`. Pin the release and turn the profile on in
`.env`, then start the stack with `docker compose up --wait`:

```sh
ELYSIUM_VERSION=1.4.2
COMPOSE_PROFILES=auto-update
```

The updater owns the `ELYSIUM_VERSION` line from then on. Nothing else in `.env` is touched, nor anything else in the
directory, such as `compose.override.yml`.

### When it updates

The API asks Docker Hub for the API image's release tags every hour, and on Settings, Updates, Check now. It tells
the updater to apply the newest release when all of these hold:

- the running build is a release: its image was published from a `vX.Y.Z` tag, which bakes the version in
  (`/api/version` reports it as `release`). `main` and development builds never update themselves;
- the newest release is newer, and has not rolled back here before, unless an admin asks for it with Update now;
- no update is applying;
- nothing is running that a restart would cut short. Work such as an agent turn holds an activity
  (`api/src/activity.rs`) for as long as it runs, and the update waits until none is held.

The updater asks every minute over `GET /internal/updater/instruction`, which answers `wait` or
`apply <target> from <running>`, and reports with `POST /internal/updater/outcome`: `succeeded <version>` or
`rolled-back <version> <reason>`. Both carry the key `x-elysium-updater-key`, the hex SHA-256 of
`elysium/updater/key:<ELYSIUM_ENCRYPTION_KEY>`. Each update is a `deployment_updates` row (`docs/database.md`). One
that reports nothing within 45 minutes counts as rolled back, and a release that starts while an update to it is
applying records it as succeeded. Every change is published as `updates.status`.

### What an update does

1. Pulls the target's API image, unless Docker has it already (release tags never move), and unpacks the
   configuration it carries.
2. Pulls every other Elysium image that configuration names at the target's version, such as the workspace image
   agents run in, which is not a compose service. `publish.yml` pushes the API image last, so a release whose API
   tag exists has all of them.
3. Backs up Elysium's, Kratos's, and Hydra's databases with `pg_dump` into `backups/<time>-<running>/`, keeping the
   newest five.
4. Installs the configuration in place of the directory's, and points `ELYSIUM_VERSION` at the target. `.env` is
   rewritten in place, so it keeps its owner and mode.
5. Brings every service but the updater up, pulling only images Docker lacks (every one is a fixed version), and
   waits up to ten minutes for every health check. The `migrate` services run the target's migrations on the way.
6. Checks that the API answering is the target, reports success, and recreates itself last if its own definition
   changed.

When step 5 or 6 fails, it rolls back: the running release's configuration and version return, each database is
dropped and recreated from the backup (so tables the failed release's migrations added are gone too), and the stack
comes back up. Anything written in the minutes between the backup and the rollback is lost with it. A failure before
step 4 changes nothing. Either way the release counts as rolled back and is held for an admin; the next release
applies on its own.

Compose resolves the stack's bind mounts against the deployment directory and passes the paths to the Docker daemon,
which reads them on the host. The updater therefore finds where its `/deployment` mount comes from and links that host
path to it, so compose runs at the path the daemon knows.

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
