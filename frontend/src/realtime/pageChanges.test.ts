// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { pageHistoryKey, pageKey, PAGES_KEY, pageSuggestionsKey, projectPagesKey } from '../api/routes/pageRoutes'
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

  it('ends a burst in one load of the list and one of each page, history, and suggestions it named', () => {
    reloadChangedPage('one')
    reloadChangedPage('two')
    reloadChangedPage('one')

    vi.advanceTimersByTime(PAGE_CHANGES_WINDOW_MS)

    expect(mutateMock.mock.calls.map(([ key ]) => key)).toEqual([
      PAGES_KEY,
      expect.any(Function),
      pageKey('one'),
      pageHistoryKey('one'),
      pageSuggestionsKey('one'),
      pageKey('two'),
      pageHistoryKey('two'),
      pageSuggestionsKey('two'),
    ])
  })

  it('loads the docs of whichever project a view holds, and no other key', () => {
    reloadChangedPage('one')

    vi.advanceTimersByTime(PAGE_CHANGES_WINDOW_MS)

    const matchesProjectPages = mutateMock.mock.calls[1][0]
    expect(matchesProjectPages(projectPagesKey('launch'))).toBe(true)
    expect(matchesProjectPages('v1/projects/launch/model-settings')).toBe(false)
    expect(matchesProjectPages('v1/projects')).toBe(false)
    expect(matchesProjectPages(PAGES_KEY)).toBe(false)
  })

  it('starts over for the next burst', () => {
    reloadChangedPage('one')
    vi.advanceTimersByTime(PAGE_CHANGES_WINDOW_MS)
    mutateMock.mockClear()

    reloadChangedPage('two')
    vi.advanceTimersByTime(PAGE_CHANGES_WINDOW_MS)

    expect(mutateMock.mock.calls.map(([ key ]) => key)).toEqual([
      PAGES_KEY,
      expect.any(Function),
      pageKey('two'),
      pageHistoryKey('two'),
      pageSuggestionsKey('two'),
    ])
  })
})
