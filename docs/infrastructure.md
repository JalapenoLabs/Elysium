# Infrastructure

Three stacks share one compose file:

- **Production**, `compose.yml` alone: runs the published `jalapenolabs/elysium-api` image, which serves the API and
  the web app's production build and, as the `engine` service, runs the engine. `docker compose up --wait` pulls it.
- **Production from a checkout**, `compose.yml` with `compose.build.yml` layered over it: the same production images,
  built from the checkout under local tags, for a deployment that runs `main` rather than a release. Set
  `COMPOSE_FILE=compose.yml:compose.build.yml` in `.env` (adding `compose.override.yml` when the deployment has one),
  leave the `auto-update` profile off, and deploy with `git pull --ff-only && docker compose up --detach --build --wait`.
  The work instance at alex.mooreslab.ai runs this way.
- **Development**, `compose.yml` with `compose.dev.yml` layered over it: builds the API and the engine from the
  checkout and runs the web app on the Vite dev server with hot reload.
  `docker compose -f compose.yml -f compose.dev.yml up --build --wait`.

Either way the stack needs its configuration beside `compose.yml`: `nginx/`, `kratos/`, `hydra/`, and `updater/`,
which Compose builds into images on every `up` (see Configuration images below). A checkout has them, and so does
every API image, which carries the configuration it was released with (see Auto-update below).

`ELYSIUM_VERSION` in `.env` pins the release the production stack runs, such as `1.4.2`. Unset, it runs `latest`,
the newest release; pin it for a deployment that must not change under a `docker compose pull`.

Non-secret configuration, such as `CORS_ALLOWED_ORIGINS`, is written inline, once for the API and the engine together
(`x-server-environment`): they are one crate and read the same configuration. A `compose.override.yml` that sets a
variable for one must set it for both, such as `OAUTH_BROKER_URL`, which the engine needs to keep mailboxes signed in.
`.env`
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
| `nginx`    | `nginx/Dockerfile` (`nginx:1.30.4-alpine`) | `elysium-nginx` | The only published ports: web on `127.0.0.1:4000` by default, mail on 25, 465, 993, browser media on 59000-59007 (UDP and TCP) |
| `frontend` | `frontend/Dockerfile`      | `elysium-frontend` | Development only: Vite dev server       |
| `migrate`  | the API image              | `elysium-migrate`  | One-shot `migrate run`, then exits; creates Kratos's and Hydra's databases |
| `kratos-migrate` | `kratos/Dockerfile` (`oryd/kratos:v26.2.0`) | `elysium-kratos-migrate` | One-shot: Kratos's own migrations; builds Kratos's image |
| `kratos`   | Kratos's image, built by `kratos-migrate` | `elysium-kratos` | Identities and sessions; see `docs/auth.md` |
| `hydra-migrate` | `hydra/Dockerfile` (`oryd/hydra:v26.2.0`) | `elysium-hydra-migrate` | One-shot: Hydra's own migrations; builds Hydra's image |
| `hydra`    | Hydra's image, built by `hydra-migrate` | `elysium-hydra` | OAuth for MCP clients; see `docs/mcp.md` |
| `api`      | the API image              | none               | One replica; reachable only via nginx   |
| `engine`   | the API image, `elysium-engine` | `elysium-engine` | Every loop, schedule, and agent turn; one runs. Health on its own port 8080, `/health` |
| `postgres` | `pgvector/pgvector:0.8.7-pg18-trixie` (by digest) | `elysium-postgres` | Postgres 18.6 with pgvector; volume `postgres-data`, `timezone=UTC` |
| `redis`    | `redis:8.10.1-alpine`      | `elysium-redis`    | Password required, AOF on, `redis-data` |
| `docker-proxy` | `tecnativa/docker-socket-proxy:v0.5.0` | `elysium-docker-proxy` | The API's and the engine's filtered Docker API |
| `embeddings` | `jalapenolabs/elysium-embeddings` (`embeddings/Dockerfile`) | `elysium-embeddings` | The embedding model, on the internal `embeddings` network; 4 CPUs, 3 GiB; see `docs/knowledge.md`, Embeddings |
| `updater`  | `updater/Dockerfile` (`docker:29.9.1-cli`) | `elysium-updater` | Profile `auto-update` only; holds the Docker socket |

The `docker-control` network is internal and joins only the API, the engine, and `docker-proxy`. The `embeddings`
network is internal too, and joins only the API, the engine, and the embedding service, which is on no other network,
so nothing sent to the model can leave the host. The `elysium-mail`
network (subnet `172.29.53.0/24`) joins nginx at the fixed address `172.29.53.2`, the API, the engine, and the Stalwart
container; every other address comes from `172.29.53.128/25`. Change the subnet, the range, and `x-mail-ingress-address` together if
the subnet collides with one on the host.

The mail server is not a compose service. The API creates its container (`elysium-stalwart`), volumes, and network
through `docker-proxy` when the mail server is created on the Email settings page, and they live outside the
compose project; the engine brings it back up when it starts. See `docs/mail.md`.

Browsers are not compose services either. The engine creates each one's container, profile volume, and network, all
named `elysium-browser-<id>`, through `docker-proxy`, and joins itself, nginx, and every API replica to that network.
Compose drops those joins whenever it recreates nginx, the API, or the engine; the engine's reconciler restores them
within 10 seconds. The media
ports `59000-59007` are published on every interface, and a cloud host needs a firewall rule allowing them over UDP
and TCP. Browsers need Docker Engine 28.0 or later; see `docs/browser.md`.

The agent workspace is not a compose service either. The engine creates its container (`elysium-workspace`) and volume
(`elysium-workspace`) through `docker-proxy` when it starts, from `WORKSPACE_IMAGE`, the `elysium-workspace` image of
the same release, and runs every agent turn in it by `docker exec`; `docker-proxy` allows exec for that. The API works
in the running workspace too, for a session's files and a new session's clones, but never creates or replaces it. The
`elysium-agents` network joins only the API, which answers agents' tool calls there as `api`, and the workspace. It is
a plain bridge, because agents need the internet, and nginx is not on it. The development stack builds the image as
`elysium-workspace:dev` (service `workspace-image`) before the engine starts. See `docs/coding.md`.

Transcriptions are not compose services either. The engine creates one container per meeting it transcribes through
`docker-proxy`, named `elysium-whisper-<step job id>`, from `WHISPER_IMAGE`, the `elysium-whisper` image of the same
release, with no network at all, and removes it when the transcription ends. The recording goes in on its standard
input and the transcript comes out on its standard output, over Docker's attach stream. The development stack builds
the image as `elysium-whisper:dev` (service `whisper-image`) before the engine starts, and production from a checkout
as `elysium-whisper:local`. See `docs/meetings.md`, Transcription.

The embedding model is a compose service, `embeddings`, unlike Whisper: it is called all day, by the engine for every
passage of the knowledge base and by the API for every search, so it stays running. Production runs
`jalapenolabs/elysium-embeddings` of the same release; the development stack builds it as `elysium-embeddings:dev`
and production from a checkout as `elysium-embeddings:local`, each service building and running its own image. Its
four CPUs mean a host needs at least four. See `docs/knowledge.md`, Embeddings.

The engine keeps one volume of its own, `engine-data` at `/var/lib/elysium`: the Bitwarden CLI's state, so it stays
signed in as the same device across restarts, and each database dump of a running backup until it is uploaded (see
Backups below). Removing it only makes the CLI sign in again at the next start. The API keeps nothing; the image
declares no volume, so it gets none. See `docs/passwords.md`.

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
the production HTTP routing and `nginx/development.conf` the development routing, which `compose.dev.yml` builds in
its place (`nginx/Dockerfile`, stage `development`). Both include `nginx/locations.conf`, the locations they route alike (cover uploads, Kratos, Hydra, the event
stream, a browser's stream), so each file holds only what differs. `nginx/mail.conf` passes the mail ports to
Stalwart with the PROXY protocol; see `docs/mail.md`. `nginx/browser.conf` passes each media port to the browser
that owns it; see `docs/browser.md`.

The API has no container name so it can scale; it runs one replica today, for what `docs/engine.md` lists under One
replica of the API. The engine has a container name, which holds it to one: Elysia's intake must hear each event
once.

## Startup order

`migrate` waits for `postgres` health. `api`, `engine`, `kratos-migrate`, and `hydra-migrate` wait for `migrate` to
exit successfully; `api` and `engine` also wait for `redis` health, `kratos` for `kratos-migrate`, and `hydra` for
`hydra-migrate`. `nginx` waits for `api`, `kratos`, and `hydra` health, never the engine's: the web app does not need
it. Neither the API nor the engine waits for the other. Both retry their own connections, so a store restart mid-run
does not require restarting either.

`migrate`, `api`, and `engine` share one image, which has the migrations compiled in: `jalapenolabs/elysium-api` in
production, `elysium-api:local` in production from a checkout, and `elysium-api` in development. Built from a
checkout, `migrate` alone builds it and `api` and `engine` run it with `pull_policy: never`, for the reason each
configuration image has one builder (see Configuration images): two builders would make every deploy recreate both,
changed or not.

## Configuration images

nginx, Kratos, Hydra, and the updater run their upstream image with Elysium's configuration built in, from the
`Dockerfile` in each directory, never with the configuration bind-mounted. Compose recreates a container when its
definition or its image changes; a mounted file changes neither, so a deploy that changed only `kratos/kratos.yml`
would leave Kratos running on the old file. Top-level `configs:` behave the same outside a swarm: they are bind
mounts, and their content is not part of what Compose compares.

| Image                   | Built from                              | Built by         | Run by                     |
|-------------------------|-----------------------------------------|------------------|----------------------------|
| `elysium-nginx:local`   | `nginx/Dockerfile`, stage `production`  | `nginx`          | `nginx`                    |
| `elysium-kratos:local`  | `kratos/Dockerfile`                     | `kratos-migrate` | `kratos-migrate`, `kratos` |
| `elysium-hydra:local`   | `hydra/Dockerfile`                      | `hydra-migrate`  | `hydra-migrate`, `hydra`   |
| `elysium-updater:local` | `updater/Dockerfile`                    | `updater`        | `updater`                  |

- The building services set `pull_policy: build`, so every `docker compose up` rebuilds these images, `--build` or
  not, from the build cache when nothing changed. A changed file is a new image and the containers on the old one are
  recreated; an unchanged file is the same image and nothing restarts. Every stack gets this from the same plain
  `docker compose up`: development, production from a checkout, and a release's updater.
- Each image has one builder. Compose labels an image with the service that built it, so two services building one
  tag would make two different images of the same files, and every `up` would recreate both. `kratos` and `hydra` run
  their migrator's image with `pull_policy: never`, so `docker compose pull` skips them.
- Provenance attestations are off on every image the stacks build, these and the API, workspace, and frontend images
  built from a checkout: they are stamped with the build's time, which would make every build a new image. None of
  them is pushed anywhere.
- Compose also labels an image with its own version, so upgrading Compose rebuilds these images once and restarts
  their services once.
- Changing which upstream release runs is an edit to the `FROM` line in that directory's `Dockerfile`, where it is
  pinned (Kratos, Hydra, and the docker CLI by digest too).

The development stack (`compose.dev.yml`) builds nginx's `development` stage instead, as `elysium-nginx:dev`, which
routes every path outside `/api` to the Vite dev server. The web app's sources stay bind-mounted into the frontend
container: that is hot reload, which Vite watches for itself, not configuration.

CI holds every stack to these rules with `.github/scripts/check-compose.py`, and builds each configuration image (see
`docs/ci.md`, Compose).

## API image

Built from the repository root so `.git` can be copied into the builder for `/api/version`. `api/migrations` is
copied in as well, because the binary embeds the SQL. `cargo-chef` caches compiled dependencies in their own
layer. Source and `.git` are copied afterwards, so a commit only rebuilds the crate itself. The runtime image is
`debian:trixie-slim` with `ca-certificates`, `curl` for the healthcheck, and the Bitwarden CLI (`bw`, from its own
stage, pinned and checked against its published digest; see `docs/passwords.md`), running as a non-root user.

`api/Dockerfile` ends in two stages:

- `server`, the API and the engine, the crate's two binaries (`elysium-api`, the default command, and
  `elysium-engine`). The development stack builds this one, so a frontend edit never rebuilds the API.
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

`.github/workflows/publish.yml` pushes five images to Docker Hub under the `jalapenolabs` organization:

| Image                                  | Built from                         | Runs                                       |
|----------------------------------------|------------------------------------|--------------------------------------------|
| `jalapenolabs/elysium-api`             | `api/Dockerfile`, stage `app`      | The API and the web app, and the engine    |
| `jalapenolabs/elysium-workspace`       | `workspace/Dockerfile`             | Agent turns, in the container the engine makes |
| `jalapenolabs/elysium-whisper`         | `whisper/Dockerfile`               | Meetings' transcriptions, a container the engine makes for each |
| `jalapenolabs/elysium-embeddings`      | `embeddings/Dockerfile`            | The embedding model, the compose service `embeddings` |
| `jalapenolabs/elysium-oauth-broker`    | `oauth-broker/Dockerfile`          | The OAuth broker, standalone               |

Every push to `main` publishes `main` and `sha-<commit>`. A tag `vX.Y.Z` publishes `X.Y.Z`, and moves `X.Y` and
`latest` when it is the newest release; a pre-release tag such as `vX.Y.Z-rc.1` publishes only its own version. Each
image is labelled with the commit, the version, and this repository. The workflow is described in `docs/ci.md`.

All are `linux/amd64` only, since the runners are x86_64; an arm64 host cannot run them. Postgres, Redis, and the
socket proxy are upstream images pinned in `compose.yml`; nginx, Kratos, Hydra, and the updater are upstream images
pinned in their directory's `Dockerfile`, built on the host with the configuration (see Configuration images).

## Auto-update

A deployment that runs a release updates itself to each newer one. The API decides when (`api/src/updates/`), and
the updater, a compose service in the `auto-update` profile (`updater/updater.sh`), carries it out.

### Turning it on

The deployment directory needs only `.env` and what a release image carries, which it unpacks itself:

```sh
docker run --rm --entrypoint tar jalapenolabs/elysium-api:latest \
  -C /usr/share/elysium/deployment -cf - . | tar -xf - -C /srv/elysium
```

That writes `compose.yml`, `nginx/`, `kratos/`, `hydra/`, and `updater/`, which `up` builds the configuration images
from. Pin the release and turn the profile on in `.env`, then start the stack with `docker compose up --wait`:

```sh
ELYSIUM_VERSION=1.4.2
COMPOSE_PROFILES=auto-update
```

The updater owns the `ELYSIUM_VERSION` line from then on. Nothing else in `.env` is touched, nor anything else in the
directory, such as `compose.override.yml`.

### When it updates

The engine asks Docker Hub for the API image's release tags every hour, and the API on Settings, Updates, Check now;
either records the answer in `update_checks`. The API tells the updater to apply the newest release when all of these
hold:

- the running build is a release: its image was published from a `vX.Y.Z` tag, which bakes the version in
  (`/api/version` reports it as `release`). `main` and development builds never update themselves;
- the newest release is newer, and has not rolled back here before, unless an admin asks for it with Update now;
- no update is applying;
- nothing is running that a restart would cut short. Work such as an agent turn holds an activity
  (`api/src/activity.rs`) for as long as it runs, which the engine mirrors into `process_activities` on a lease it
  renews, and the update waits until no live lease lists one.

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
   agents run in and the Whisper image meetings are transcribed in, which are not compose services, and the
   embedding image, which is, so a failed pull stops the update before anything changes. `publish.yml` pushes the API
   image last, so a release whose API tag exists has all of them.
3. Backs up Elysium's, Kratos's, and Hydra's databases with `pg_dump` into `backups/<time>-<running>/`, keeping the
   newest five. This copy stays on the host and exists only to undo the update; the nightly backup off the host is
   separate (see Backups below).
4. Installs the configuration in place of the directory's, and points `ELYSIUM_VERSION` at the target. `.env` is
   rewritten in place, so it keeps its owner and mode.
5. Brings every service but the updater up, pulling only images Docker lacks (every one is a fixed version) and
   building the configuration images from what it installed, and waits up to ten minutes for every health check. It
   passes no `--pull` flag, which would override the configuration images' `pull_policy: build` and leave them on the
   running release's files. The `migrate` services run the target's migrations on the way.
6. Checks that the API answering is the target, reports success, and recreates itself last if its definition or its
   script changed.

When step 5 or 6 fails, it rolls back: the running release's configuration and version return, Postgres comes back up
on the image that configuration names, each database is dropped and recreated from the backup there (so tables the
failed release's migrations added are gone too, and every index is built under the collation that release reads it
with), and the stack comes back up. Anything written in the minutes between the backup and the rollback is lost with
it. A failure before step 4 changes nothing. Either way the release counts as rolled back and is held for an admin; the
next release applies on its own.

Compose resolves the stack's bind mounts against the deployment directory and passes the paths to the Docker daemon,
which reads them on the host. The updater therefore finds where its `/deployment` mount comes from and links that host
path to it, so compose runs at the path the daemon knows.

## Backups

Every night the engine backs up the three databases on the stack's Postgres (Elysium's own, Kratos's, and Hydra's) to
a storage location off the host, whether or not auto-update is on. The code is `api/src/backups/`.

### Turning them on

An admin chooses the storage location on Settings, Backups (`/settings/backups`): any Bunny Storage zone or S3 bucket
from Settings, Storage (`docs/storage.md`). The API reaches the location with its saved settings before it is chosen,
so a wrong key shows at once rather than at night. Until a location is chosen nothing is backed up: the page says so,
and the API's log warns once a night (`backups.schedule.no_destination`). The location cannot be deleted while it is
chosen.

### What a backup does

1. Dumps each database with the server's own `pg_dump --format custom`, run in the `postgres` container
   (`docker exec` through `docker-proxy`), exactly as the updater does, into `/var/lib/elysium/backups` on the
   engine's volume. The server's own binary always matches its version, and the custom format is compressed.
2. Uploads each dump to `database-backups/<started>-<database>.dump` under the location's directory, such as
   `database-backups/20261103T080000Z-elysium.dump`, and removes it from the volume.
3. Once all three are stored, removes every backup in `database-backups/` older than 30 days (`RETENTION_DAYS`).
   Nothing else in the location is touched: only files in that directory whose whole name has that shape.

A backup holds an activity while it runs, so auto-update never restarts the stack under it. One runs at a time.

### When it runs

- **Nightly** at 08:00 UTC (`BACKUP_TIME`), checked every ten minutes, so an engine that was down at 08:00 backs up
  as soon as it starts. Any backup that succeeded since 08:00, whatever started it, is that night's.
- **Back up now** on Settings, Backups.
- **Before a deploy**, by the deployment's own tooling, over `/internal/backups` (see below).

The last two are the API's: it records the backup as running and signals the engine, which takes it and runs it. One
asked for while the engine is down runs when it starts.

Every attempt is a `database_backups` row (`docs/database.md`), listed on Settings, Backups, and every change is
published as `backups.status`.

### When it fails

A failed attempt is logged at error level (`backups.run.failure`) with its reason, recorded on its row, and shown on
Settings, Backups. Whatever it had uploaded is removed again, so the location only ever holds whole backups. A
scheduled attempt that fails is retried an hour later, three attempts a night in all; when the last fails, an inbox
item, "Last night's database backup failed", says why, raised as `system`. A backup that a restart cut short is
recorded as failed when the engine next starts, and its files are removed.

### From a script

`/internal/backups` lets tooling on the host back up before it changes the stack, through the API container (nginx
never routes `/internal`). It speaks plain text and needs the updater's key, the hex SHA-256 of
`elysium/updater/key:<ELYSIUM_ENCRYPTION_KEY>`:

```sh
key="$(printf '%s' "elysium/updater/key:$ELYSIUM_ENCRYPTION_KEY" | sha256sum | cut -c1-64)"
call() { docker compose exec -T api curl -sS -H "x-elysium-updater-key: $key" "$@"; }
backup="$(call -X POST http://127.0.0.1:8080/internal/backups)"   # running <id>
until outcome="$(call "http://127.0.0.1:8080/internal/backups/${backup#running }")" \
  && [ "$outcome" != running ]; do sleep 5; done
echo "$outcome"                                                   # succeeded, or failed <reason>
```

`POST` answers `202` with `running <id>`, or `409` with `already-running` or `no-destination`. `GET
/internal/backups/{id}` answers `running`, `succeeded`, or `failed <reason>`.

### What a backup holds, and what it does not

A backup is the three databases alone. Without the same `ELYSIUM_ENCRYPTION_KEY` it is of little use: every secret
in Elysium's database is sealed with it, and Kratos and Hydra derive their own secrets from it. Keep a copy of `.env`
somewhere safe off the host; no backup includes it.

Nothing outside Postgres is backed up: not `engine-data` (the Bitwarden CLI signs in again), not the agent workspace's
volume (session directories and agents' checkouts), not the mail server's volumes, and not the browsers' profiles
(each browser must be signed in again).

### Restoring a backup

The restore replaces each database whole with its dump, as the updater's rollback does. Everything written since the
backup is lost.

1. **Have the stack's directory**, with the `.env` whose `ELYSIUM_ENCRYPTION_KEY` the backup was made under. Its
   Postgres credentials may differ: the restore runs as whoever `.env` names. Run the newest release, or `main`: a
   restored schema is brought up to date by the `migrate` services, but an older API refuses a newer schema.
2. **Pick the backup.** List `database-backups/` in the location and take the newest `<started>` that has all three
   files, one per database. Elysium's own is named for the database it came from, normally `POSTGRES_DB`.
3. **Download the three files** into `restore/` in the stack's directory, with the location's own credentials:

   ```sh
   started=20261103T080000Z
   # Google Cloud Storage, with the location's HMAC key
   AWS_ACCESS_KEY_ID=<access id> AWS_SECRET_ACCESS_KEY=<secret> aws s3 cp --recursive \
     --endpoint-url https://storage.googleapis.com --exclude '*' --include "$started-*" \
     s3://<bucket>/<directory>/database-backups/ restore/
   # Amazon S3: the same without --endpoint-url, with the IAM user's key
   # Bunny Storage: one file at a time, with the zone's password, from the region's endpoint
   curl --fail -H 'AccessKey: <zone password>' -o "restore/$started-elysium.dump" \
     "https://<endpoint>/<zone>/<directory>/database-backups/$started-elysium.dump"
   ```

4. **Start Postgres** if it is not running, and stop everything that holds a connection to it:

   ```sh
   docker compose up --detach --wait postgres
   docker compose stop nginx api engine kratos hydra
   ```

5. **Recreate each database from its dump.** `own` is Elysium's database: `POSTGRES_DB` in `.env`. If the dump came
   from a deployment whose `POSTGRES_DB` differed, rename that file to match first.

   ```sh
   own=elysium
   for database in "$own" kratos hydra; do
     docker compose exec -T postgres sh -c '
       psql --username "$POSTGRES_USER" --dbname postgres --quiet --set ON_ERROR_STOP=1 \
         --command "DROP DATABASE IF EXISTS \"$1\" WITH (FORCE)" \
         --command "CREATE DATABASE \"$1\"" \
       && pg_restore --username "$POSTGRES_USER" --dbname "$1" --no-owner --exit-on-error' \
       restore "$database" < "restore/$started-$database.dump"
   done
   ```

6. **Bring the stack back up** with `docker compose up --detach --wait`. The `migrate` services apply any migration
   newer than the backup on the way.
7. **Check it**: sign in, and open Settings, Backups, which now lists the history as of the backup. Sign each browser
   in again on its connector's page, under Settings, Connectors, if its profile was lost with the host.

To rehearse a restore without touching a deployment, run two scratch stacks beside each other: a compose project of
their own (`-p`) with an override renaming every `container_name`, started with `up --wait kratos hydra`, plus the
`docker-proxy` published on `127.0.0.1` for the first. The opt-in test
`backups::dump::tests::a_running_stacks_databases_dump_through_docker` dumps the first stack's databases with the
API's own dump code (`BACKUPS_TEST_DOCKER_URL`, `BACKUPS_TEST_POSTGRES_CONTAINER`, `BACKUPS_TEST_DATABASES`,
`BACKUPS_TEST_OUTPUT`); steps 4 to 6 then restore them into the second, whose Kratos and Hydra must come up healthy
and whose `migrate` must find the schema up to date.

## Time

Postgres runs with `timezone=UTC`, and every server container sets `TZ=UTC`.

## Persistence

Postgres 18 images place the cluster under `/var/lib/postgresql/<major>/docker`, so the named volume mounts
`/var/lib/postgresql`, not the older `.../data` path.

## Upgrading Postgres's image

Postgres is pgvector's build of the official image, pinned by digest in `compose.yml`, for the knowledge base's
embeddings (`docs/knowledge.md`). Its tag names pgvector's release and Postgres's major version; the digest fixes the
minor, today 18.6. A new digest is an edit to `compose.yml` and `api/scripts/verify-migrations.sh` together, which CI
compares (`docs/ci.md`, Pins). Within Postgres 18 the data directory carries over as it is, with no dump and restore.

The image is Debian (glibc), so a data volume first made by the Alpine image (`postgres:18.6-alpine3.23`, musl) starts
unchanged under it, but with every text index in musl's order. The first `migrate run` rebuilds them
(`docs/database.md`, Collations). Until it has, Kratos, Hydra, or the API could miss rows that are there, so upgrade
such a deployment with everything stopped:

```sh
git pull --ff-only
docker compose down                       # keeps the volumes
docker compose up --detach --build --wait
```

Nothing connects before `migrate` has rebuilt the indexes, since Kratos, Hydra, and the API all wait for it. Without
the `down`, Kratos and Hydra keep running across Postgres's restart: the rebuild still runs, but a write in those
seconds that slips a duplicate past a stale unique index makes the rebuild fail, and `migrate` with it, naming the
index. A release's updater runs `migrate` the same way, and its rollback restores each database from its dump into the
Postgres of the release it returns to (see What an update does). The updater that applies an update is the running
release's, so a release whose updater restores before bringing that Postgres up, rolling back from the pgvector image,
leaves the Alpine image reading indexes built under glibc: run `REINDEX DATABASE` in Elysium's, Kratos's, and Hydra's
databases after such a rollback.

The Debian image runs Postgres as uid 999 where Alpine's ran as 70. Its entrypoint, started as root as compose starts
it, gives the data directory to its own user before Postgres starts, so the volume needs nothing done to it.

The upgrade was rehearsed on 2026-11-04 with these compose files under a scratch project:

1. `postgres:18.6-alpine3.23` on a fresh volume, migrated by the published `main` image, with Kratos and Hydra up.
   Seeded with 1,200 people and 400 projects whose emails and names sort differently under musl and glibc, 120 Kratos
   identities with password identifiers, and 40 Hydra clients. `amcheck` (`bt_index_parent_check`) passed every btree
   index in all three databases.
2. The stack down, the volume kept, and Postgres alone up on the pgvector image: Postgres 18.6 started on the volume
   unchanged, and `amcheck` found `users_email_key` and `projects_name_unique` in Elysium's database and three of
   Kratos's identifier and recovery-address indexes out of order.
3. `migrate run` from this checkout rebuilt all three databases (`migration.collation.rebuilt`, from no version to
   glibc's 2.41) and applied the knowledge index. Every btree index in every database then passed `amcheck`, every
   seeded row was there, and a lookup by email through its index found its row.
4. `up --wait kratos hydra`: `kratos-migrate` and `hydra-migrate` exited 0, Kratos and Hydra came up healthy, Kratos's
   admin API found an identity by its identifier, and Hydra listed all 40 clients.

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

- More than one API replica, once what `docs/engine.md` lists under One replica of the API is kept in Postgres or
  Redis.
