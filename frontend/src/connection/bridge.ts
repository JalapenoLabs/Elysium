// Copyright © 2026 Jalapeno Labs

// The frontend's view of the desktop shell's `window.elysium` contract.
//
// The RUNTIME source of truth is `desktop/src/bridge.ts` (deliberately
// electron-free); the preload implements it. This file mirrors the same shape
// so the frontend typechecks standalone, without a cross-package tsconfig
// include reaching into the desktop project (that include proved fragile across
// environments). Keep this in sync with `desktop/src/bridge.ts`; both are small
// and change rarely. See docs/desktop.md.

// A saved Elysium instance the client can connect to.
export type ConnectionServer = {
  id: string
  name: string
  url: string
}

// The public identity a server reports (GET /api/v1/instance).
export type ConnectionInstance = {
  name: string | null
  claimed: boolean
  version: string
}

// Connection management, present only in the desktop shell.
export type ElysiumConnection = {
  list: () => Promise<ConnectionServer[]>
  active: () => Promise<ConnectionServer | null>
  add: (url: string) => Promise<{ server: ConnectionServer, instance: ConnectionInstance }>
  remove: (id: string) => Promise<void>
  select: (id: string) => Promise<void>
  test: (url: string) => Promise<ConnectionInstance>
}

export type ElysiumBridge = {
  relaunch: () => void
  platform: string
  versions: {
    electron: string
    chrome: string
    node: string
  }
  connection: ElysiumConnection
}

// `window.elysium` is exposed by the desktop preload and absent in a plain
// browser, so it is optional; callers guard with `window.elysium?.…`.
declare global {
  interface Window {
    elysium?: ElysiumBridge
  }
}
