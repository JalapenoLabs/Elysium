// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  TOAST_BASE_DURATION_MS,
  TOAST_DURATION_PER_WORD_MS,
  TOAST_WITH_ACTION_MIN_DURATION_MS,
} from '../constants'
import { isKeptByVariant, toastTimeoutMs } from './toastPolicy'

describe('toastTimeoutMs', () => {
  it('keeps warnings and errors up until they are dismissed', () => {
    expect(toastTimeoutMs('warning', [ 'The page was open too long' ], false)).toBe(0)
    expect(toastTimeoutMs('danger', [ 'Something went wrong' ], true)).toBe(0)
  })

  it('gives every word shown 300ms on top of a base second', () => {
    expect(toastTimeoutMs('success', [ 'Project saved' ], false))
      .toBe(TOAST_BASE_DURATION_MS + 2 * TOAST_DURATION_PER_WORD_MS)
    expect(toastTimeoutMs('default', [ 'Mailbox connected', 'It reads new mail every minute' ], false))
      .toBe(TOAST_BASE_DURATION_MS + 8 * TOAST_DURATION_PER_WORD_MS)
  })

  it('counts words, not the spaces or line breaks between them', () => {
    expect(toastTimeoutMs('success', [ '  Saved \n\n now  ', '' ], false))
      .toBe(TOAST_BASE_DURATION_MS + 2 * TOAST_DURATION_PER_WORD_MS)
  })

  it('leaves a toast with an action up long enough to reach the button', () => {
    expect(toastTimeoutMs('success', [ 'Learning deleted', 'Undo' ], true))
      .toBe(TOAST_WITH_ACTION_MIN_DURATION_MS)

    const longMessage = Array.from({ length: 30 }, () => 'word').join(' ')
    expect(toastTimeoutMs('success', [ longMessage, 'Undo' ], true))
      .toBe(TOAST_BASE_DURATION_MS + 31 * TOAST_DURATION_PER_WORD_MS)
  })
})

describe('isKeptByVariant', () => {
  it('keeps everything but confirmations of what the person just did', () => {
    expect(isKeptByVariant).toEqual({
      default: true,
      success: false,
      warning: true,
      danger: true,
    })
  })
})
