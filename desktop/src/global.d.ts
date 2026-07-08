// Copyright © 2026 Jalapeno Labs

import type { ElysiumBridge } from './preload'

// The preload exposes `window.elysium` (see preload.ts). Declaring it on the
// global Window here gives the renderer, and the React frontend, a well-typed
// handle instead of reaching into an untyped `window`. When the frontend needs
// it, it can reference this declaration (or a shared package) rather than
// redeclaring the shape.
declare global {
  interface Window {
    elysium: ElysiumBridge
  }
}

export {}
