// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render } from '@testing-library/react'

// User interface
import { DEFAULT_EXIT_DURATION, Toast, toastQueue } from '@heroui/react'

// Misc
import { toast } from '../toast/toast'

// Issue #164: every warning and error toast vanished after HeroUI's default 4 seconds, often
// before it was read, and every success lasted 4 seconds however long it was. Toasts now go
// through `src/toast/toast.ts`: warnings and errors stay until dismissed, and the rest stay
// about 300ms per word plus a second.
//
// A toast's countdown starts when the toast region shows it, so each test renders the region
// the app mounts, as `AppShell` does.

// jsdom has no ResizeObserver; the toast region measures each toast with one.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', InertObserver)
  vi.useFakeTimers()
  render(<Toast.Provider />)
})

// HeroUI's queue outlives the test, so it is emptied for the next one.
afterEach(() => {
  act(() => {
    toastQueue.clear()
    vi.advanceTimersByTime(DEFAULT_EXIT_DURATION)
  })
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

function isOnScreen(toastId: string) {
  return toastQueue.visibleToasts.some((shown) => shown.key === toastId)
}

// Shows a toast inside `act`, so the region renders it and starts its countdown.
function show(showToast: () => string) {
  let toastId = ''
  act(() => {
    toastId = showToast()
  })
  return toastId
}

function wait(milliseconds: number) {
  act(() => {
    vi.advanceTimersByTime(milliseconds)
  })
}

describe('issue #164', () => {
  it('keeps warnings and errors up until they are dismissed', () => {
    const errorId = show(() => toast.danger('Could not save the credential'))
    const warningId = show(() => toast.warning('The page was open too long, so it reloaded'))

    wait(60 * 60 * 1_000)
    expect(isOnScreen(errorId)).toBe(true)
    expect(isOnScreen(warningId)).toBe(true)

    act(() => toast.close(errorId))
    wait(DEFAULT_EXIT_DURATION)
    expect(isOnScreen(errorId)).toBe(false)
  })

  it('times a success out by its length rather than a fixed 4 seconds', () => {
    // Three words: one second, plus 300ms each.
    const shortId = show(() => toast.success('Credential saved here'))
    // Fourteen words: 5.2 seconds.
    const longId = show(() => toast.success(
      'Mailbox connected. Elysium reads it every minute and tells you when new mail arrives.',
    ))

    wait(1_900 + DEFAULT_EXIT_DURATION)
    expect(isOnScreen(shortId)).toBe(false)
    expect(isOnScreen(longId)).toBe(true)

    wait(5_200 - 1_900)
    expect(isOnScreen(longId)).toBe(false)
  })
})
