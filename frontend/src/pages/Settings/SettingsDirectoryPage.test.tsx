// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { meLoaded } from '../../store/authSlice'
import { usersLoaded } from '../../store/usersSlice'

// User interface
import { SettingsDirectoryPage } from './SettingsDirectoryPage'

// Misc
import { makeUser } from '../../testFixtures'
import { TestAriaRouter } from '../../testRouter'

// The users come from Redux; the loader's request is the network boundary.
vi.mock('../../hooks/useServerData', () => ({
  useUsersLoader: () => 'loaded',
}))

function renderDirectory() {
  const admin = makeUser({ id: 'admin', role: 'admin' })
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: admin,
    hasSecondFactor: true,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  store.dispatch(usersLoaded([
    admin,
    makeUser({ id: 'grace', name: 'Grace Hopper', status: 'pending', approvedAt: null }),
  ]))
  return render(<Provider store={store}>
    <MemoryRouter initialEntries={[ '/settings' ]}>
      <TestAriaRouter>
        <SettingsDirectoryPage />
      </TestAriaRouter>
    </MemoryRouter>
  </Provider>)
}

// Issue #180: only each entry's title was clickable, beside a description. Every entry is now
// one tile, its icon and title, all of it a real link, so it opens wherever it is clicked and
// in a new tab too.
describe('SettingsDirectoryPage', () => {
  it('makes every entry one link holding its icon and title, in its section and order', () => {
    renderDirectory()

    const sections = screen
      .getAllByRole('heading', { level: 2 })
      .map((heading) => {
        const tiles = within(heading.parentElement!).getAllByRole('link')
        return {
          section: heading.textContent,
          tiles: tiles.map((tile) => [ tile.textContent, tile.getAttribute('href') ]),
        }
      })

    expect(sections).toEqual([
      {
        section: 'Personal settings',
        tiles: [
          [ 'My account', '/settings/account' ],
          [ 'Security', '/settings/security' ],
          [ 'MCP', '/settings/mcp' ],
        ],
      },
      {
        section: 'Knowledge base',
        tiles: [
          [ 'Meetings', '/settings/meetings' ],
          [ 'Learnings', '/settings/learnings' ],
          [ 'Learning categories', '/settings/learning-categories' ],
        ],
      },
      {
        section: 'Workspace settings',
        tiles: [
          [ 'Users1 waiting', '/settings/users' ],
          [ 'Manage LLMs', '/settings/llms' ],
          [ 'Email', '/settings/email' ],
          [ 'Connectors', '/settings/connectors' ],
          [ 'Storage', '/settings/storage' ],
          [ 'Environment variables', '/settings/environment' ],
          [ 'Passwords', '/settings/passwords' ],
          [ 'Updates', '/settings/updates' ],
          [ 'Backups', '/settings/backups' ],
        ],
      },
    ])

    for (const tile of screen.getAllByRole('link')) {
      expect(tile.tagName).toBe('A')
      expect(tile.querySelector('svg')).not.toBeNull()
    }
  })

  it('shows no descriptions', () => {
    const { container } = renderDirectory()

    expect(container.querySelectorAll('p')).toHaveLength(0)
  })
})
