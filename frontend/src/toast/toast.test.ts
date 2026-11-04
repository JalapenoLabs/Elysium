// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// User interface
import { DEFAULT_EXIT_DURATION, toastQueue } from '@heroui/react'

type Context = {
  toast: typeof import('./toast').toast
  store: typeof import('../store').store
}

// Each test loads fresh modules, so it starts with an empty store. Timers are faked so a closed
// toast's exit animation can be run to its end.
beforeEach<Context>(async (context) => {
  vi.useFakeTimers()
  vi.resetModules()
  context.toast = (await import('./toast')).toast
  context.store = (await import('../store')).store
})

// HeroUI's queue lives in node_modules, which fresh modules do not reach, so it is emptied
// for the next test.
afterEach(() => {
  toastQueue.clear()
  vi.advanceTimersByTime(DEFAULT_EXIT_DURATION)
  vi.useRealTimers()
})

// Whether a toast is still on screen once any exit animation it started has played.
function isOnScreen(toastId: string) {
  vi.advanceTimersByTime(DEFAULT_EXIT_DURATION)
  return toastQueue.visibleToasts.some((candidate) => candidate.key === toastId)
}

function findShown(toastId: string) {
  const shown = toastQueue.visibleToasts.find((candidate) => candidate.key === toastId)
  if (!shown) {
    throw new Error(`Toast ${toastId} is not on screen`)
  }
  return shown
}

describe('toast', () => {
  it<Context>('shows the message and description in the variant asked for', (context) => {
    const toastId = context.toast.danger('Could not save', { description: 'The name is taken' })

    const shown = findShown(toastId)
    expect(shown.content).toMatchObject({
      title: 'Could not save',
      description: 'The name is taken',
      variant: 'danger',
    })
    expect(shown.timeout).toBe(0)
  })

  it<Context>('times a success out by its length', (context) => {
    const toastId = context.toast.success('Project saved')

    expect(findShown(toastId).timeout).toBe(1_600)
  })

  it<Context>('closes the toast when its action is pressed, then runs the action', (context) => {
    const onPress = vi.fn()
    const toastId = context.toast.success('Learning deleted', { action: { label: 'Undo', onPress }})

    const shown = findShown(toastId)
    expect(shown.content.actionProps?.children).toBe('Undo')
    shown.content.actionProps?.onPress?.({} as never)

    expect(onPress).toHaveBeenCalledOnce()
    expect(isOnScreen(toastId)).toBe(false)
  })

  it<Context>('keeps every toast but a success for the notification menu', (context) => {
    const savedId = context.toast.success('Saved')
    const failedId = context.toast.danger('Could not reach Jira', { description: 'Check the site' })
    const noticeId = context.toast('Your changes have been saved!')

    const kept = context.store.getState().notifications.items
    expect(kept.map((notification) => notification.id)).toEqual([ noticeId, failedId ])
    expect(kept).not.toContainEqual(expect.objectContaining({ id: savedId }))
    expect(kept[1]).toMatchObject({
      variant: 'danger',
      title: 'Could not reach Jira',
      description: 'Check the site',
    })
    expect(Date.parse(kept[1].createdAt)).not.toBeNaN()
  })

  it<Context>('lets the caller decide what the notification menu keeps', (context) => {
    context.toast.success('Mailbox connected', { keepInNotifications: true })
    context.toast.warning('Copied', { keepInNotifications: false })

    const kept = context.store.getState().notifications.items
    expect(kept.map((notification) => notification.title)).toEqual([ 'Mailbox connected' ])
  })

  it<Context>('shows work in progress until it is closed, and never keeps it', (context) => {
    const toastId = context.toast.loading('Testing the connection')

    const shown = findShown(toastId)
    expect(shown.timeout).toBe(0)
    expect(shown.content.isLoading).toBe(true)
    expect(context.store.getState().notifications.items).toEqual([])

    context.toast.close(toastId)
    expect(isOnScreen(toastId)).toBe(false)
  })
})
