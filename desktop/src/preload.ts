// Copyright © 2026 Jalapeno Labs

// Core
import { contextBridge, ipcRenderer } from 'electron'

// The preload is the only bridge between the sandboxed renderer and the main
// process. Keep the exposed surface minimal and explicit: we deliberately do not
// expose Node or the full ipcRenderer. As real desktop capabilities land
// (multi-window, native menus, deep links), extend this bridge one method at a
// time rather than widening it wholesale.
const elysiumBridge = {
  // Ask the main process to (re)load the Elysium app. Used by the launcher
  // fallback page's retry button.
  retry(): void {
    ipcRenderer.send('elysium:retry')
  },
  // Static host info the renderer may want for diagnostics / conditional UI.
  platform: process.platform,
  versions: {
    electron: process.versions.electron,
    chrome: process.versions.chrome,
    node: process.versions.node,
  },
} as const

export type ElysiumBridge = typeof elysiumBridge

contextBridge.exposeInMainWorld('elysium', elysiumBridge)
