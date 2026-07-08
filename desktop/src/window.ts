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
  WINDOW_WIDTH,
  WINDOW_HEIGHT,
  WINDOW_MIN_WIDTH,
  WINDOW_MIN_HEIGHT,
} from './constants.js'

// Serves the packaged frontend over a custom protocol in production. Created once
// here (even in dev, where it simply goes unused) so it is always ready and
// registers its privileged scheme before the app is ready.
const loadPackagedRenderer = serve({ directory: RENDERER_DIRECTORY })

// Creates a top-level window and loads the app into it: the live dev URL in
// development, the packaged renderer in production. Multiple windows are expected
// in the future, so nothing here is held as a singleton; callers and
// `BrowserWindow.getAllWindows()` own the lifecycle.
export function createWindow(): BrowserWindow {
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

  if (IS_DEV) {
    window.loadURL(APP_URL)
  }
  else {
    loadPackagedRenderer(window)
  }

  return window
}

// Relaunch the whole desktop app: quit the current instance and start a fresh
// one. Used for recovering the GUI or applying changes that need a clean boot.
ipcMain.on(IpcChannel.Relaunch, () => {
  app.relaunch()
  app.quit()
})
