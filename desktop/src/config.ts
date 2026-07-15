// Copyright © 2026 Jalapeno Labs

// Core
import { app } from 'electron'

// Utility
import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, dirname } from 'node:path'
import { randomUUID } from 'node:crypto'

// A saved Elysium instance the client can connect to. `url` is the server's
// origin (scheme + host [+ port]); `name` is its display identity, captured from
// GET /api/v1/instance when the server was added.
export type Server = {
  id: string
  name: string
  url: string
}

type Store = {
  servers: Server[]
  activeId: string | null
}

const EMPTY: Store = { servers: [], activeId: null }

// Persisted under the OS-standard per-user app data directory, so a laptop keeps
// its saved instances and active choice across launches.
function storeFile(): string {
  return join(app.getPath('userData'), 'connections.json')
}

// Cached in memory so the proxy hot path reads the active server synchronously;
// the cache is the source of truth once loaded, and every mutation writes through.
let cache: Store | null = null

// Loads the store from disk once, defaulting to empty (and caching the result).
export async function loadConfig(): Promise<Store> {
  if (cache) {
    return cache
  }
  try {
    cache = JSON.parse(await readFile(storeFile(), 'utf8')) as Store
  }
  catch {
    // Missing or unreadable file: start empty. The connection screen prompts for
    // the first server.
    cache = { ...EMPTY }
  }
  return cache
}

async function save(store: Store): Promise<void> {
  cache = store
  const file = storeFile()
  await mkdir(dirname(file), { recursive: true })
  await writeFile(file, JSON.stringify(store, null, 2), 'utf8')
}

/** Every saved instance. */
export async function listServers(): Promise<Server[]> {
  return (await loadConfig()).servers
}

/** The active instance, or `null` when none is chosen yet. */
export async function getActiveServer(): Promise<Server | null> {
  const store = await loadConfig()
  return store.servers.find((server) => server.id === store.activeId) ?? null
}

/**
 * The active instance from the in-memory cache, for the proxy's synchronous path.
 *
 * Returns `null` before {@link loadConfig} has run; the app loads the config at
 * startup, so by the time requests flow the cache is populated.
 */
export function activeServerSync(): Server | null {
  if (!cache) {
    return null
  }
  return cache.servers.find((server) => server.id === cache!.activeId) ?? null
}

/** Adds an instance (and makes it active if it is the first), returning it. */
export async function addServer(name: string, url: string): Promise<Server> {
  const store = await loadConfig()
  const server: Server = { id: randomUUID(), name, url: normalizeUrl(url) }
  store.servers.push(server)
  if (!store.activeId) {
    store.activeId = server.id
  }
  await save(store)
  return server
}

/** Removes an instance; re-points the active choice if it was the one removed. */
export async function removeServer(id: string): Promise<void> {
  const store = await loadConfig()
  store.servers = store.servers.filter((server) => server.id !== id)
  if (store.activeId === id) {
    store.activeId = store.servers[0]?.id ?? null
  }
  await save(store)
}

/** Makes a known instance the active one. */
export async function selectServer(id: string): Promise<void> {
  const store = await loadConfig()
  if (store.servers.some((server) => server.id === id)) {
    store.activeId = id
    await save(store)
  }
}

// Normalizes a user-entered address to an origin: trims, drops any trailing slash,
// and defaults to https when no scheme is given (so "elysium.example.com" works).
export function normalizeUrl(url: string): string {
  const trimmed = url.trim().replace(/\/+$/, '')
  return /^https?:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`
}
