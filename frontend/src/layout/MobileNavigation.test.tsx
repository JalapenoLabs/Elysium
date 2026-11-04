// Copyright © 2026 Jalapeno Labs

import type { User } from '../api/routes/userRoutes'

// Core
import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter, Route, Routes } from 'react-router'

// Redux
import { createAppStore } from '../store'
import { meLoaded } from '../store/authSlice'

// User interface
import { MobileNavigation } from './MobileNavigation'
import { Sidebar } from './Sidebar'

// Misc
import { makeUser } from '../testFixtures'

function storeFor(user: User) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user,
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  return store
}

// At phone widths the sidebar kept its full 14rem, squeezing every page into the rest of a
// 375px screen. Below md the sidebar now hides and the topbar's menu opens the same links.
describe('Sidebar', () => {
  it('is hidden below md and shown from md up', () => {
    const { container } = render(<Provider store={storeFor(makeUser())}>
      <MemoryRouter>
        <Sidebar />
      </MemoryRouter>
    </Provider>)

    const sidebar = container.querySelector('aside')
    expect(sidebar?.classList.contains('hidden')).toBe(true)
    expect(sidebar?.classList.contains('md:flex')).toBe(true)
  })

  // Runs read what was ingested, which a guest never reaches.
  it('links to Runs for members alone', () => {
    const { unmount } = render(<Provider store={storeFor(makeUser())}>
      <MemoryRouter>
        <Sidebar />
      </MemoryRouter>
    </Provider>)
    expect(screen.getByRole('link', { name: 'Runs' })).toBeTruthy()
    unmount()

    render(<Provider store={storeFor(makeUser({ role: 'guest' }))}>
      <MemoryRouter>
        <Sidebar />
      </MemoryRouter>
    </Provider>)
    expect(screen.queryByRole('link', { name: 'Runs' })).toBeNull()
    expect(screen.getByRole('link', { name: 'Coding' })).toBeTruthy()
  })
})

describe('MobileNavigation', () => {
  function renderAt(path: string) {
    return render(<Provider store={storeFor(makeUser())}>
      <MemoryRouter initialEntries={[ path ]}>
        <MobileNavigation />
        <Routes>
          <Route path='*' element={<span />} />
        </Routes>
      </MemoryRouter>
    </Provider>)
  }

  it('opens the primary links in a drawer', async () => {
    renderAt('/action-items')

    expect(screen.queryByRole('dialog')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }))

    const drawer = await screen.findByRole('dialog')
    expect(drawer.textContent).toContain('Coding')
    expect(drawer.textContent).toContain('Studio')
  })

  it('closes once a link is followed', async () => {
    renderAt('/action-items')

    fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }))
    fireEvent.click(await screen.findByRole('link', { name: 'Coding' }))

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })
})
