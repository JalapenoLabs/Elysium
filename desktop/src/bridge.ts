// Copyright © 2026 Jalapeno Labs

// The renderer-facing contract exposed as `window.elysium` by the preload bridge
// (see preload.mts). This is the single source of truth for the shape: the
// preload implements it, global.d.ts augments `window` with it, and the frontend
// consumes the same declaration (see docs/desktop.md). Kept free of any electron
// imports so the frontend can include it without pulling in electron types.
export type ElysiumBridge = {
  relaunch: () => void
  platform: string
  versions: {
    electron: string
    chrome: string
    node: string
  }
}
