// Copyright © 2026 Jalapeno Labs

import type { UiText } from '@ory/client-fetch'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useKratosMessageToasts } from './useKratosMessageToasts'

// User interface
import { DEFAULT_EXIT_DURATION, toastQueue } from '@heroui/react'

function message(id: number, text: string, type: UiText['type']): UiText {
  return { id, text, type }
}

// Timers are faked so a replaced toast's exit animation can be played to its end.
beforeEach(() => {
  vi.useFakeTimers()
})

// HeroUI's queue outlives the test, so it is emptied for the next one.
afterEach(() => {
  toastQueue.clear()
  vi.advanceTimersByTime(DEFAULT_EXIT_DURATION)
  vi.useRealTimers()
})

// The toasts on screen, newest first, once any closed one has played its exit animation.
function shownToasts() {
  vi.advanceTimersByTime(DEFAULT_EXIT_DURATION)
  return toastQueue.visibleToasts.map((shown) => ({
    title: shown.content.title,
    variant: shown.content.variant,
  }))
}

describe('useKratosMessageToasts', () => {
  it('shows errors as danger toasts and information as plain ones', () => {
    const { result } = renderHook(() => useKratosMessageToasts())

    act(() => result.current([
      message(4000006, 'The provided credentials are invalid.', 'error'),
      message(1010003, 'Please confirm this action by verifying that it is you.', 'info'),
    ]))

    expect(shownToasts()).toEqual([
      { title: 'Please confirm this action by verifying that it is you.', variant: 'default' },
      { title: 'The provided credentials are invalid.', variant: 'danger' },
    ])
  })

  it('translates the messages Elysium sends through Kratos', () => {
    const { result } = renderHook(() => useKratosMessageToasts())

    act(() => result.current([ message(4190001, 'Sign-up is closed.', 'error') ]))

    expect(shownToasts()).toEqual([
      { title: 'Sign-up is closed. Ask an admin to open it.', variant: 'danger' },
    ])
  })

  it('leaves out the success Kratos reports after every save', () => {
    const { result } = renderHook(() => useKratosMessageToasts())

    act(() => result.current([ message(1050001, 'Your changes have been saved!', 'success') ]))

    expect(shownToasts()).toEqual([])
  })

  it('replaces the last refusal rather than stacking another copy', () => {
    const { result } = renderHook(() => useKratosMessageToasts())
    const refusal = message(4000006, 'The provided credentials are invalid.', 'error')

    act(() => result.current([ refusal ]))
    act(() => result.current([ refusal ]))

    expect(shownToasts()).toHaveLength(1)

    act(() => result.current([]))
    expect(shownToasts()).toEqual([])
  })
})
