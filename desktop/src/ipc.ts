// Copyright © 2026 Jalapeno Labs

// Central registry of IPC channel names shared between the main process and the
// preload bridge. Every channel string lives here so main and preload always
// reference the same value, never a stray literal. Add new channels to this map.
export const IpcChannel = {
  // Renderer asks the main process to relaunch the whole desktop app.
  Relaunch: 'elysium:relaunch',

  // Connection management: the client points its API traffic at a chosen instance.
  // List the saved instances.
  ConnectionList: 'elysium:connection:list',
  // The active instance (or null).
  ConnectionActive: 'elysium:connection:active',
  // Validate + save a new instance by URL (probes GET /api/v1/instance).
  ConnectionAdd: 'elysium:connection:add',
  // Forget a saved instance.
  ConnectionRemove: 'elysium:connection:remove',
  // Switch the active instance (clears cookies + reloads to reconnect).
  ConnectionSelect: 'elysium:connection:select',
  // Probe an address without saving it, to preview whose instance it is.
  ConnectionTest: 'elysium:connection:test',
} as const

export type IpcChannelValue = typeof IpcChannel[keyof typeof IpcChannel]
