// Copyright © 2026 Jalapeno Labs

// Misc
import {
  TOAST_BASE_DURATION_MS,
  TOAST_DURATION_PER_WORD_MS,
  TOAST_WITH_ACTION_MIN_DURATION_MS,
} from '../constants'

// How a toast is colored, which also decides how long it stays and whether it is kept.
export type ToastVariant = 'default' | 'success' | 'warning' | 'danger'

// Warnings and errors ask something of the person, so they stay until dismissed rather than
// disappearing before they are read.
const isStickyByVariant = {
  default: false,
  success: false,
  warning: true,
  danger: true,
} as const satisfies Record<ToastVariant, boolean>

// Whether the notification menu keeps a toast, unless its caller says. A success confirms
// what the person just did ("Saved", "Copied"); keeping every one of those would bury the
// notifications worth finding again, so only the rest are kept.
export const isKeptByVariant = {
  default: true,
  success: false,
  warning: true,
  danger: true,
} as const satisfies Record<ToastVariant, boolean>

// How long a toast stays up, in milliseconds, with 0 meaning until dismissed. `texts` is
// everything it shows: its message, description, and action label.
export function toastTimeoutMs(variant: ToastVariant, texts: string[], hasAction: boolean) {
  if (isStickyByVariant[variant]) {
    return 0
  }

  let wordCount = 0
  for (const text of texts) {
    wordCount += text.split(/\s+/).filter(Boolean).length
  }

  const readingTime = TOAST_BASE_DURATION_MS + wordCount * TOAST_DURATION_PER_WORD_MS
  if (hasAction) {
    return Math.max(readingTime, TOAST_WITH_ACTION_MIN_DURATION_MS)
  }
  return readingTime
}

