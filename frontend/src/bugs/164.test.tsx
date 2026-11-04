// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render } from '@testing-library/react'

// Misc
import { DEFAULT_EXIT_DURATION } from '@heroui/react'

// Issue #164: every warning and error toast vanished after HeroUI's default 4 seconds, often
// before it was read, and every success lasted 4 seconds however long it was. Toasts now go
// through `src/toast/toast.ts`: warnings and errors stay until dismissed, and the rest stay
// about 300ms per word plus a second.
//
// A toast's countdown starts when the toast region shows it, so each test renders the region
// the app mounts, as `AppShell` does.

type Context = {
  toast: typeof import('../toast/toast').toast
  queue: typeof import('@heroui/react').toastQueue
}

// jsdom has no ResizeObserver; the toast region measures each toast with one.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach<Context>(async (context) => {
  vi.stubGlobal('ResizeObserver', InertObserver)
  vi.useFakeTimers()
  vi.resetModules()
  context.toast = (await import('../toast/toast')).toast
  const heroUi = await import('@heroui/react')
  context.queue = heroUi.toastQueue
  render(<heroUi.Toast.Provider />)
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

function isOnScreen(context: Context, toastId: string) {
  return context.queue.visibleToasts.some((shown) => shown.key === toastId)
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
  it<Context>('keeps warnings and errors up until they are dismissed', (context) => {
    const errorId = show(() => context.toast.danger('Could not save the credential'))
    const warningId = show(() => context.toast.warning('The page was open too long, so it reloaded'))

    wait(60 * 60 * 1_000)
    expect(isOnScreen(context, errorId)).toBe(true)
    expect(isOnScreen(context, warningId)).toBe(true)

    act(() => context.toast.close(errorId))
    wait(DEFAULT_EXIT_DURATION)
    expect(isOnScreen(context, errorId)).toBe(false)
  })

  it<Context>('times a success out by its length rather than a fixed 4 seconds', (context) => {
    // Three words: one second, plus 300ms each.
    const shortId = show(() => context.toast.success('Credential saved here'))
    // Fourteen words: 5.2 seconds.
    const longId = show(() => context.toast.success(
      'Mailbox connected. Elysium reads it every minute and tells you when new mail arrives.',
    ))

    wait(1_900 + DEFAULT_EXIT_DURATION)
    expect(isOnScreen(context, shortId)).toBe(false)
    expect(isOnScreen(context, longId)).toBe(true)

    wait(5_200 - 1_900)
    expect(isOnScreen(context, longId)).toBe(false)
  })
})
