// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter, Route, Routes } from 'react-router'

// Redux
import { createAppStore } from '../store'
import { meLoaded } from '../store/authSlice'

// User interface
import { UserMenu } from './UserMenu'

// Misc
import { makeUser } from '../testFixtures'

function renderMenu() {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser(),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  return render(<Provider store={store}>
    <MemoryRouter initialEntries={[ '/action-items' ]}>
      <UserMenu />
      <Routes>
        <Route path='/settings/profile' element={<p>Profile page</p>} />
        <Route path='*' element={<span />} />
      </Routes>
    </MemoryRouter>
  </Provider>)
}

// The account menu offered only Sign-in & security and Sign out. My profile now leads it, every
// item starts with an icon, and Sign out sits last below a divider.
describe('UserMenu', () => {
  it('leads with My profile and ends with Sign out below a divider', async () => {
    renderMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Account menu for Ada Lovelace' }))

    const menu = await screen.findByRole('menu')
    const entries = Array.from(menu.querySelectorAll('[role="menuitem"], [role="separator"]'))
    const labels = entries.map((entry) => entry.getAttribute('role') === 'separator'
      ? 'divider'
      : entry.textContent)
    expect(labels).toEqual([ 'My profile', 'Sign-in & security', 'divider', 'Sign out' ])

    for (const item of within(menu).getAllByRole('menuitem')) {
      expect(item.querySelector('svg')).not.toBeNull()
    }
  })

  it('opens My profile', async () => {
    renderMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Account menu for Ada Lovelace' }))

    fireEvent.click(await screen.findByRole('menuitem', { name: 'My profile' }))
    expect(await screen.findByText('Profile page')).toBeTruthy()
  })
})
