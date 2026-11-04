// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  notificationCleared,
  notificationRecorded,
  notificationsCleared,
  notificationsRead,
  selectNotifications,
  selectUnreadNotificationCount,
} from './notificationsSlice'

// Misc
import { NOTIFICATIONS_LIMIT } from '../constants'

function record(store: ReturnType<typeof createAppStore>, id: string) {
  store.dispatch(notificationRecorded({
    id,
    variant: 'danger',
    title: `Notification ${id}`,
    description: null,
    link: null,
  }))
}

describe('notificationRecorded', () => {
  it('keeps the newest first, unread, with when it was shown', () => {
    const store = createAppStore()
    record(store, 'first')
    record(store, 'second')

    const notifications = selectNotifications(store.getState())
    expect(notifications.map((notification) => notification.id)).toEqual([ 'second', 'first' ])
    expect(notifications[0].isRead).toBe(false)
    expect(Number.isNaN(Date.parse(notifications[0].createdAt))).toBe(false)
  })

  it('drops the oldest past the limit', () => {
    const store = createAppStore()
    for (let index = 0; index <= NOTIFICATIONS_LIMIT; index += 1) {
      record(store, String(index))
    }

    const notifications = selectNotifications(store.getState())
    expect(notifications).toHaveLength(NOTIFICATIONS_LIMIT)
    expect(notifications.at(-1)?.id).toBe('1')
    expect(selectUnreadNotificationCount(store.getState())).toBe(NOTIFICATIONS_LIMIT)
  })
})

describe('notificationsRead', () => {
  it('reads everything kept, and counts only what arrives after', () => {
    const store = createAppStore()
    record(store, 'first')
    record(store, 'second')
    expect(selectUnreadNotificationCount(store.getState())).toBe(2)

    store.dispatch(notificationsRead())
    expect(selectUnreadNotificationCount(store.getState())).toBe(0)

    record(store, 'third')
    expect(selectUnreadNotificationCount(store.getState())).toBe(1)
  })
})

describe('clearing', () => {
  it('clears one by its id and leaves the rest', () => {
    const store = createAppStore()
    record(store, 'first')
    record(store, 'second')

    store.dispatch(notificationCleared('first'))
    const notifications = selectNotifications(store.getState())
    expect(notifications.map((notification) => notification.id)).toEqual([ 'second' ])
  })

  it('clears all of them at once', () => {
    const store = createAppStore()
    record(store, 'first')
    record(store, 'second')

    store.dispatch(notificationsCleared())
    expect(selectNotifications(store.getState())).toEqual([])
    expect(selectUnreadNotificationCount(store.getState())).toBe(0)
  })
})
