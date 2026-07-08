// Copyright © 2026 Jalapeno Labs

import type { ForgeConfig, IForgeResolvableMaker } from '@electron-forge/shared-types'

// Installer targets, one per maker. Makers are referenced by module name (not
// imported) so Electron Forge resolves them itself; this also sidesteps ESM/CJS
// interop for the community AppImage maker. `platforms` limits each maker to the
// OS it can produce, so Forge skips the others for a given `make` invocation.
const MAKER_BY_TARGET: Record<string, IForgeResolvableMaker> = {
  // `bin` must match packagerConfig.executableName, or the Linux makers look for
  // a binary named after the package and fail to find the packaged executable.
  deb: {
    name: '@electron-forge/maker-deb',
    platforms: [ 'linux' ],
    config: { options: { bin: 'elysium', categories: [ 'Development' ] } },
  },
  rpm: {
    name: '@electron-forge/maker-rpm',
    platforms: [ 'linux' ],
    config: { options: { bin: 'elysium', categories: [ 'Development' ], license: 'UNLICENSED' } },
  },
  appimage: {
    name: '@reforged/maker-appimage',
    platforms: [ 'linux' ],
    config: { options: { bin: 'elysium', categories: [ 'Development' ] } },
  },
  windows: {
    name: '@electron-forge/maker-squirrel',
    platforms: [ 'win32' ],
    // Plain-text authors/owners: the package.json author carries an angle-bracket
    // email, which is valid for Debian's maintainer field but breaks Squirrel's
    // NuGet nuspec XML (the <email> reads as an unclosed tag).
    config: { authors: 'Jalapeno Labs LLC', owners: 'Jalapeno Labs LLC' },
  },
}

// Which installer(s) to build, chosen by env so CI can fan out one target per
// matrix leg. Unset builds every target (whatever matches the host platform).
function selectMakers(): IForgeResolvableMaker[] {
  const requested = process.env.ELYSIUM_MAKE_TARGET
  if (!requested) {
    return Object.values(MAKER_BY_TARGET)
  }

  const maker = MAKER_BY_TARGET[requested]
  if (!maker) {
    const known = Object.keys(MAKER_BY_TARGET).join(', ')
    throw new Error(`Unknown ELYSIUM_MAKE_TARGET '${requested}'. Expected one of: ${known}`)
  }
  return [ maker ]
}

const config: ForgeConfig = {
  packagerConfig: {
    name: 'Elysium',
    executableName: 'elysium',
    appBundleId: 'io.jalapenolabs.elysium.desktop',
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
