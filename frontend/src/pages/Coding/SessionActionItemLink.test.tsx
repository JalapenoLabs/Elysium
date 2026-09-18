// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { actionItemUpserted } from '../../store/actionItemsSlice'

// User interface
import { SessionActionItemLink } from './SessionActionItemLink'

// Misc
import { makeActionItem } from '../../testFixtures'

// The item arrives through Redux; the loader's request is the network boundary.
vi.mock('../../hooks/useServerData', () => ({
  useActionItemLoader: () => 'loaded',
}))

function renderLink(actionItemId: string, store = createAppStore()) {
  return render(<Provider store={store}>
    <MemoryRouter>
      <SessionActionItemLink actionItemId={actionItemId} />
    </MemoryRouter>
  </Provider>)
}

describe('SessionActionItemLink', () => {
  it('links to the item the session was started from, by its title', () => {
    const store = createAppStore()
    store.dispatch(actionItemUpserted(makeActionItem({ id: 'login', title: 'Fix the login bug' })))
    renderLink('login', store)

    const link = screen.getByText('From Fix the login bug')
    expect(link.closest('a')?.getAttribute('href')).toBe('/action-items/login')
  })

  it('says so when the item cannot be shown', () => {
    renderLink('missing')

    expect(screen.getByText('From an action item that could not be loaded')).toBeTruthy()
  })
})
