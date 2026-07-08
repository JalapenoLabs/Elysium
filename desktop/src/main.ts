// Copyright © 2026 Jalapeno Labs

// Core
import { app, BrowserWindow, ipcMain, shell } from 'electron'

// Misc
import {
  APP_URL,
  LAUNCHER_PAGE,
  PRELOAD_SCRIPT,
  WINDOW_WIDTH,
  WINDOW_HEIGHT,
  WINDOW_MIN_WIDTH,
  WINDOW_MIN_HEIGHT,
} from './constants'

// The single top-level window. The rich multi-window / rc-dock layout described
// in the project vision is a later phase; today the shell hosts one window that
// loads the dockerized Elysium web app.
let mainWindow: BrowserWindow | null = null

// Points the given window at the Elysium app. On failure (the stack is not up,
// or the URL is wrong) the window shows the bundled launcher page instead of a
// blank frame, so the user gets a clear message and a retry.
async function loadApp(window: BrowserWindow): Promise<void> {
  try {
    await window.loadURL(APP_URL)
  }
  catch (error) {
    console.debug(`Could not reach the Elysium app at ${APP_URL}; showing launcher`, error)
    await window.loadFile(LAUNCHER_PAGE)
  }
}

function createMainWindow(): BrowserWindow {
  const window = new BrowserWindow({
    width: WINDOW_WIDTH,
    height: WINDOW_HEIGHT,
    minWidth: WINDOW_MIN_WIDTH,
    minHeight: WINDOW_MIN_HEIGHT,
    backgroundColor: '#0b0f1a',
    show: false,
    webPreferences: {
      preload: PRELOAD_SCRIPT,
      // Secure defaults: isolate the renderer from Node and run it sandboxed.
      // The renderer only talks to the main process through the preload bridge.
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })

  // Avoid a white flash: reveal the window only once content has painted.
  window.once('ready-to-show', () => {
    window.show()
  })

  // Any target="_blank" or window.open goes to the user's real browser rather
  // than spawning an unmanaged Electron window.
  window.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  // A failed navigation (app URL unreachable) falls back to the launcher page.
  // errorCode -3 is an aborted load (e.g. a redirect); it is not a real failure.
  window.webContents.on('did-fail-load', (_event, errorCode, errorDescription, validatedURL) => {
    if (errorCode === -3) {
      return
    }
    console.debug(`Load failed (${errorCode} ${errorDescription}) for ${validatedURL}; showing launcher`)
    window.loadFile(LAUNCHER_PAGE)
  })

  return window
}

// The launcher page's "Retry" button asks the main process to try the app URL
// again, so the user can start the stack and reconnect without restarting.
ipcMain.on('elysium:retry', (event) => {
  const window = BrowserWindow.fromWebContents(event.sender)
  if (!window) {
    console.debug('Received elysium:retry from an unknown window; ignoring')
    return
  }
  loadApp(window)
})

app.whenReady().then(() => {
  mainWindow = createMainWindow()
  loadApp(mainWindow)

  // macOS keeps apps alive with no windows; recreate one when the dock icon is
  // clicked. (macOS packaging itself is a later phase; the lifecycle is handled
  // now so the behavior is correct once it lands.)
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      mainWindow = createMainWindow()
      loadApp(mainWindow)
    }
  })
})

// Quit when every window is closed, except on macOS where the convention is to
// stay running until the user explicitly quits.
app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit()
  }
})
