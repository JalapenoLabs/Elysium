// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'
import { API_BASE_PATH } from '../../constants'

// Mirrors `BrowserState` in api/src/browser/mod.rs.
export const BROWSER_STATES = [ 'starting', 'running', 'failed' ] as const
export type BrowserState = typeof BROWSER_STATES[number]

// A browser the host runs as a connector's account, for its owner to sign in on. It starts and
// is removed with its connector. Only its owner ever receives it; the password is never
// returned.
export type BrowserSession = {
  id: string
  name: string
  // The page it opened when it was created.
  startUrl: string
  // The host port its media arrives on.
  port: number
  status: {
    state: BrowserState
    // Why it failed, when it did.
    error: string | null
  }
  createdBy: string
  createdAt: string
  updatedAt: string
}

type ListBrowserSessionsResponse = {
  browserSessions: BrowserSession[]
}

export function listBrowserSessions() {
  return apiClient
    .get('v1/browser-sessions')
    .json<ListBrowserSessionsResponse>()
}

type BrowserSessionResponse = {
  browserSession: BrowserSession
}

export function getBrowserSession(browserSessionId: string) {
  return apiClient
    .get(`v1/browser-sessions/${browserSessionId}`)
    .json<BrowserSessionResponse>()
}

// Back to its start page in a fresh tab, every other tab closed, still signed in.
export function startOverBrowserSession(browserSessionId: string) {
  return apiClient.post(`v1/browser-sessions/${browserSessionId}/start-over`)
}

// Empties its profile, signing it out of everything, and starts it again at its start page.
export function resetBrowserSession(browserSessionId: string) {
  return apiClient
    .post(`v1/browser-sessions/${browserSessionId}/reset`)
    .json<BrowserSessionResponse>()
}

// The WebSocket a viewer negotiates the browser's stream over, on the page's own origin.
export function getBrowserStreamUrl(browserSessionId: string) {
  const scheme = location.protocol === 'https:'
    ? 'wss:'
    : 'ws:'
  return `${scheme}//${location.host}${API_BASE_PATH}/v1/browser-sessions/${browserSessionId}/stream`
}
