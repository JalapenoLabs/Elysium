// Copyright © 2026 Jalapeno Labs

// The renderer-facing contract exposed as `window.elysium` by the preload bridge
// (see preload.mts). This is the single source of truth for the shape: the
// preload implements it, global.d.ts augments `window` with it, and the frontend
// consumes the same declaration (see docs/desktop.md). Kept free of any electron
// imports so the frontend can include it without pulling in electron types.
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

export type ElysiumBridge = {
  relaunch: () => void
  platform: string
  versions: {
    electron: string
    chrome: string
    node: string
  }
  // Connection management, present only in the desktop shell. The renderer's
  // connection screen drives which instance the client's API traffic goes to.
  connection: {
    list: () => Promise<ConnectionServer[]>
    active: () => Promise<ConnectionServer | null>
    add: (url: string) => Promise<{ server: ConnectionServer, instance: ConnectionInstance }>
    remove: (id: string) => Promise<void>
    select: (id: string) => Promise<void>
    test: (url: string) => Promise<ConnectionInstance>
  }
}
