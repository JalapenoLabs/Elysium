// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { createMemoryRouter, RouterProvider } from 'react-router'

// User interface
import { UnknownRouteRedirect } from './UnknownRouteRedirect'

// Misc
import { UrlTree } from '../urls'

function renderAt(path: string) {
  const router = createMemoryRouter([
    {
      path: UrlTree.settings,
      element: <span>Settings page</span>,
    },
    {
      path: UrlTree.settingsLlms,
      element: <span>LLMs page</span>,
    },
    {
      path: '*',
      element: <UnknownRouteRedirect />,
    },
  ], { initialEntries: [ UrlTree.root, path ]})

  render(<RouterProvider router={router} />)
  return router
}

beforeEach(() => {
  // Each redirect logs the address it left; keep the output quiet.
  vi.spyOn(console, 'debug').mockImplementation(() => {})
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('UnknownRouteRedirect', () => {
  it('opens the nearest parent page of an unknown address', async () => {
    const router = renderAt('/settings/llms/does-not-exist')

    expect(await screen.findByText('LLMs page')).toBeTruthy()
    expect(router.state.location.pathname).toBe(UrlTree.settingsLlms)
  })

  it('replaces the unknown address, so Back skips it', async () => {
    const router = renderAt('/settings/does-not-exist')

    await screen.findByText('Settings page')
    expect(router.state.historyAction).toBe('REPLACE')
  })
})
