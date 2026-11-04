// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter, Route, Routes } from 'react-router'

// Redux
import { createAppStore } from '../store'
import { meLoaded } from '../store/authSlice'

// User interface
import { ConfirmGate } from '../gates/ConfirmGate'
import { UserMenu } from './UserMenu'

// Misc
import { signOut } from '../auth/signOut'
import { makeUser } from '../testFixtures'
import { TestAriaRouter } from '../testRouter'

// Signing out ends the session and reloads the page, which jsdom cannot do.
vi.mock('../auth/signOut', () => ({
  signOut: vi.fn(),
}))

beforeEach(() => {
  vi.mocked(signOut).mockReset()
})

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
      <TestAriaRouter>
        <ConfirmGate>
          <UserMenu />
          <Routes>
            <Route path='/settings/account' element={<p>My account page</p>} />
            <Route path='*' element={<span />} />
          </Routes>
        </ConfirmGate>
      </TestAriaRouter>
    </MemoryRouter>
  </Provider>)
}

function trigger() {
  // The tooltip's trigger takes the button's name too; the real button is the one to press.
  return screen
    .getAllByRole('button', { name: 'Account menu for Ada Lovelace' })
    .find((element) => element.tagName === 'BUTTON')!
}

async function openMenu() {
  renderMenu()
  fireEvent.click(trigger())
  return screen.findByRole('menu')
}

// The menu follows `~/.claude/docs/ux.md`, Navbars: who is signed in, then My account and
// Security as links, then Sign out below a divider, which confirms first.
describe('UserMenu', () => {
  it('names the person in small bold and their email in smaller italic, then a divider', async () => {
    await openMenu()

    const name = screen.getByText('Ada Lovelace')
    const email = screen.getByText('ada@example.com')
    expect(name.className).toContain('text-sm')
    expect(name.className).toContain('font-bold')
    expect(email.className).toContain('text-xs')
    expect(email.className).toContain('italic')
    expect(name.parentElement?.nextElementSibling?.getAttribute('role')).toBe('separator')
  })

  it('offers My account and Security, then Sign out below a divider, each with its icon', async () => {
    const menu = await openMenu()

    const entries = Array.from(menu.querySelectorAll('[role="menuitem"], [role="separator"]'))
    const labels = entries.map((entry) => entry.getAttribute('role') === 'separator'
      ? 'divider'
      : entry.textContent)
    expect(labels).toEqual([ 'My account', 'Security', 'divider', 'Sign out' ])

    for (const item of within(menu).getAllByRole('menuitem')) {
      expect(item.querySelector('svg')).not.toBeNull()
    }
  })

  it('links to My account and Security, so either opens in a new tab', async () => {
    const menu = await openMenu()

    const account = within(menu).getByRole('menuitem', { name: 'My account' })
    const security = within(menu).getByRole('menuitem', { name: 'Security' })
    expect(account.tagName).toBe('A')
    expect(account.getAttribute('href')).toBe('/settings/account')
    expect(security.tagName).toBe('A')
    expect(security.getAttribute('href')).toBe('/settings/security')

    fireEvent.click(account)
    expect(await screen.findByText('My account page')).toBeTruthy()
  })

  it('signs out only once the dialog is confirmed', async () => {
    const menu = await openMenu()

    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Sign out' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText('Sign out?')).toBeTruthy()
    expect(signOut).not.toHaveBeenCalled()

    fireEvent.click(within(dialog).getByRole('button', { name: 'Sign out' }))
    expect(signOut).toHaveBeenCalledTimes(1)
  })

  it('names the avatar in a tooltip', async () => {
    renderMenu()

    // React Aria opens a tooltip on keyboard focus, not on a programmatic one alone.
    fireEvent.keyDown(document.body, { key: 'Tab' })
    act(() => trigger().focus())
    expect((await screen.findByRole('tooltip')).textContent).toBe('Account')
  })
})
