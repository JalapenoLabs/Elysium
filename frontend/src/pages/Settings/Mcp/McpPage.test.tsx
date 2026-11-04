// Copyright © 2026 Jalapeno Labs

import type { OAuthClient, OAuthGrant } from '../../../api/routes/oauthRoutes'
import type { UserRole } from '../../../api/routes/userRoutes'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'
import { SWRConfig } from 'swr'

// Redux
import { createAppStore } from '../../../store'
import { meLoaded } from '../../../store/authSlice'

// User interface
import { McpPage } from './McpPage'

// Misc
import { listOAuthClients, listOAuthGrants } from '../../../api/routes/oauthRoutes'

// The two lists are the network boundary.
vi.mock('../../../api/routes/oauthRoutes', () => ({
  listOAuthGrants: vi.fn(),
  listOAuthClients: vi.fn(),
  revokeOAuthGrant: vi.fn(),
  deleteOAuthClient: vi.fn(),
}))

function client(id: string, name: string): OAuthClient {
  return { id, name, uri: '', redirectHosts: [], createdAt: '2026-10-28T14:59:00Z' }
}

function grant(id: string, name: string): OAuthGrant {
  return { client: client(id, name), scopes: [ 'workspace:read' ], grantedAt: '2026-10-28T15:00:00Z' }
}

function renderAs(role: UserRole) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: {
      id: 'person',
      kind: 'person',
      name: 'Ada',
      email: 'ada@example.com',
      role,
      status: 'active',
      approvedAt: '2026-10-01T00:00:00Z',
      lastSeenAt: null,
      createdAt: '2026-10-01T00:00:00Z',
      updatedAt: '2026-10-01T00:00:00Z',
    },
    hasSecondFactor: true,
    mfaEnrollmentRequired: false,
  }))
  // A fresh cache per test, so one test's lists never answer another's.
  return render(<SWRConfig value={{ provider: () => new Map() }}>
    <Provider store={store}>
      <MemoryRouter>
        <McpPage />
      </MemoryRouter>
    </Provider>
  </SWRConfig>)
}

// jsdom has neither observer; the tabs and the settings frame each watch an element.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

describe('McpPage', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.stubGlobal('IntersectionObserver', InertObserver)
    vi.mocked(listOAuthGrants).mockResolvedValue({
      grants: [ grant('claude', 'Claude Code'), grant('zed', 'Zed') ],
    })
    vi.mocked(listOAuthClients).mockResolvedValue({
      clients: [ client('claude', 'Claude Code'), client('zed', 'Zed'), client('cursor', 'Cursor') ],
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('counts both tabs before either is opened', async () => {
    renderAs('admin')

    const grantsTab = screen.getByRole('tab', { name: /Your connected apps/ })
    const clientsTab = screen.getByRole('tab', { name: /All registered clients/ })
    expect(await within(grantsTab).findByText('2')).toBeTruthy()
    expect(await within(clientsTab).findByText('3')).toBeTruthy()
    expect(screen.getByText('Zed')).toBeTruthy()
    expect(screen.queryByText('Cursor')).toBeNull()
  })

  it('shows only a person their own connections', async () => {
    renderAs('member')

    const grantsTab = screen.getByRole('tab', { name: /Your connected apps/ })
    expect(await within(grantsTab).findByText('2')).toBeTruthy()
    expect(screen.queryByRole('tab', { name: /All registered clients/ })).toBeNull()
    expect(listOAuthClients).not.toHaveBeenCalled()
  })
})
