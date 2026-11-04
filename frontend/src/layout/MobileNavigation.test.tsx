// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'

// Redux
import { createAppStore } from '../store'
import { meLoaded } from '../store/authSlice'
import { sidebarCollapsedChanged } from '../store/sidebarSlice'

// User interface
import { MobileNavigation } from './MobileNavigation'

// Misc
import { makeUser } from '../testFixtures'
import { TestAriaRouter } from '../testRouter'

function CurrentPath() {
  const location = useLocation()
  return <output aria-label='Current path'>{location.pathname}</output>
}

function renderAt(path: string) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser(),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  // The desktop sidebar's choice never reaches the drawer.
  store.dispatch(sidebarCollapsedChanged(true))

  return render(<Provider store={store}>
    <MemoryRouter initialEntries={[ path ]}>
      <TestAriaRouter>
        <MobileNavigation />
        <Routes>
          <Route path='*' element={<CurrentPath />} />
        </Routes>
      </TestAriaRouter>
    </MemoryRouter>
  </Provider>)
}

describe('MobileNavigation', () => {
  it('opens the primary links in a drawer, with their labels', async () => {
    renderAt('/action-items')

    expect(screen.queryByRole('dialog')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }))

    const drawer = await screen.findByRole('dialog')
    expect(drawer.textContent).toContain('Coding')
    expect(drawer.textContent).toContain('Studio')
    expect(screen.getByText('Coding').className).toContain('opacity-100')
  })

  it('goes to the page and closes once a link is followed', async () => {
    renderAt('/action-items')

    fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }))
    fireEvent.click(await screen.findByRole('link', { name: 'Coding' }))

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(screen.getByLabelText('Current path').textContent).toBe('/coding')
  })
})
