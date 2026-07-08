# Elysium desktop shell

Electron desktop shell for Elysium. It is a thin native wrapper that loads the
dockerized Elysium web app; it does not bundle the app itself.

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

Installers are built with Earthly (no local Node toolchain needed):

```sh
earthly +release          # deb + rpm + AppImage + Windows
earthly +deb              # a single target
```

Output lands in `release/`.

## Scripts

| Script                  | Purpose                                        |
|-------------------------|------------------------------------------------|
| `yarn dev`              | tsc watch + Electron auto-restart              |
| `yarn build`            | compile `src/` -> `dist/`                      |
| `yarn start`            | run the compiled shell once                    |
| `yarn typecheck`        | type-check without emitting                    |
| `yarn package:deb`      | build the Linux `.deb` (needs Linux tooling)   |
| `yarn package:rpm`      | build the Linux `.rpm`                         |
| `yarn package:appimage` | build the Linux `.AppImage`                    |
| `yarn package:windows`  | build the Windows installer (needs Wine)       |

Prefer the Earthly targets for packaging: they provide the system tooling (dpkg,
rpm, Wine) in a reproducible container. The `package:*` scripts are what Earthly
invokes and can be run directly on a suitably provisioned host.
