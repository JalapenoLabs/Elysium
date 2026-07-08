// Copyright © 2026 Jalapeno Labs

// Core
import { BrowserWindow, ipcMain, shell } from 'electron'

// Misc
import { IpcChannel } from './ipc'
import {
  IS_DEV,
  APP_URL,
  RENDERER_DIRECTORY,
  PRELOAD_SCRIPT,
  APP_RETRY_DELAY_MS,
  WINDOW_WIDTH,
  WINDOW_HEIGHT,
  WINDOW_MIN_WIDTH,
  WINDOW_MIN_HEIGHT,
} from './constants'

// Loads the packaged frontend over a custom protocol in production. Set once by
// `initRenderer` before the app is ready; null in development, where the live
// app URL is loaded instead.
let loadPackagedRenderer: ((window: BrowserWindow) => Promise<void>) | null = null

// Registers the production renderer server. electron-serve is ESM-only, so it is
// pulled in via dynamic import from this CommonJS module. Must run before the
// app `ready` event because electron-serve registers a privileged scheme.
export async function initRenderer(): Promise<void> {
  if (IS_DEV) {
    return
  }

  const { default: serve } = await import('electron-serve')
  loadPackagedRenderer = serve({ directory: RENDERER_DIRECTORY })
}

// Loads the Elysium app into a window. The React frontend owns every on-screen
// state (loading, errors, empty), so when a dev load fails because the stack is
// not up yet, we simply retry until it answers rather than rendering our own UI.
export async function loadApp(window: BrowserWindow): Promise<void> {
  if (window.isDestroyed()) {
    return
  }

  try {
    if (loadPackagedRenderer) {
      await loadPackagedRenderer(window)
    }
    else {
      await window.loadURL(APP_URL)
    }
  }
  catch (error) {
    console.debug(`Could not load the Elysium app; retrying in ${APP_RETRY_DELAY_MS}ms`, error)
    setTimeout(() => {
      loadApp(window)
    }, APP_RETRY_DELAY_MS)
  }
}

// Creates a top-level window and starts loading the app into it. Multiple
// windows are expected in the future, so nothing here is held as a singleton;
// callers and `BrowserWindow.getAllWindows()` own the lifecycle.
export function createMainWindow(): BrowserWindow {
  const window = new BrowserWindow({
    width: WINDOW_WIDTH,
    height: WINDOW_HEIGHT,
    minWidth: WINDOW_MIN_WIDTH,
    minHeight: WINDOW_MIN_HEIGHT,
    backgroundColor: '#0b0f1a',
    show: false,
    webPreferences: {
      preload: PRELOAD_SCRIPT,
      // Context isolation is the primary renderer boundary and Node stays out of
      // the renderer. Sandbox is off so the preload can `require` the shared
      // typed IPC module; see docs/desktop.md for the rationale.
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  })

  // Avoid a white flash: reveal the window only once content has painted.
  window.once('ready-to-show', () => {
    window.show()
  })

  // Any target="_blank" / window.open goes to the user's real browser rather
  // than spawning an unmanaged Electron window.
  window.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  loadApp(window)
  return window
}

// The renderer can ask the main process to (re)load the app (e.g. a reconnect
// control in the frontend). Reloads only the requesting window.
ipcMain.on(IpcChannel.Retry, (event) => {
  const window = BrowserWindow.fromWebContents(event.sender)
  if (!window) {
    console.debug('Received retry from an unknown window; ignoring')
    return
  }
  loadApp(window)
})
