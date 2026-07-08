// Copyright © 2026 Jalapeno Labs

// Renderer-facing type for the Electron preload bridge (see
// desktop/src/preload.ts). The desktop shell exposes `window.elysium` via
// contextBridge; declaring it here gives the frontend a typed handle. It is
// optional because it is absent when Elysium runs in a plain browser rather than
// the desktop shell, so callers must guard with `window.elysium?.…`.
//
// The shape mirrors the desktop's ElysiumBridge. The frontend builds in isolation
// (the desktop package is not in its build context), so the contract is declared
// here rather than imported. A shared workspace package would let both sides
// consume one definition (see docs/desktop.md roadmap).
export type ElysiumBridge = {
  retry: () => void
  platform: string
  versions: {
    electron: string
    chrome: string
    node: string
  }
}

declare global {
  interface Window {
    elysium?: ElysiumBridge
  }
}

export {}
