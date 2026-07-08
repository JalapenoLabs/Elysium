# Elysium desktop shell

Electron desktop shell for Elysium. It is a thin native wrapper that loads the
Elysium web app (dev: the Vite dev server; production: the packaged frontend via
electron-serve); it does not rebuild the app itself.

See [`docs/desktop.md`](../docs/desktop.md) for the full architecture, the
dockerize-the-app rationale, and the roadmap (including macOS).

## Dev

```sh
# From the repo root, start the app the shell will load:
docker compose up

# Here, compile-on-save and launch Electron with auto-restart:
yarn install
yarn dev
```

Override what the shell loads with `ELYSIUM_APP_URL` (defaults to
`http://localhost:5173`).

## Package

Installers are built with Electron Forge, wrapped by Earthly (no local Node
toolchain needed):

```sh
earthly +release          # deb + rpm + AppImage + Windows
earthly +deb              # a single target
```

Output lands in `out/make/`.

To run Forge directly (on a suitably provisioned host), select the target with
`ELYSIUM_MAKE_TARGET` and run `yarn make`:

```sh
yarn build
ELYSIUM_MAKE_TARGET=deb yarn make
ELYSIUM_MAKE_TARGET=windows yarn make --platform=win32
```

## Scripts

| Script           | Purpose                                        |
|------------------|------------------------------------------------|
| `yarn dev`       | tsc watch + Electron auto-restart              |
| `yarn build`     | compile `src/` -> `dist/`                      |
| `yarn start`     | run the app via Electron Forge                 |
| `yarn typecheck` | type-check without emitting                    |
| `yarn package`   | Forge package (unpacked app, no installer)     |
| `yarn make`      | Forge make (installer for `ELYSIUM_MAKE_TARGET`) |

Prefer the Earthly targets for packaging: they provide the system tooling (dpkg,
rpm, squashfs, Wine, Mono) in a reproducible container.
