// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// User interface
import { DEFAULT_EXIT_DURATION, toastQueue } from '@heroui/react'

// Misc
import { SLOW_REQUEST_NOTICE_MS } from '../constants'

type Context = {
  fetchWithSlowRequestNotice: typeof import('./slowRequestNotice').fetchWithSlowRequestNotice
  // Answers the requests sent so far, oldest first.
  answer: () => void
}

const slowNotice = 'Your request is taking longer than usual... Hang tight.'

// Each test loads fresh modules, so no notice or count carries over. The stubbed fetch holds
// every request until the test answers it.
beforeEach<Context>(async (context) => {
  vi.useFakeTimers()
  vi.resetModules()
  context.fetchWithSlowRequestNotice = (await import('./slowRequestNotice')).fetchWithSlowRequestNotice

  const pending: Array<() => void> = []
  vi.stubGlobal('fetch', () => new Promise<Response>((resolve) => {
    pending.push(() => resolve(new Response('{}')))
  }))
  context.answer = () => pending.shift()?.()
})

// HeroUI's queue lives in node_modules, which fresh modules do not reach, so it is emptied
// for the next test.
afterEach(() => {
  toastQueue.clear()
  vi.advanceTimersByTime(DEFAULT_EXIT_DURATION)
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

// The notices on screen. A closed one stays mounted while its exit animation plays, so a
// test lets that play out (`DEFAULT_EXIT_DURATION`) before counting what was closed.
function noticesShown() {
  return toastQueue.visibleToasts.filter((shown) => shown.content.title === slowNotice)
}

describe('fetchWithSlowRequestNotice', () => {
  it<Context>('says a slow change is still going until it returns', async (context) => {
    const request = context.fetchWithSlowRequestNotice(new Request('http://localhost/api/v1/items', {
      method: 'POST',
    }))

    await vi.advanceTimersByTimeAsync(SLOW_REQUEST_NOTICE_MS - 1)
    expect(noticesShown()).toHaveLength(0)

    await vi.advanceTimersByTimeAsync(1)
    const [ notice ] = noticesShown()
    expect(notice.content.isLoading).toBe(true)
    expect(notice.timeout).toBe(0)

    context.answer()
    await request
    await vi.advanceTimersByTimeAsync(DEFAULT_EXIT_DURATION)
    expect(noticesShown()).toHaveLength(0)
  })

  it<Context>('closes the notice when the change fails', async (context) => {
    vi.stubGlobal('fetch', () => new Promise<Response>((_resolve, reject) => {
      setTimeout(() => reject(new TypeError('Failed to fetch')), SLOW_REQUEST_NOTICE_MS + 1_000)
    }))
    const request = context.fetchWithSlowRequestNotice('http://localhost/api/v1/items', { method: 'DELETE' })
    const failure = expect(request).rejects.toThrow('Failed to fetch')

    await vi.advanceTimersByTimeAsync(SLOW_REQUEST_NOTICE_MS)
    expect(noticesShown()).toHaveLength(1)

    await vi.advanceTimersByTimeAsync(1_000)
    await failure
    await vi.advanceTimersByTimeAsync(DEFAULT_EXIT_DURATION)
    expect(noticesShown()).toHaveLength(0)
  })

  it<Context>('says nothing about a change that returns in time', async (context) => {
    const request = context.fetchWithSlowRequestNotice('http://localhost/api/v1/items', { method: 'PATCH' })
    await vi.advanceTimersByTimeAsync(SLOW_REQUEST_NOTICE_MS - 1)
    context.answer()
    await request

    await vi.advanceTimersByTimeAsync(SLOW_REQUEST_NOTICE_MS)
    expect(noticesShown()).toHaveLength(0)
  })

  it<Context>('says nothing about a read, however long it takes', async (context) => {
    void context.fetchWithSlowRequestNotice(new Request('http://localhost/api/v1/items'))

    await vi.advanceTimersByTimeAsync(SLOW_REQUEST_NOTICE_MS * 2)
    expect(noticesShown()).toHaveLength(0)
  })

  it<Context>('shows one notice for overlapping changes, until the last one returns', async (context) => {
    const first = context.fetchWithSlowRequestNotice('http://localhost/api/v1/items', { method: 'POST' })
    const second = context.fetchWithSlowRequestNotice('http://localhost/api/v1/items', { method: 'POST' })

    await vi.advanceTimersByTimeAsync(SLOW_REQUEST_NOTICE_MS)
    expect(noticesShown()).toHaveLength(1)

    context.answer()
    await first
    await vi.advanceTimersByTimeAsync(DEFAULT_EXIT_DURATION)
    expect(noticesShown()).toHaveLength(1)

    context.answer()
    await second
    await vi.advanceTimersByTimeAsync(DEFAULT_EXIT_DURATION)
    expect(noticesShown()).toHaveLength(0)
  })
})
