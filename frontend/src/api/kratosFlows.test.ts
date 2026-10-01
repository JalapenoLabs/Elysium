// Copyright © 2026 Jalapeno Labs

import type { KratosUi } from './kratosFlows'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { collectMessages, csrfToken, hasGroup, inputValue, toKratosFailure, toLocalPath } from './kratosFlows'

function input(name: string, group: string, value?: string, messages: { id: number, text: string }[] = []) {
  return {
    type: 'input',
    group,
    attributes: { node_type: 'input', name, type: 'hidden', value, disabled: false },
    messages: messages.map((message) => ({ ...message, type: 'error' })),
    meta: {},
  }
}

// Flows arrive as JSON; the nodes above are shaped the way Kratos sends them.
function flow(nodes: unknown[], messages: unknown[] = []) {
  return { id: 'flow-1', ui: { nodes, messages }} as unknown as KratosUi
}

describe('toKratosFailure', () => {
  it('returns the flow of a refused submission so its messages can show', () => {
    const refused = {
      id: 'flow-1',
      ui: { action: '/self-service/login?flow=flow-1', method: 'POST', nodes: [], messages: []},
    }
    expect(toKratosFailure(400, refused)).toEqual({ kind: 'invalid', flow: refused })
  })

  it('names what each Kratos error id asks of the page', () => {
    expect(toKratosFailure(403, { error: { id: 'session_refresh_required' }})).toEqual({ kind: 'refresh' })
    expect(toKratosFailure(403, { error: { id: 'session_aal2_required' }})).toEqual({ kind: 'secondFactor' })
    expect(toKratosFailure(410, { error: { id: 'self_service_flow_expired' }})).toEqual({ kind: 'expired' })
    expect(toKratosFailure(403, { error: { id: 'security_csrf_violation' }})).toEqual({ kind: 'expired' })
    expect(toKratosFailure(400, { error: { id: 'session_already_available' }})).toEqual({ kind: 'signedIn' })
    expect(toKratosFailure(401, { error: { id: 'session_inactive' }})).toEqual({ kind: 'signedOut' })
    // A disabled account's sign-in carries no id, only the reason to show.
    const disabled = { error: { message: 'identity is disabled', reason: 'This account was disabled.' }}
    expect(toKratosFailure(401, disabled)).toEqual({ kind: 'failed', message: 'This account was disabled.' })
  })

  it('follows a redirect Kratos asks the browser to make', () => {
    const body = { error: { id: 'browser_location_change_required' }, redirect_browser_to: '/settings/security?flow=2' }
    expect(toKratosFailure(422, body)).toEqual({ kind: 'redirect', to: '/settings/security?flow=2' })
  })

  it('treats a gone flow as expired and anything else as a failure with its reason', () => {
    expect(toKratosFailure(410, null)).toEqual({ kind: 'expired' })
    expect(toKratosFailure(500, { error: { message: 'boom', reason: 'The database is down.' }}))
      .toEqual({ kind: 'failed', message: 'The database is down.' })
    expect(toKratosFailure(502, null)).toEqual({ kind: 'failed', message: null })
  })
})

describe('flow nodes', () => {
  const loginFlow = flow([
    input('csrf_token', 'default', 'token-1'),
    input('identifier', 'default', 'ada@example.com', [{ id: 4000006, text: 'Invalid credentials.' }]),
    input('passkey_challenge', 'passkey', '{"publicKey":{}}'),
  ], [{ id: 1, text: 'Welcome', type: 'info' }])

  it('reads input values and the anti-forgery token', () => {
    expect(csrfToken(loginFlow)).toBe('token-1')
    expect(inputValue(loginFlow, 'identifier')).toBe('ada@example.com')
    expect(inputValue(loginFlow, 'missing')).toBe('')
  })

  it('knows which methods a flow offers', () => {
    expect(hasGroup(loginFlow, 'passkey')).toBe(true)
    expect(hasGroup(loginFlow, 'totp')).toBe(false)
  })

  it('collects the form messages and each field’s', () => {
    const messages = collectMessages(loginFlow)
    expect(messages.form.map((message) => message.text)).toEqual([ 'Welcome' ])
    expect(messages.byField.identifier.map((message) => message.text)).toEqual([ 'Invalid credentials.' ])
    expect(messages.byField.csrf_token).toBeUndefined()
  })
})

describe('toLocalPath', () => {
  it('keeps paths on this origin', () => {
    expect(toLocalPath('/projects?view=tiles')).toBe('/projects?view=tiles')
  })

  it('drops anything that could leave the site', () => {
    expect(toLocalPath('https://evil.example')).toBeNull()
    expect(toLocalPath('//evil.example/path')).toBeNull()
    expect(toLocalPath('')).toBeNull()
    expect(toLocalPath(null)).toBeNull()
  })

  // A backslash reads as a slash in an http URL, so `/\host` leaves the origin although its
  // text starts with a single slash. This was an open redirect after signing in.
  it('drops paths the browser would resolve to another host', () => {
    expect(toLocalPath('/\\evil.example')).toBeNull()
    expect(toLocalPath('/\\/evil.example/path')).toBeNull()
    expect(toLocalPath('/\t/evil.example')).toBeNull()
    expect(toLocalPath('javascript:alert(1)')).toBeNull()
  })

  it('normalizes what it keeps to the path the browser would open', () => {
    expect(toLocalPath('/settings/../projects#top')).toBe('/projects#top')
  })
})
