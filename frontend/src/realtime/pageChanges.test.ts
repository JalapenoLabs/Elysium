// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { pageHistoryKey, pageKey, PAGES_KEY } from '../api/routes/pageRoutes'
import { PAGE_CHANGES_WINDOW_MS } from '../constants'
import { reloadChangedPage } from './pageChanges'

const { mutateMock } = vi.hoisted(() => ({ mutateMock: vi.fn() }))

vi.mock('swr', () => ({ mutate: mutateMock }))

describe('reloadChangedPage', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mutateMock.mockClear()
  })

  // Ends any burst a test left open, since the module keeps its timer between tests.
  afterEach(() => {
    vi.runOnlyPendingTimers()
    vi.useRealTimers()
  })

  it('waits for a burst to end before it loads anything', () => {
    reloadChangedPage('one')

    expect(mutateMock).not.toHaveBeenCalled()
  })

  it('ends a burst in one load of the list and one of each page and history it named', () => {
    reloadChangedPage('one')
    reloadChangedPage('two')
    reloadChangedPage('one')

    vi.advanceTimersByTime(PAGE_CHANGES_WINDOW_MS)

    expect(mutateMock.mock.calls.map(([ key ]) => key)).toEqual([
      PAGES_KEY,
      pageKey('one'),
      pageHistoryKey('one'),
      pageKey('two'),
      pageHistoryKey('two'),
    ])
  })

  it('starts over for the next burst', () => {
    reloadChangedPage('one')
    vi.advanceTimersByTime(PAGE_CHANGES_WINDOW_MS)
    mutateMock.mockClear()

    reloadChangedPage('two')
    vi.advanceTimersByTime(PAGE_CHANGES_WINDOW_MS)

    expect(mutateMock.mock.calls.map(([ key ]) => key)).toEqual([
      PAGES_KEY,
      pageKey('two'),
      pageHistoryKey('two'),
    ])
  })
})
