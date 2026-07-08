// Copyright © 2026 Jalapeno Labs

// Central registry of IPC channel names shared between the main process and the
// preload bridge. Every channel string lives here so main and preload always
// reference the same value, never a stray literal. Add new channels to this map.
export const IpcChannel = {
  // Renderer asks the main process to (re)load the Elysium app. Sent by the
  // launcher fallback page's retry button.
  Retry: 'elysium:retry',
} as const

export type IpcChannelValue = typeof IpcChannel[keyof typeof IpcChannel]
