// Copyright © 2026 Jalapeno Labs

// Core
import { app, BrowserWindow, ipcMain, shell } from 'electron'
import serve from 'electron-serve'
import localShortcut from 'electron-localshortcut'

// Misc
import { IpcChannel } from './ipc.js'
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
} from './constants.js'

// Serves the packaged frontend over a custom protocol in production. Created once
// here (even in dev, where it simply goes unused) so it is always ready and
// registers its privileged scheme before the app is ready.
const loadPackagedRenderer = serve({ directory: RENDERER_DIRECTORY })

// Loads the Elysium app into a window: the live dev URL in development, the
// packaged renderer in production. The React frontend owns every on-screen state
// (loading, errors, empty), so when a dev load fails because the stack is not up
// yet, we retry until it answers rather than rendering our own UI.
export async function loadApp(window: BrowserWindow): Promise<void> {
  if (window.isDestroyed()) {
    return
  }

  try {
    if (IS_DEV) {
      await window.loadURL(APP_URL)
    }
    else {
      await loadPackagedRenderer(window)
    }
  }
  catch (error) {
    console.debug(`Could not load the Elysium app; retrying in ${APP_RETRY_DELAY_MS}ms`, error)
    setTimeout(() => {
      loadApp(window)
    }, APP_RETRY_DELAY_MS)
  }
}

// Creates a top-level window and starts loading the app into it. Multiple windows
// are expected in the future, so nothing here is held as a singleton; callers and
// `BrowserWindow.getAllWindows()` own the lifecycle.
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
      // the renderer. Sandbox is off so the preload can import the shared typed
      // modules; see docs/desktop.md for the rationale.
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

  // Reload shortcuts, scoped to this window (they fire only while it is focused):
  // Ctrl/Cmd+R soft-reloads, Ctrl/Cmd+Shift+R reloads ignoring the cache.
  localShortcut.register(window, 'CommandOrControl+R', () => {
    window.webContents.reload()
  })
  localShortcut.register(window, 'CommandOrControl+Shift+R', () => {
    window.webContents.reloadIgnoringCache()
  })
  window.on('closed', () => {
    localShortcut.unregisterAll(window)
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

// Relaunch the whole desktop app: quit the current instance and start a fresh
// one. Used for recovering the GUI or applying changes that need a clean boot.
ipcMain.on(IpcChannel.Relaunch, () => {
  app.relaunch()
  app.quit()
})
