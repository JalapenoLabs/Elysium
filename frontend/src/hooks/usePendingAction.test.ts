// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { usePendingAction } from './usePendingAction'

// A promise the test settles by hand, standing in for a request in flight.
function deferred() {
  let resolve: () => void = () => undefined
  let reject: (reason: unknown) => void = () => undefined
  const promise = new Promise<void>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

describe('usePendingAction', () => {
  it('names the action in flight until it settles', async () => {
    const { result } = renderHook(() => usePendingAction<'resolve' | 'dismiss'>())
    const request = deferred()

    let running: Promise<void> = Promise.resolve()
    act(() => {
      running = result.current.run('resolve', () => request.promise)
    })
    expect(result.current.pendingKey).toBe('resolve')
    expect(result.current.isAnyPending).toBe(true)
    // The running one shows pending; only the others are disabled.
    expect(result.current.isOtherPending('resolve')).toBe(false)
    expect(result.current.isOtherPending('dismiss')).toBe(true)

    await act(async () => {
      request.resolve()
      await running
    })
    expect(result.current.pendingKey).toBeNull()
    expect(result.current.isAnyPending).toBe(false)
    expect(result.current.isOtherPending('dismiss')).toBe(false)
  })

  it('ignores a second action while one runs, even within one render', async () => {
    const { result } = renderHook(() => usePendingAction<'resolve' | 'dismiss'>())
    const request = deferred()
    const second = vi.fn(() => Promise.resolve())

    let running: Promise<void> = Promise.resolve()
    act(() => {
      running = result.current.run('resolve', () => request.promise)
      // The same render's `run`, as a double click would call it.
      void result.current.run('dismiss', second)
    })
    expect(second).not.toHaveBeenCalled()
    expect(result.current.pendingKey).toBe('resolve')

    await act(async () => {
      request.resolve()
      await running
    })
  })

  it('clears the pending action when it fails, and passes the failure on', async () => {
    const { result } = renderHook(() => usePendingAction<'end'>())
    const failure = new Error('refused')

    await act(async () => {
      await expect(result.current.run('end', () => Promise.reject(failure))).rejects.toBe(failure)
    })
    expect(result.current.pendingKey).toBeNull()
  })
})
