// Copyright © 2026 Jalapeno Labs

import type { ElysiumBridge } from './bridge.js'

// The preload exposes `window.elysium` (see preload.mts). Declaring it on the
// global Window here gives the renderer, and the React frontend that includes
// this file, a well-typed handle instead of reaching into an untyped `window`.
// Optional because it is absent when Elysium runs in a plain browser rather than
// the desktop shell, so callers must guard with `window.elysium?.…`.
declare global {
  interface Window {
    elysium?: ElysiumBridge
  }
}

export {}
