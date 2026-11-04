// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, screen } from '@testing-library/react'

// User interface
import { AuthenticatorCodeInput } from './AuthenticatorCodeInput'

// Once focused, `input-otp` hit-tests the field from timers (0, 2, and 5 seconds, then 6 to
// stop) with `document.elementFromPoint`, which jsdom lacks and `testSetup.ts` fills in. Run
// under fake timers, those checks throw inside this test if that ever goes missing, rather than
// after every test has passed, where Vitest still fails the run.

// jsdom has no ResizeObserver; `input-otp` watches the field's size with one.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

describe('AuthenticatorCodeInput', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('takes focus when asked, and outlives the password manager checks that focus starts', () => {
    render(
      <AuthenticatorCodeInput
        id='authenticator-code'
        label='Authenticator code'
        value=''
        onChange={() => {}}
        shouldFocus
      />,
    )

    const field = screen.getByRole('textbox', { name: 'Authenticator code' })
    expect(document.activeElement).toBe(field)

    // Past the last of the checks, at six seconds.
    act(() => {
      vi.advanceTimersByTime(6_000)
    })

    expect(document.activeElement).toBe(field)
  })
})
