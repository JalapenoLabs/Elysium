// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { ToastLink } from '../toast/toast'
import type { ToastVariant } from '../toast/toastPolicy'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Misc
import { NOTIFICATIONS_LIMIT } from '../constants'

// One toast the notification menu keeps: what it said and how it was colored. Its id is the
// toast's own, so a kept record and the toast on screen can be matched.
export type Notification = {
  id: string
  variant: ToastVariant
  title: string
  description: string | null
  link: ToastLink | null
  // When it was shown, as an ISO 8601 UTC timestamp.
  createdAt: string
}

type NotificationsState = {
  // Newest first, at most `NOTIFICATIONS_LIMIT`. Kept for the session only: a reload
  // starts empty.
  items: Notification[]
}

const initialState: NotificationsState = {
  items: [],
}

// Every toast worth finding again lands here, recorded by `src/toast/toast.tsx`. The toast
// module alone decides what is kept, so the notification menu only reads this slice.
export const notificationsSlice = createSlice({
  name: 'notifications',
  initialState,
  reducers: {
    notificationRecorded: {
      reducer(state, action: PayloadAction<Notification>) {
        state.items.unshift(action.payload)
        state.items.splice(NOTIFICATIONS_LIMIT)
      },
      // The time is read while preparing the action, so the reducer stays pure.
      prepare(notification: Omit<Notification, 'createdAt'>) {
        return {
          payload: {
            ...notification,
            createdAt: new Date().toISOString(),
          },
        }
      },
    },
  },
})

export const { notificationRecorded } = notificationsSlice.actions

export function selectNotifications(state: RootState) {
  return state.notifications.items
}
