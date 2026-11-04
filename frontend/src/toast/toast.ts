// Copyright © 2026 Jalapeno Labs

import type { ToastVariant } from './toastPolicy'

// Redux
import { store } from '../store'
import { notificationRecorded } from '../store/notificationsSlice'

// User interface
// The one import of HeroUI's toast function: everything else shows toasts through this
// module, which ESLint enforces (`no-restricted-imports` in eslint.config.ts).
import { toast as heroToast } from '@heroui/react'

// Misc
import { isKeptByVariant, toastTimeoutMs } from './toastPolicy'

// One button beside a toast's message, such as Undo. Pressing it also closes the toast.
export type ToastAction = {
  label: string
  onPress: () => void
}

export type ToastOptions = {
  description?: string
  action?: ToastAction
  // Whether the notification menu keeps this toast. Unset, it keeps everything but a
  // success (see `isKeptByVariant`).
  keepInNotifications?: boolean
}

function show(variant: ToastVariant, message: string, options: ToastOptions = {}) {
  const { description, action } = options

  const texts = [ message ]
  if (description) {
    texts.push(description)
  }
  if (action) {
    texts.push(action.label)
  }

  // The action reads the id only when pressed, by which time the toast exists.
  const toastId = heroToast(message, {
    variant,
    description,
    timeout: toastTimeoutMs(variant, texts, Boolean(action)),
    actionProps: action && {
      children: action.label,
      onPress: () => {
        heroToast.close(toastId)
        action.onPress()
      },
    },
  })

  if (options.keepInNotifications ?? isKeptByVariant[variant]) {
    store.dispatch(notificationRecorded({
      id: toastId,
      variant,
      title: message,
      description: description ?? null,
    }))
  }

  return toastId
}

// Elysium's toasts. Each returns the toast's id, for `toast.close`.
//
// - Warnings and errors stay until the person dismisses them.
// - Everything else stays about 300ms per word it shows, plus a second, and longer when it
//   carries an action, so there is time to reach the button.
// - Every toast but a success is kept for the notification menu (`notificationsSlice`).
export const toast = Object.assign(
  (message: string, options?: ToastOptions) => show('default', message, options),
  {
    success: (message: string, options?: ToastOptions) => show('success', message, options),
    warning: (message: string, options?: ToastOptions) => show('warning', message, options),
    danger: (message: string, options?: ToastOptions) => show('danger', message, options),
    // Work in progress, with a spinner. It stays until closed with `toast.close`, and the
    // notification menu never keeps it, since it says nothing once the work ends.
    loading: (message: string) => heroToast(message, { isLoading: true, timeout: 0 }),
    close: (toastId: string) => heroToast.close(toastId),
  },
)
