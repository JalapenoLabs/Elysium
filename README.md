# Elysium

A base template for a full-stack app, hot-reloaded end to end via Docker Compose:

- **database** - Postgres (pinned to `17.5-alpine`)
- **backend** - Rust + [Axum](https://github.com/tokio-rs/axum) API on port `31416`, hot-reloaded with `watchexec`
- **frontend** - [Vite](https://vite.dev) + React 19 SPA on port `5173`, hot-reloaded with Vite HMR

## Stack

| Layer     | Choices |
|-----------|---------|
| Frontend  | React 19 (SWC), Redux Toolkit, React Router, HeroUI v2, Tailwind CSS v3, Ky, Vite |
| Backend   | Rust, Axum, Tokio, SQLx, tracing, mimalloc |
| Contract  | Protobuf (single `proto/` tree), prost (Rust) + buf/protoc-gen-es (TypeScript) |
| Identity  | Keycloak 26 (passwordless, passkeys, TOTP), dedicated Postgres |
| Database  | Postgres 17.5 |
| Tooling   | Docker Compose, Yarn Berry (node-modules linker) |

## Getting started

1. Copy the environment file and adjust if you like:

   ```sh
   cp .env.example .env
   ```

2. Build and start everything:

   ```sh
   docker compose up --build
   ```

   The first backend build is slow (it compiles the dev tooling and every
   dependency once), then caches for fast subsequent runs.

3. Open the app:

   - Frontend: http://localhost:5173
   - API liveness: http://localhost:31416/ (returns `pong`)
   - API health: http://localhost:31416/health (JSON)
   - API greeting: http://localhost:31416/api/v1/hello
   - API + DB check: http://localhost:31416/api/v1/db/health
   - Keycloak admin console: http://localhost:8080 (master-realm admin from `.env`)

   Liveness (`/`) and health (`/health`) are plain text and JSON for monitors.
   The versioned `/api/v1/*` responses are binary protobuf (`application/x-protobuf`),
   decoded by the frontend from types generated out of the `proto/` tree.

On the Home page, "Ping the API" calls the Rust backend with Ky and confirms it
can reach Postgres.

## Hot reload

Both services reload on save:

- **Frontend** - edit anything under `frontend/src`; Vite HMR updates the browser.
- **Backend** - edit anything under `backend/src`; `watchexec` reruns `cargo run`.

Polling is enabled for both watchers because Docker bind mounts from a Windows
host do not deliver filesystem events reliably.

## Project layout

```
.
├── compose.yaml            # database + backend + frontend
├── .env.example            # configuration defaults
├── backend/                # Rust / Axum API
│   ├── Dockerfile          # cargo-chef dep caching + watchexec hot reload
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         # bootstrap: logging, pool, server
│       ├── state.rs        # shared app state (Postgres pool)
│       └── api.rs          # routes and handlers
├── frontend/               # Vite + React SPA
│   ├── Dockerfile
│   ├── vite.config.ts      # dev server + polling for HMR
│   ├── tailwind.config.js  # HeroUI plugin wired in
│   └── src/
│       ├── main.tsx        # entry: Redux + Router providers
│       ├── router.tsx
│       ├── api/            # Ky client + route modules
│       ├── store/          # Redux Toolkit slices
│       ├── components/     # layout, navbar
│       └── pages/          # Home, About
└── desktop/                # Electron shell (loads the dockerized app)
    ├── Earthfile           # deb / rpm / appimage / windows / release
    ├── electron-builder.yml
    └── src/                # main + preload (TypeScript)
```

## Desktop shell

Elysium also ships as an Electron desktop app. The shell is a thin native wrapper
that loads the dockerized app rather than bundling it. Bring the stack up, then run
the shell:

```sh
docker compose up            # the app the shell will load

cd desktop
yarn install
yarn dev                     # tsc watch + Electron with auto-restart
```

Installers (Linux `.deb` / `.rpm` / `.AppImage`, Windows `.exe`) are built with
Earthly: `cd desktop && earthly +release`. macOS is on the roadmap. See
[`docs/desktop.md`](docs/desktop.md) for the full picture.

## Common commands

```sh
# Start in the background
docker compose up --build -d

# Tail logs
docker compose logs -f backend
docker compose logs -f frontend

# Stop and remove containers
docker compose down

# Also drop volumes (database data, cargo cache, node_modules)
docker compose down -v
```

Inside `frontend/` you can also run the usual checks:

```sh
yarn typecheck
yarn lint
yarn build
```

## Notes

- CORS is permissive on the backend for local development. Lock it down before
  deploying anywhere shared.
- Secrets live in `.env`, which is git-ignored. Never commit real credentials.
