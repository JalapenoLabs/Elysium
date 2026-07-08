# Desktop shell

Elysium ships as an **Electron** desktop application. The desktop shell is a thin
native wrapper that hosts the Elysium web app; in production it will be the GUI
users launch, capable of multiple windows and rc-dock layouts spread across
monitors.

## Principle: dockerize the app, the shell just launches it

The shell does not rebuild the app. The Elysium stack (backend, Postgres,
Keycloak, and the frontend) runs as the dockerized Compose stack, and the shell
loads the frontend:

- **Dev**: the shell loads the Vite dev server at `http://localhost:5173`.
  Override with `ELYSIUM_APP_URL`.
- **Production**: the built frontend SPA is served locally over a custom protocol
  by [`electron-serve`](https://github.com/sindresorhus/electron-serve) from the
  packaged `renderer/` directory, and talks to the dockerized backend over HTTP.

The React frontend owns every on-screen state (loading, errors, empty). When a
dev load fails because the stack is not up yet, the shell simply retries until it
answers rather than rendering its own page.

## Layout

Everything desktop lives in `desktop/`:

```
desktop/
├── Earthfile               # deb / rpm / appimage / windows / release targets
├── forge.config.ts         # Electron Forge: makers, ENV-selected targets
├── package.json            # pinned Electron + Forge toolchain
├── tsconfig.json           # compiles src/ -> dist/ as CommonJS
└── src/
    ├── main.ts             # app lifecycle (bootstrap, activate, quit)
    ├── window.ts           # window creation, app loading + retry, IPC handler
    ├── preload.ts          # minimal contextBridge surface (window.elysium)
    ├── ipc.ts              # central map of IPC channel names
    ├── constants.ts        # app URL, window sizing, paths, timings
    └── global.d.ts         # types window.elysium for the renderer/frontend
```

There is deliberately no window singleton: multiple windows are expected, so the
lifecycle is owned by `BrowserWindow.getAllWindows()`, not a module global.

### Security

`contextIsolation: true` and `nodeIntegration: false` are the primary renderer
boundaries. `sandbox` is disabled so the preload can `require` the shared, typed
`ipc.ts` module (a sandboxed preload cannot import local modules). The exposed
`window.elysium` surface is intentionally tiny and grows one method at a time.

### IPC

All channel names live in `src/ipc.ts` (`IpcChannel`), referenced by both the
main process and the preload so a channel string is never duplicated as a
literal. `window.elysium` is typed globally via `src/global.d.ts`, giving the
renderer (and the React frontend) a well-typed handle.

## Running in dev

Bring up the app stack (so there is something to load), then run the shell:

```sh
# From the repo root: start the dockerized app.
docker compose up

# In desktop/: compile-on-save and launch Electron with auto-restart.
cd desktop
yarn install
yarn dev
```

`yarn dev` runs the TypeScript compiler in watch mode alongside `electronmon`,
which restarts the app when the compiled output changes. Point the shell
elsewhere with `ELYSIUM_APP_URL=http://host:port yarn dev`.

## Packaging (Electron Forge + Earthly)

Packaging uses Electron Forge, driven by `forge.config.ts`. Which installer is
built is chosen by the `ELYSIUM_MAKE_TARGET` env var, so a single `yarn make`
command serves every target and CI can fan out one target per job.

Earthly wraps this so builds are reproducible on any runner with Earthly, no
local Node toolchain required. All targets build on the pinned
`node:24.13.0-slim` image.

| Target       | Maker                        | Output                          |
|--------------|------------------------------|---------------------------------|
| `+deb`       | `@electron-forge/maker-deb`  | Linux `.deb`                    |
| `+rpm`       | `@electron-forge/maker-rpm`  | Linux `.rpm`                    |
| `+appimage`  | `@reforged/maker-appimage`   | Linux `.AppImage`               |
| `+windows`   | `@electron-forge/maker-squirrel` | Windows `.exe` (via Wine + Mono) |
| `+release`   | all of the above             | everything, in parallel         |

```sh
cd desktop
earthly +release          # everything
earthly +deb              # just the Debian package
```

Installers are written to `desktop/out/make/`. Notes on the build environment:

- **AppImage** sets `APPIMAGE_EXTRACT_AND_RUN=1` because containers have no FUSE,
  and needs `squashfs-tools`.
- **Windows** installs Wine (with the i386 architecture) and Mono so Forge's
  Squirrel maker can run the Windows toolchain on Linux.
- `ELECTRON_SKIP_BINARY_DOWNLOAD=1` is set during install; Forge fetches the
  matching Electron itself during packaging.

The AppImage maker is the community `@reforged/maker-appimage`, since Forge ships
no first-party AppImage maker.

## CI

The `Desktop` job in `.github/workflows/ci.yml` is a matrix over the four
targets, each running `earthly +<target>` as an independent, parallel check
(`fail-fast: false`). It runs on **pull requests** and **on-demand**
(`workflow_dispatch`) only, not on every push to `main`, because the packaging
build is heavy. Earthly is installed at a pinned version via
`earthly/actions-setup`; the job targets a self-hosted runner tagged `linux` and
`earthly` (Docker available for BuildKit). Artifacts are built but not uploaded
anywhere yet.

## Roadmap

- **macOS**: `.dmg`/`.zip` targets, code signing, and notarization for a universal
  (arm64 + x64) build. macOS installers cannot be produced on a Linux runner, so
  this needs a macOS builder and is intentionally not wired yet. The lifecycle in
  `main.ts` already follows macOS conventions (stay alive with no windows,
  recreate on `activate`).
- **Production renderer**: wire the frontend build to populate `renderer/` during
  packaging so `electron-serve` has the SPA to serve.
- **Multi-window + rc-dock**: multiple windows with shared Redux state (via a
  shared web worker) and dockable, poppable panels across monitors.
- **Native integration**: application menus, deep links, notifications, tray, and
  auto-update.
- **Branded icons**: per-platform icon sets (currently the default Electron icon).
- **Shared workspace tooling**: a root `package.json` / unified `.yarnrc.yml` so
  the frontend and desktop share one Yarn setup.
- **Artifact publishing**: upload the installers once a distribution channel is
  chosen.
