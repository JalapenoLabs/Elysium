// Copyright © 2026 Jalapeno Labs

// Core
import { app, BrowserWindow, ipcMain, session } from 'electron'

// Misc
import { createWindow } from './window.js'
import { startConnectionProxy, probeInstance } from './proxy.js'
import { IS_DEV, APP_URL } from './constants.js'
import { IpcChannel } from './ipc.js'
import {
  loadConfig,
  listServers,
  getActiveServer,
  addServer,
  removeServer,
  selectServer,
  normalizeUrl,
} from './config.js'

app.whenReady().then(async () => {
  // Load saved connections first so the proxy can read the active instance
  // synchronously once requests start flowing.
  await loadConfig()
  registerConnectionIpc()

  // In development the renderer is the Vite dev server, which proxies /api to the
  // local stack. In a packaged build the local reverse proxy both serves the
  // renderer and forwards API/auth/realtime traffic to the chosen instance, so the
  // client can connect to any server.
  const appUrl = IS_DEV ? APP_URL : await startConnectionProxy()

  createWindow(appUrl)

  // macOS keeps apps alive with no windows; recreate one when the dock icon is
  // clicked. (macOS packaging is a later phase; the lifecycle is correct now so it
  // behaves once it lands.)
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow(appUrl)
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

// Wires the connection-management IPC the renderer's connection screen calls. The
// main process owns the saved instances and the proxy; the renderer only asks.
function registerConnectionIpc(): void {
  ipcMain.handle(IpcChannel.ConnectionList, () => listServers())
  ipcMain.handle(IpcChannel.ConnectionActive, () => getActiveServer())
  ipcMain.handle(IpcChannel.ConnectionTest, (_event, url: string) => probeInstance(normalizeUrl(url)))

  ipcMain.handle(IpcChannel.ConnectionAdd, async (_event, url: string) => {
    const normalized = normalizeUrl(url)
    // Validate the address is a reachable Elysium instance before saving it, and
    // adopt its reported name as the display identity.
    const instance = await probeInstance(normalized)
    const name = instance.name ?? new URL(normalized).host
    const server = await addServer(name, normalized)
    return { server, instance }
  })

  ipcMain.handle(IpcChannel.ConnectionRemove, (_event, id: string) => removeServer(id))

  ipcMain.handle(IpcChannel.ConnectionSelect, async (_event, id: string) => {
    await selectServer(id)
    // A different instance is a different session: clear cookies so the old
    // session is never sent to the new server, then reload every window to
    // reconnect and re-authenticate against the newly-active instance.
    await session.defaultSession.clearStorageData({ storages: [ 'cookies' ] })
    for (const window of BrowserWindow.getAllWindows()) {
      window.webContents.reload()
    }
  })
}
