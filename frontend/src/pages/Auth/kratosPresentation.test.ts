// Copyright © 2026 Jalapeno Labs

import type { UiText } from '@ory/client-fetch'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { describeKratosMessage, fieldErrorText, flowErrorText, kratosMessageStatus } from './kratosPresentation'

function message(id: number, text: string, type: UiText['type'] = 'error'): UiText {
  return { id, text, type }
}

describe('describeKratosMessage', () => {
  it('translates the messages Elysium sends through Kratos', () => {
    expect(describeKratosMessage(message(4190001, 'Sign-up is closed.'))).toEqual({
      key: 'messages.signupClosed',
      text: null,
    })
    expect(describeKratosMessage(message(4190002, 'Sign up with a password.')).key).toBe('messages.passwordRequired')
  })

  it('shows the messages Kratos writes itself as sent', () => {
    expect(describeKratosMessage(message(4000006, 'The provided credentials are invalid.'))).toEqual({
      key: null,
      text: 'The provided credentials are invalid.',
    })
  })
})

describe('kratosMessageStatus', () => {
  it('colors errors, successes, and information apart', () => {
    expect(kratosMessageStatus(message(1, 'x', 'error'))).toBe('danger')
    expect(kratosMessageStatus(message(1, 'x', 'success'))).toBe('success')
    expect(kratosMessageStatus(message(1, 'x', 'info'))).toBe('default')
  })
})

describe('fieldErrorText', () => {
  it('joins every message on a field, translating Elysium’s', () => {
    const text = fieldErrorText(
      [ message(4190001, 'Sign-up is closed.'), message(4000007, 'An account exists.') ],
      (key) => `[${key}]`,
    )
    expect(text).toBe('[messages.signupClosed] An account exists.')
  })

  it('says nothing for a field without messages', () => {
    expect(fieldErrorText(undefined, (key) => key)).toBeUndefined()
    expect(fieldErrorText([], (key) => key)).toBeUndefined()
  })
})

describe('flowErrorText', () => {
  it('prefers the reason, then the message', () => {
    const error = { message: 'Bad request', reason: 'The link was already used.' }
    expect(flowErrorText(error)).toBe('The link was already used.')
    expect(flowErrorText({ message: 'Bad request' })).toBe('Bad request')
  })

  it('has nothing to say for an error without either', () => {
    expect(flowErrorText({ code: 400 })).toBeNull()
    expect(flowErrorText(undefined)).toBeNull()
  })
})
