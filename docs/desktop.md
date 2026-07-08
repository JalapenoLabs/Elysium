# Desktop shell

Elysium ships as an **Electron** desktop application. The desktop shell is a thin
native wrapper that hosts the Elysium web app; in production it will be the GUI
users launch, capable of multiple windows and rc-dock layouts spread across
monitors.

## Principle: dockerize the app, the shell just launches it

The shell does **not** bundle the web app. The entire Elysium stack (backend,
frontend, Postgres, Keycloak) runs as the dockerized Compose stack, and the
Electron shell simply points a window at it. This keeps one deployment model
(the containers) authoritative and means the desktop build only ever compiles and
packages the shell itself, never the app.

- **Dev**: the shell loads the Vite dev server at `http://localhost:5173`.
- **Production**: the shell loads the dockerized stack. Auto-starting Compose from
  the shell and serving the frontend as production static assets are roadmap
  items; today the shell loads a configurable URL.

The target URL is `http://localhost:5173` by default and is overridable with the
`ELYSIUM_APP_URL` environment variable. If the URL is unreachable, the window
shows a bundled launcher page (`static/launcher.html`) with a retry button rather
than a blank frame.

## Layout

Everything desktop lives in `desktop/`:

```
desktop/
├── Earthfile               # deb / rpm / appimage / windows / release targets
├── electron-builder.yml    # packaging config (targets, appId, output dir)
├── package.json            # pinned Electron + electron-builder toolchain
├── tsconfig.json           # compiles src/ -> dist/ as CommonJS
├── src/
│   ├── main.ts             # main process: window lifecycle, load/retry
│   ├── preload.ts          # minimal, explicit contextBridge surface
│   └── constants.ts        # app URL, window sizing, asset paths
└── static/
    └── launcher.html       # offline fallback shown when the app is unreachable
```

The renderer is sandboxed (`contextIsolation: true`, `nodeIntegration: false`,
`sandbox: true`). The only bridge between renderer and main is `preload.ts`, which
exposes a deliberately tiny `window.elysium` surface (a `retry()` and host version
info). It is widened one method at a time as real desktop capabilities land, never
wholesale.

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

## Packaging (Earthly)

Packaging is done with Earthly so it is reproducible and runs on any runner with
Earthly installed, no local Node toolchain required. All targets build on the
pinned `node:24.13.0-slim` image.

| Target       | Output                                    |
|--------------|-------------------------------------------|
| `+deb`       | Linux `.deb`                              |
| `+rpm`       | Linux `.rpm`                              |
| `+appimage`  | Linux `.AppImage`                         |
| `+windows`   | Windows NSIS `.exe` (cross-built via Wine)|
| `+release`   | all of the above, in parallel             |

```sh
cd desktop
earthly +release          # everything
earthly +deb              # just the Debian package
```

Installers are written to `desktop/release/`. Notes on the build environment:

- **AppImage** sets `APPIMAGE_EXTRACT_AND_RUN=1` because containers have no FUSE.
- **Windows** installs Wine (with the i386 architecture) so electron-builder can
  run the NSIS toolchain on Linux.
- `ELECTRON_SKIP_BINARY_DOWNLOAD=1` is set during install; electron-builder
  fetches the matching Electron itself during packaging.

## CI

The `Desktop` job in `.github/workflows/ci.yml` runs `earthly +release` in
parallel with the frontend and backend checks. It runs on **pull requests** and
**on-demand** (`workflow_dispatch`) only, not on every push to `main`, because the
packaging build is heavy. It targets a self-hosted runner tagged `linux` and
`earthly`. Artifacts are built but not uploaded anywhere yet.

## Roadmap

- **macOS**: `.dmg`/`.zip` targets, code signing, and notarization for a universal
  (arm64 + x64) build. macOS installers cannot be produced on a Linux runner, so
  this needs a macOS builder and is intentionally not wired yet. The `mac` block in
  `electron-builder.yml` is omitted until then; the app lifecycle in `main.ts`
  already follows macOS conventions (stay alive with no windows, recreate on
  `activate`).
- **Production app loading**: serve the frontend as production static assets and
  have the shell start / attach to the Compose stack instead of a dev URL.
- **Multi-window + rc-dock**: multiple windows with shared Redux state (via a
  shared web worker) and dockable, poppable panels across monitors.
- **Native integration**: application menus, deep links, notifications, tray, and
  auto-update.
- **Branded icons**: per-platform icon sets (currently the default Electron icon).
- **Artifact publishing**: upload the installers once a distribution channel is
  chosen.
