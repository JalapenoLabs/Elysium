// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { createMemoryRouter, Outlet, RouterProvider } from 'react-router'

// Misc
import { workspaceRoutes } from '../router'
import { UrlTree } from '../urls'

// Issue #172: the account pages are My account and Security, named the same in the menu, their
// titles, the settings directory, and their addresses. My account moved to `/settings/account`;
// its earlier addresses must still land there, so links and bookmarks to them keep working.

vi.mock('../pages/Settings/Account/AccountPage', () => ({
  AccountPage: () => <p>My account page</p>,
}))

// The workspace's own routes, without the app's frame around them.
function renderAt(path: string) {
  const router = createMemoryRouter([{ ...workspaceRoutes, element: <Outlet /> }], {
    initialEntries: [ path ],
  })
  render(<RouterProvider router={router} />)
  return router
}

describe('issue #172', () => {
  it.each([
    UrlTree.settingsAccount,
    UrlTree.settingsProfile,
    UrlTree.settingsPersonalDetails,
  ])('opens My account from %s', async (path) => {
    const router = renderAt(path)

    expect(await screen.findByText('My account page')).toBeTruthy()
    expect(router.state.location.pathname).toBe('/settings/account')
  })
})
