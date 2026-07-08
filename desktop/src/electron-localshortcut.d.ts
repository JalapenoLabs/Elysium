// Copyright © 2026 Jalapeno Labs

// Minimal ambient types for electron-localshortcut (3.2.1), which ships no types.
// It registers accelerators scoped to a single BrowserWindow, firing only while
// that window is focused (unlike globalShortcut).
declare module 'electron-localshortcut' {
  import type { BrowserWindow } from 'electron'

  interface LocalShortcut {
    register(window: BrowserWindow, accelerator: string | string[], callback: () => void): void
    unregister(window: BrowserWindow, accelerator: string | string[]): void
    unregisterAll(window: BrowserWindow): void
    isRegistered(window: BrowserWindow, accelerator: string): boolean
  }

  const localShortcut: LocalShortcut
  export = localShortcut
}
