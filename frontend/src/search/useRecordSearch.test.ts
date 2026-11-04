// Copyright © 2026 Jalapeno Labs

import type { RecordSearchHit } from '../api/routes/searchRoutes'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'

// Misc
import { searchRecords } from '../api/routes/searchRoutes'
import { RECORD_SEARCH_DELAY_MS, RECORD_SEARCH_MAX_CHARACTERS } from '../constants'
import { useRecordSearch } from './useRecordSearch'

vi.mock('../api/routes/searchRoutes', () => ({
  searchRecords: vi.fn(),
}))

// A request the test answers when it chooses, as the API would after a while.
type PendingSearch = {
  query: string
  signal: AbortSignal
  answer: (hits: RecordSearchHit[]) => void
  fail: (error: Error) => void
}

let pending: PendingSearch[] = []

beforeEach(() => {
  vi.useFakeTimers()
  pending = []
  vi.mocked(searchRecords).mockReset()
  vi.mocked(searchRecords).mockImplementation((query, _limit, signal) => new Promise((resolve, reject) => {
    pending.push({
      query,
      signal,
      answer: (hits) => resolve({ results: hits }),
      fail: reject,
    })
  }))
})

afterEach(() => {
  vi.useRealTimers()
})

function renderSearch(query: string, limit = 5) {
  return renderHook(
    (props: { query: string, limit: number }) => useRecordSearch(props.query, props.limit),
    { initialProps: { query, limit }},
  )
}

async function waitOutTheDelay() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(RECORD_SEARCH_DELAY_MS)
  })
}

const project: RecordSearchHit = { kind: 'project', id: 'p1', title: 'Platform' }

describe('useRecordSearch', () => {
  it('asks 300ms after typing stops, and loads from the keystroke until the answer', async () => {
    const { result } = renderSearch('plat')
    expect(result.current.isLoading).toBe(true)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(RECORD_SEARCH_DELAY_MS - 1)
    })
    expect(searchRecords).not.toHaveBeenCalled()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1)
    })
    expect(searchRecords).toHaveBeenCalledWith('plat', 5, expect.any(AbortSignal))
    expect(result.current.isLoading).toBe(true)

    await act(async () => pending[0].answer([ project ]))
    expect(result.current.isLoading).toBe(false)
    expect(result.current.results.map((found) => found.title)).toEqual([ 'Platform' ])
  })

  it('sends one request for a word typed steadily, for what was typed last', async () => {
    const { rerender } = renderSearch('p')
    for (const query of [ 'pl', 'pla', 'plat' ]) {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(RECORD_SEARCH_DELAY_MS / 2)
      })
      rerender({ query, limit: 5 })
    }
    await waitOutTheDelay()

    expect(searchRecords).toHaveBeenCalledTimes(1)
    expect(pending[0].query).toBe('plat')
  })

  it('cancels a request a newer query replaces, and never shows its answer', async () => {
    const { result, rerender } = renderSearch('plat')
    await waitOutTheDelay()
    const older = pending[0]

    rerender({ query: 'platform', limit: 5 })
    expect(older.signal.aborted).toBe(true)
    expect(result.current.isLoading).toBe(true)

    // The older answer arriving late changes nothing; the newer one is what shows.
    await act(async () => older.answer([{ kind: 'project', id: 'old', title: 'Plates' }]))
    expect(result.current.isLoading).toBe(true)

    await waitOutTheDelay()
    await act(async () => pending[1].answer([ project ]))
    expect(result.current.results.map((found) => found.id)).toEqual([ 'records:project:p1' ])
  })

  it('cancels its request when the search closes', async () => {
    const { unmount } = renderSearch('plat')
    await waitOutTheDelay()

    unmount()
    expect(pending[0].signal.aborted).toBe(true)
  })

  it('asks for nothing it would not search: blank, punctuation, or too long', async () => {
    for (const query of [ '   ', '-- ?!', 'a'.repeat(RECORD_SEARCH_MAX_CHARACTERS + 1) ]) {
      const { result } = renderSearch(query)
      expect(result.current).toEqual({ results: [], isLoading: false, hasFailed: false })
    }
    await waitOutTheDelay()
    expect(searchRecords).not.toHaveBeenCalled()
  })

  it('says it could not answer when the request fails, and stops loading', async () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => {})
    const { result } = renderSearch('plat')
    await waitOutTheDelay()

    await act(async () => pending[0].fail(new Error('Too many searches')))
    expect(result.current).toEqual({ results: [], isLoading: false, hasFailed: true })
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })

  it('asks again when the page wants more of each kind', async () => {
    const { rerender } = renderSearch('plat')
    await waitOutTheDelay()
    await act(async () => pending[0].answer([ project ]))

    rerender({ query: 'plat', limit: 50 })
    await waitOutTheDelay()
    expect(searchRecords).toHaveBeenLastCalledWith('plat', 50, expect.any(AbortSignal))
  })
})
