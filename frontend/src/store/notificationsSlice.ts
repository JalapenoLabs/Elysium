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
  // Whether the person has seen it in the notification drawer. Unread ones count on the bell.
  isRead: boolean
}

type NotificationsState = {
  // Newest first, at most `NOTIFICATIONS_LIMIT`. Kept in memory for the page's life alone, as
  // every other slice is: a reload starts empty, and signing out (a full page load) leaves
  // nothing for whoever signs in next.
  items: Notification[]
}

const initialState: NotificationsState = {
  items: [],
}

// Every toast worth finding again lands here, recorded by `src/toast/toast.tsx`. The toast
// module alone decides what is kept; the notification drawer reads it, marks it read, and
// clears it.
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
      prepare(notification: Omit<Notification, 'createdAt' | 'isRead'>) {
        return {
          payload: {
            ...notification,
            createdAt: new Date().toISOString(),
            isRead: false,
          },
        }
      },
    },
    // The drawer is open, so everything in it has been seen.
    notificationsRead(state) {
      for (const notification of state.items) {
        notification.isRead = true
      }
    },
    notificationCleared(state, action: PayloadAction<string>) {
      state.items = state.items.filter((notification) => notification.id !== action.payload)
    },
    notificationsCleared(state) {
      state.items = []
    },
  },
})

export const {
  notificationRecorded,
  notificationsRead,
  notificationCleared,
  notificationsCleared,
} = notificationsSlice.actions

export function selectNotifications(state: RootState) {
  return state.notifications.items
}

// How many the bell counts. A number, so selecting it never re-renders on an equal count.
export function selectUnreadNotificationCount(state: RootState) {
  let unreadCount = 0
  for (const notification of state.notifications.items) {
    if (!notification.isRead) {
      unreadCount += 1
    }
  }
  return unreadCount
}
