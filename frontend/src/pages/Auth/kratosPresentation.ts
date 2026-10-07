// Copyright © 2026 Jalapeno Labs

import type { UiText } from '@ory/client-fetch'
import type { ParseKeys } from 'i18next'

// Messages Elysium's own sign-up hook sends through Kratos (api/src/routes/internal/
// registration.rs), translated here. Every other Kratos message shows Kratos's English text.
const elysiumMessageKeys = new Map<number, ParseKeys<'auth'>>([
  [ 4_190_001, 'messages.signupClosed' ],
  [ 4_190_002, 'messages.passwordRequired' ],
])

// What to show for one Kratos message: a translation key for Elysium's own, or Kratos's
// text as sent.
export function describeKratosMessage(message: UiText) {
  const key = elysiumMessageKeys.get(message.id)
  if (key) {
    return { key, text: null } as const
  }
  return { key: null, text: message.text } as const
}

// Kratos marks each message as an error, a success, or plain information.
export function kratosMessageStatus(message: UiText) {
  if (message.type === 'error') {
    return 'danger'
  }
  if (message.type === 'success') {
    return 'success'
  }
  return 'default'
}

// The messages Kratos put on one field, as the text its error line shows, or undefined for
// none. `translate` resolves Elysium's own messages.
export function fieldErrorText(
  messages: UiText[] | undefined,
  translate: (key: ParseKeys<'auth'>) => string,
): string | undefined {
  if (!messages?.length) {
    return undefined
  }

  const texts: string[] = []
  for (const message of messages) {
    const described = describeKratosMessage(message)
    texts.push(
      described.key
        ? translate(described.key)
        : described.text,
    )
  }
  return texts.join(' ')
}

// The explanation inside a Kratos flow error (`reason`, else `message`), if it carries one.
export function flowErrorText(error: object | undefined): string | null {
  if (!error) {
    return null
  }
  const reason = 'reason' in error && typeof error.reason === 'string'
    ? error.reason
    : null
  const message = 'message' in error && typeof error.message === 'string'
    ? error.message
    : null
  return reason ?? message
}
