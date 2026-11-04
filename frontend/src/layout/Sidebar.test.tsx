// Copyright © 2026 Jalapeno Labs

import type { User } from '../api/routes/userRoutes'

// Core
import { describe, expect, it } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../store'
import { meLoaded } from '../store/authSlice'
import { selectIsSidebarCollapsed, sidebarCollapsedChanged } from '../store/sidebarSlice'

// User interface
import { Sidebar } from './Sidebar'

// Misc
import { makeUser } from '../testFixtures'
import { TestAriaRouter } from '../testRouter'
import { UrlTree } from '../urls'

function storeFor(user: User) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user,
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  store.dispatch(sidebarCollapsedChanged(false))
  return store
}

function renderSidebar(store = storeFor(makeUser()), path: string = UrlTree.coding) {
  const result = render(<Provider store={store}>
    <MemoryRouter initialEntries={[ path ]}>
      <TestAriaRouter>
        <Sidebar />
      </TestAriaRouter>
    </MemoryRouter>
  </Provider>)
  return {
    ...result,
    store,
    sidebar: result.container.querySelector('aside'),
  }
}

describe('Sidebar', () => {
  // At phone widths the sidebar kept its full width, squeezing every page into the rest of a
  // 375px screen. Below md it hides and the topbar's menu opens the same links.
  it('is hidden below md and shown from md up', () => {
    const { sidebar } = renderSidebar()

    expect(sidebar?.classList.contains('hidden')).toBe(true)
    expect(sidebar?.classList.contains('md:flex')).toBe(true)
  })

  // Runs read what was ingested, which a guest never reaches.
  it('links to Runs for members alone', () => {
    const { unmount } = renderSidebar(storeFor(makeUser()))
    expect(screen.getByRole('link', { name: 'Runs' })).toBeTruthy()
    unmount()

    renderSidebar(storeFor(makeUser({ role: 'guest' })))
    expect(screen.queryByRole('link', { name: 'Runs' })).toBeNull()
    expect(screen.getByRole('link', { name: 'Coding' })).toBeTruthy()
  })

  it('marks the page you are on, and its area for the pages under it', () => {
    renderSidebar(undefined, '/coding/5')

    expect(screen.getByRole('link', { name: 'Coding' }).getAttribute('aria-current')).toBe('page')
    expect(screen.getByRole('link', { name: 'Studio' }).getAttribute('aria-current')).toBeNull()
  })

  it('ends with the logo, linking to the main page', () => {
    renderSidebar()

    const links = screen.getAllByRole('link')
    const logo = links.at(-1)
    expect(logo?.textContent).toContain('Elysium')
    expect(logo?.getAttribute('href')).toBe(UrlTree.root)
  })

  it('collapses to its icons and expands again from the chevron', () => {
    const { sidebar, store } = renderSidebar()
    expect(sidebar?.classList.contains('w-60')).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }))
    expect(selectIsSidebarCollapsed(store.getState())).toBe(true)
    expect(sidebar?.classList.contains('w-16')).toBe(true)
    // The label is clipped away, not removed, so the link keeps its name.
    expect(screen.getByRole('link', { name: 'Coding' })).toBeTruthy()
    expect(screen.getByText('Coding').className).toContain('opacity-0')

    fireEvent.click(screen.getByRole('button', { name: 'Expand sidebar' }))
    expect(selectIsSidebarCollapsed(store.getState())).toBe(false)
    expect(sidebar?.classList.contains('w-60')).toBe(true)
  })

  it('names an icon in a tooltip while collapsed, and only then', async () => {
    // React Aria opens a tooltip on keyboard focus, so tab onto the link as a keyboard would.
    function tabOnto(name: string) {
      fireEvent.keyDown(document.body, { key: 'Tab' })
      act(() => screen.getByRole('link', { name }).focus())
    }

    const store = storeFor(makeUser())
    const { unmount } = renderSidebar(store)
    tabOnto('Studio')
    // Long enough for an enabled tooltip to have opened.
    await new Promise((resolve) => setTimeout(resolve, 500))
    expect(screen.queryByRole('tooltip')).toBeNull()
    unmount()

    store.dispatch(sidebarCollapsedChanged(true))
    renderSidebar(store)
    tabOnto('Studio')
    expect((await screen.findByRole('tooltip')).textContent).toBe('Studio')
  })
})
