// Copyright © 2026 Jalapeno Labs

// Core
import { contextBridge, ipcRenderer } from 'electron'

// Misc
import { IpcChannel } from './ipc'

// The preload is the only bridge between the renderer and the main process. Keep
// the exposed surface minimal and explicit: no Node, no raw ipcRenderer. Extend
// it one method at a time as real desktop capabilities land. The shape is typed
// for the renderer via global.d.ts.
const elysiumBridge = {
  // Ask the main process to (re)load the Elysium app.
  retry(): void {
    ipcRenderer.send(IpcChannel.Retry)
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
