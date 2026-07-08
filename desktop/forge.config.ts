// Copyright © 2026 Jalapeno Labs

import type { ForgeConfig, IForgeResolvableMaker } from '@electron-forge/shared-types'

import { createRequire } from 'node:module'

// Pull identity from package.json so there is one source of truth for the app's
// name, executable, and license rather than duplicating them here.
const packageJson = createRequire(import.meta.url)('./package.json')

// Installer targets, one per maker. Makers are referenced by module name (not
// imported) so Electron Forge resolves them itself; this also sidesteps ESM/CJS
// interop for the community AppImage maker. `platforms` limits each maker to the
// OS it can produce, so Forge skips the others for a given `make` invocation.
// `bin` matches packagerConfig.executableName (the package name) so the Linux
// makers find the packaged executable.
const MAKER_BY_TARGET: Record<string, IForgeResolvableMaker> = {
  deb: {
    name: '@electron-forge/maker-deb',
    platforms: [ 'linux' ],
    config: {
      options: {
        bin: packageJson.name,
        categories: [ 'Development' ],
      },
    },
  },
  rpm: {
    name: '@electron-forge/maker-rpm',
    platforms: [ 'linux' ],
    config: {
      options: {
        bin: packageJson.name,
        categories: [ 'Development' ],
        license: packageJson.license,
      },
    },
  },
  appimage: {
    name: '@reforged/maker-appimage',
    platforms: [ 'linux' ],
    config: {
      options: {
        bin: packageJson.name,
        categories: [ 'Development' ],
      },
    },
  },
  windows: {
    name: '@electron-forge/maker-squirrel',
    platforms: [ 'win32' ],
    config: {
      // Plain-text authors/owners: package.json's author carries an angle-bracket
      // email, which is valid for Debian's maintainer field but breaks Squirrel's
      // NuGet nuspec XML (the <email> reads as an unclosed tag).
      authors: 'Jalapeno Labs LLC',
      owners: 'Jalapeno Labs LLC',
    },
  },
}

// Which installer(s) to build, chosen by env so CI can fan out one target per
// matrix leg. With no target (or an unrecognized one), build every installer
// that matches the host platform.
function selectMakers(): IForgeResolvableMaker[] {
  const requested = process.env.ELYSIUM_MAKE_TARGET
  const maker = requested ? MAKER_BY_TARGET[requested] : undefined
  if (!maker) {
    return Object.values(MAKER_BY_TARGET)
  }
  return [ maker ]
}

const config: ForgeConfig = {
  packagerConfig: {
    name: packageJson.productName,
    executableName: packageJson.name,
    // Ship only what the app needs at runtime; keep sources and build config out.
    ignore: [
      /^\/src/,
      /^\/\.yarn($|\/)/,
      /^\/tsconfig.*\.json$/,
      /^\/forge\.config\..*/,
      /^\/Earthfile$/,
      /^\/\.earthlyignore$/,
      /^\/README\.md$/,
    ],
  },
  makers: selectMakers(),
}

// Electron Forge loads the default export of its config file.
export default config
