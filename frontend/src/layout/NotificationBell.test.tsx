// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../store'
import { notificationRecorded, selectUnreadNotificationCount } from '../store/notificationsSlice'

// User interface
import { NotificationBell } from './NotificationBell'

function renderBell() {
  const store = createAppStore()
  render(<Provider store={store}>
    <NotificationBell />
  </Provider>)
  return store
}

function record(store: ReturnType<typeof createAppStore>, id: string, title: string) {
  act(() => {
    store.dispatch(notificationRecorded({
      id,
      variant: 'danger',
      title,
      description: null,
      link: null,
    }))
  })
}

// The tooltip's trigger takes the button's name too; the real button is the one to press.
function bell(name: string | RegExp) {
  return screen
    .getAllByRole('button', { name })
    .find((element) => element.tagName === 'BUTTON')!
}

// The bell follows `~/.claude/docs/ux.md`, Notification menus: a count of what is unread, a
// drawer listing everything newest first, each clearable, Clear all, and opening it reads them.
describe('NotificationBell', () => {
  it('counts unread notifications in its badge and its name', () => {
    const store = renderBell()
    expect(bell('Notifications')).toBeTruthy()

    record(store, 'first', 'Could not save')
    record(store, 'second', 'Mailbox disconnected')

    expect(bell('Notifications, 2 unread')).toBeTruthy()
    expect(screen.getByText('2')).toBeTruthy()
  })

  it('lists newest first and reads them all as it opens, including what arrives while open', async () => {
    const store = renderBell()
    record(store, 'first', 'Could not save')
    record(store, 'second', 'Mailbox disconnected')

    fireEvent.click(bell(/Notifications/))
    const list = await screen.findByRole('list', { name: 'Notifications' })
    const titles = within(list)
      .getAllByRole('listitem')
      .map((item) => item.querySelector('p')?.textContent)
    expect(titles).toEqual([ 'Mailbox disconnected', 'Could not save' ])
    expect(selectUnreadNotificationCount(store.getState())).toBe(0)

    record(store, 'third', 'Backup failed')
    expect(selectUnreadNotificationCount(store.getState())).toBe(0)
  })

  it('clears one, then all', async () => {
    const store = renderBell()
    record(store, 'first', 'Could not save')
    record(store, 'second', 'Mailbox disconnected')

    fireEvent.click(bell(/Notifications/))
    const list = await screen.findByRole('list', { name: 'Notifications' })
    fireEvent.click(within(list).getAllByRole('button', { name: 'Clear Could not save' })
      .find((element) => element.tagName === 'BUTTON')!)
    expect(within(list).getAllByRole('listitem')).toHaveLength(1)

    fireEvent.click(screen.getByRole('button', { name: 'Clear all' }))
    expect(await screen.findByText('You\'re all caught up.')).toBeTruthy()
    expect(store.getState().notifications.items).toEqual([])
    expect(screen.getByRole('button', { name: 'Clear all' })).toHaveProperty('disabled', true)
  })
})
