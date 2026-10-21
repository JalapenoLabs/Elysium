// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'
import { API_BASE_PATH } from '../../constants'

// Mirrors `BrowserState` in api/src/browser/mod.rs.
export const BROWSER_STATES = [ 'starting', 'running', 'failed' ] as const
export type BrowserState = typeof BROWSER_STATES[number]

// A browser the host runs for its owner to sign in to an account in. Only its owner ever
// receives it; the password is never returned.
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

type CreateBrowserSessionRequest = {
  name: string
  startUrl: string
}

export function createBrowserSession(body: CreateBrowserSessionRequest) {
  return apiClient
    .post('v1/browser-sessions', { json: body })
    .json<BrowserSessionResponse>()
}

export function deleteBrowserSession(browserSessionId: string) {
  return apiClient.delete(`v1/browser-sessions/${browserSessionId}`)
}

// The WebSocket a viewer negotiates the browser's stream over, on the page's own origin.
export function getBrowserStreamUrl(browserSessionId: string) {
  const scheme = location.protocol === 'https:'
    ? 'wss:'
    : 'ws:'
  return `${scheme}//${location.host}${API_BASE_PATH}/v1/browser-sessions/${browserSessionId}/stream`
}
