// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow, UiNode, UpdateSettingsFlowBody } from '@ory/client-fetch'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { StrictMode } from 'react'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'

// Redux
import { createAppStore } from '../../../store'

// Misc
import { TestAriaRouter } from '../../../testRouter'
import { AddTwoFactorPage } from './AddTwoFactorPage'

// #165: adding 2FA asks for a name, then requires downloading backup codes, then verifies the
// app, and nothing is applied until that last step. Leaving at any earlier step leaves Kratos's
// unsaved flows to expire. The codes are saved only once the app is verified, after it.

const kratos = vi.hoisted(() => ({
  toSession: vi.fn(),
  createBrowserSettingsFlow: vi.fn(),
  getSettingsFlow: vi.fn(),
  updateSettingsFlow: vi.fn(),
}))

vi.mock('../../../api/kratos', () => ({ kratos }))

vi.mock('./twoFactorNickname', () => ({
  storeAuthenticatorNickname: vi.fn(() => Promise.resolve()),
}))

const CODES = [ 'abcd1234', 'efgh5678' ]

function input(name: string, value: unknown = ''): UiNode {
  return {
    type: 'input',
    group: 'default',
    attributes: { node_type: 'input', name, type: 'hidden', value, disabled: false },
    messages: [],
    meta: {},
  }
}

// A settings flow for someone with no second factor yet: an app to scan, codes to make. Only
// the parts the page reads; the rest of a flow is left out, hence the assertion.
function freshFlow(id: string): SettingsFlow {
  return {
    id,
    ui: {
      action: '',
      method: 'POST',
      nodes: [
        input('csrf_token', 'csrf'),
        input('lookup_secret_regenerate', true),
        {
          type: 'img',
          group: 'totp',
          attributes: { node_type: 'img', id: 'totp_qr', src: 'data:image/png;base64,AA==', width: 256, height: 256 },
          messages: [],
          meta: {},
        },
        {
          type: 'text',
          group: 'totp',
          attributes: { node_type: 'text', id: 'totp_secret_key', text: { id: 1, text: 'SECRETKEY', type: 'info' }},
          messages: [],
          meta: {},
        },
        input('totp_code'),
      ],
    },
  } as SettingsFlow
}

// The codes flow once Kratos made new codes, waiting to be confirmed.
function withNewCodes(flow: SettingsFlow): SettingsFlow {
  const codesNode: UiNode = {
    type: 'text',
    group: 'lookup_secret',
    attributes: {
      node_type: 'text',
      id: 'lookup_secret_codes',
      text: {
        id: 1,
        text: CODES.join(', '),
        type: 'info',
        context: { secrets: CODES.map((code) => ({ context: { secret: code }})) },
      },
    },
    messages: [],
    meta: {},
  }
  return { ...flow, ui: { ...flow.ui, nodes: [ ...flow.ui.nodes, codesNode, input('lookup_secret_confirm', true) ]}}
}

// A flow for someone whose app is set up.
function enrolledFlow(id: string): SettingsFlow {
  const nodes = [ input('csrf_token', 'csrf'), input('totp_unlink', true) ]
  return { id, ui: { action: '', method: 'POST', nodes }} as SettingsFlow
}

function submitted(): UpdateSettingsFlowBody[] {
  return kratos.updateSettingsFlow.mock.calls.map(([ request ]) => request.updateSettingsFlowBody)
}

// The button itself: a disabled one sits inside its tooltip's trigger, which shares its name.
function button(name: string) {
  const found = screen.getAllByRole('button', { name }).find((element) => element.tagName === 'BUTTON')
  if (!found) {
    throw new Error(`no ${name} button`)
  }
  return found
}

function renderPage(path: string) {
  const router = createMemoryRouter([
    {
      path: '/settings/security/two-factor/new',
      element: <TestAriaRouter>
        <AddTwoFactorPage />
      </TestAriaRouter>,
    },
    { path: '/settings/security', element: <p>Security page</p> },
  ], { initialEntries: [ path ]})
  // As the app runs in development: StrictMode doubles effects, which must not make codes twice.
  render(<StrictMode>
    <Provider store={createAppStore()}>
      <RouterProvider router={router} />
    </Provider>
  </StrictMode>)
  return router
}

describe('AddTwoFactorPage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    let created = 0
    kratos.toSession.mockResolvedValue({ authenticated_at: new Date() })
    kratos.createBrowserSettingsFlow.mockImplementation(() => {
      created += 1
      return Promise.resolve(freshFlow(`flow-${created}`))
    })
    // A reload, or the router remounting the page, reads the flows back by their ids.
    kratos.getSettingsFlow.mockImplementation(({ id }) => Promise.resolve(freshFlow(id)))
    kratos.updateSettingsFlow.mockImplementation(({ flow, updateSettingsFlowBody: body }) => {
      if (body.lookup_secret_regenerate) {
        return Promise.resolve(withNewCodes(freshFlow(flow)))
      }
      return Promise.resolve(enrolledFlow(flow))
    })
    // jsdom has none of these: the page watches its Save, the code's boxes their size, and the
    // download makes a `blob:` URL.
    const InertObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal('IntersectionObserver', InertObserver)
    vi.stubGlobal('ResizeObserver', InertObserver)
    URL.createObjectURL = vi.fn(() => 'blob:codes')
    URL.revokeObjectURL = vi.fn()
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('names the app first, and moves on only once it has a name', async () => {
    const router = renderPage('/settings/security/two-factor/new')
    const name = await screen.findByLabelText(/Nickname/)
    const next = button('Next')
    expect(next.hasAttribute('disabled')).toBe(true)

    fireEvent.change(name, { target: { value: 'Bitwarden' }})
    fireEvent.click(button('Next'))

    await waitFor(() => expect(router.state.location.search).toContain('step=codes'))
    expect(new URLSearchParams(router.state.location.search).get('name')).toBe('Bitwarden')
  })

  it('makes the codes, and holds Next until they are downloaded, applying nothing', async () => {
    const router = renderPage('/settings/security/two-factor/new?step=codes&name=Bitwarden')

    expect(await screen.findByText('abcd1234')).toBeTruthy()
    expect(submitted()).toEqual([
      expect.objectContaining({ method: 'lookup_secret', lookup_secret_regenerate: true }),
    ])
    const next = button('Next')
    expect(next.hasAttribute('disabled')).toBe(true)

    fireEvent.click(button('Download'))
    const blob = vi.mocked(URL.createObjectURL).mock.calls[0][0]
    expect(blob instanceof Blob && await blob.text()).toContain('App: Bitwarden')

    fireEvent.click(button('Next'))
    await waitFor(() => expect(router.state.location.search).toContain('step=app'))
    // Making codes saves nothing: neither the app nor the codes are on yet.
    expect(submitted()).toHaveLength(1)
  })

  it('verifies the app, then saves the codes from their own flow', async () => {
    const router = renderPage('/settings/security/two-factor/new?step=codes&name=Bitwarden')
    await screen.findByText('abcd1234')
    fireEvent.click(button('Download'))
    fireEvent.click(button('Next'))
    await screen.findByText('SECRETKEY')

    const turnOn = screen.getAllByRole('button', { name: 'Turn on 2FA' })
    expect(turnOn.every((element) => element.hasAttribute('disabled') || element.tagName !== 'BUTTON')).toBe(true)

    const codeInput = screen.getByLabelText('Authenticator code')
    fireEvent.change(codeInput, { target: { value: '123456' }})
    await act(async () => {
      fireEvent.click(button('Turn on 2FA'))
    })

    await waitFor(() => expect(router.state.location.pathname).toBe('/settings/security'))
    const calls = kratos.updateSettingsFlow.mock.calls.map(([ request ]) => request)
    expect(calls.map((request) => request.updateSettingsFlowBody)).toEqual([
      expect.objectContaining({ lookup_secret_regenerate: true }),
      expect.objectContaining({ method: 'totp', totp_code: '123456' }),
      expect.objectContaining({ method: 'lookup_secret', lookup_secret_confirm: true }),
    ])
    // The codes are confirmed in the flow that made them, not the app's.
    expect(calls[2].flow).toBe(calls[0].flow)
    expect(calls[1].flow).not.toBe(calls[0].flow)
  })
})
