// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { actionItemsLoaded } from '../../store/actionItemsSlice'

// User interface
import { NextEmptyState } from './NextEmptyState'

// Misc
import { makeActionItem } from '../../testFixtures'

const NOW = Date.parse('2026-09-18T12:00:00.000Z')
const LATER = '2026-09-19T12:00:00.000Z'

function renderWith(items: ActionItem[]) {
  const store = createAppStore()
  store.dispatch(actionItemsLoaded(items))
  return render(<Provider store={store}>
    <MemoryRouter>
      <NextEmptyState now={NOW} />
    </MemoryRouter>
  </Provider>)
}

describe('NextEmptyState', () => {
  it('says where the rest of the work is without spelling out a count of zero', () => {
    renderWith([
      makeActionItem({ id: 'first', waitingOn: 'Sam' }),
      makeActionItem({ id: 'second', waitingOn: 'Alex' }),
    ])

    expect(screen.getByText('2 items are waiting on someone.')).toBeTruthy()
    expect(screen.queryByText(/^\d+ items? (is|are) snoozed\.$|Snoozed:/)).toBeNull()
  })

  it('names both when items are waiting and snoozed', () => {
    renderWith([
      makeActionItem({ id: 'waiting', waitingOn: 'Sam' }),
      makeActionItem({ id: 'snoozed', snoozedUntil: LATER }),
    ])

    expect(screen.getByText('Waiting on someone: 1. Snoozed: 1.')).toBeTruthy()
  })

  it('adds nothing when no open item is set aside', () => {
    renderWith([ makeActionItem({ id: 'done', state: 'resolved', waitingOn: 'Sam' }) ])

    expect(screen.queryByText(/^\d+ items? (is|are) (waiting on someone|snoozed)\.$|Snoozed:/)).toBeNull()
  })
})
