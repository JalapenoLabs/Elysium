// Copyright © 2026 Jalapeno Labs

// Core
import { contextBridge, ipcRenderer } from 'electron'

// Misc
import type { ElysiumBridge } from './bridge.js'
import { IpcChannel } from './ipc.js'

// The preload is the only bridge between the renderer and the main process. Keep
// the exposed surface minimal and explicit: no Node, no raw ipcRenderer. Extend
// it one method at a time as real desktop capabilities land. The shape is the
// shared ElysiumBridge contract (bridge.ts), typed for the renderer via
// global.d.ts. This is a .mts file so it compiles to an ESM (.mjs) preload,
// which Electron requires for ESM preload scripts.
const elysiumBridge = {
  // Ask the main process to (re)load the Elysium app.
  retry(): void {
    ipcRenderer.send(IpcChannel.Retry)
  },
  // Ask the main process to relaunch the whole desktop app.
  relaunch(): void {
    ipcRenderer.send(IpcChannel.Relaunch)
  },
  // Static host info the renderer may want for diagnostics / conditional UI.
  platform: process.platform,
  versions: {
    electron: process.versions.electron,
    chrome: process.versions.chrome,
    node: process.versions.node,
  },
} satisfies ElysiumBridge

contextBridge.exposeInMainWorld('elysium', elysiumBridge)
