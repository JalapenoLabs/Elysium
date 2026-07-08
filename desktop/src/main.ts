// Copyright © 2026 Jalapeno Labs

// Core
import { app, BrowserWindow } from 'electron'

// Misc
import { createMainWindow } from './window.js'

app.whenReady().then(() => {
  createMainWindow()

  // macOS keeps apps alive with no windows; recreate one when the dock icon is
  // clicked. (macOS packaging is a later phase; the lifecycle is correct now so
  // it behaves once it lands.)
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createMainWindow()
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
