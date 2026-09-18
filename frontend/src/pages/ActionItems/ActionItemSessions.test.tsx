// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { codingSessionsLoaded } from '../../store/codingSessionsSlice'

// User interface
import { ActionItemSessions } from './ActionItemSessions'

// Misc
import { makeCodingSession } from '../../testFixtures'

// Sessions arrive through Redux; the loaders' requests are the network boundary.
vi.mock('../../hooks/useServerData', () => ({
  useCodingSessionsLoader: () => 'loaded',
  useSatellitesLoader: () => 'loaded',
}))

function renderFor(itemId: string, sessions: CodingSession[]) {
  const store = createAppStore()
  store.dispatch(codingSessionsLoaded(sessions))
  return render(<Provider store={store}>
    <MemoryRouter>
      <ActionItemSessions itemId={itemId} />
    </MemoryRouter>
  </Provider>)
}

describe('ActionItemSessions', () => {
  it('lists the sessions started from the item, each linking to its conversation', () => {
    renderFor('login', [
      makeCodingSession({ id: 3, actionItemId: 'login', title: 'Fix the cookie' }),
      makeCodingSession({ id: 4, actionItemId: 'docs', title: 'Write the docs' }),
    ])

    const link = screen.getByText('Fix the cookie')
    expect(link.closest('a')?.getAttribute('href')).toBe('/coding/3')
    expect(screen.queryByText('Write the docs')).toBeNull()
  })

  it('says none was started yet', () => {
    renderFor('login', [ makeCodingSession({ id: 4, actionItemId: 'docs' }) ])

    expect(screen.getByText('No coding session was started from this item yet.')).toBeTruthy()
  })
})
