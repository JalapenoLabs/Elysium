// Copyright © 2026 Jalapeno Labs

import type { DataTableSelectionAction } from '@jalapenolabs/uikit'
import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { ToastOptions } from '../../toast/toast'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render } from '@testing-library/react'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../../store'
import { actionItemsLoaded, selectActionItemById } from '../../store/actionItemsSlice'

// Misc
import { ACTION_ITEM_BULK_MAX_ITEMS } from '../../constants'
import { makeActionItem } from '../../testFixtures'
import { useActionItemBulkActions } from './useActionItemBulkActions'

// The bulk route is the network boundary; toasts are watched for what they say.
const routes = vi.hoisted(() => ({
  changeActionItems: vi.fn(),
}))

vi.mock('../../api/routes/actionItemRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/actionItemRoutes')>(),
  ...routes,
}))

// The initiatives a selection could join are read from the store; nothing is fetched.
vi.mock('../../hooks/useServerData', () => ({
  useInitiativesLoader: () => 'loaded',
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

const NOW = Date.parse('2026-09-18T12:00:00.000Z')
const FIRST = makeActionItem({ id: 'item-1', title: 'Reply to Sam' })
const SECOND = makeActionItem({ id: 'item-2', title: 'Review the deck' })

function renderActions(items: ActionItem[]) {
  const store = createAppStore()
  store.dispatch(actionItemsLoaded(items))
  const captured: { actions?: ReturnType<typeof useActionItemBulkActions> } = {}
  function Harness() {
    captured.actions = useActionItemBulkActions(NOW)
    return null
  }
  render(<Provider store={store}>
    <Harness />
  </Provider>)
  return { store, captured }
}

function liveAction(captured: { actions?: ReturnType<typeof useActionItemBulkActions> }, id: string) {
  const action = captured.actions?.liveActions.find((candidate) => candidate.id === id)
  if (!action) {
    throw new Error(`no bulk action ${id}`)
  }
  return action
}

// Presses an action the way the selection bar does: a choice for a drop-up, or the button.
async function run(action: DataTableSelectionAction<ActionItem>, rows: ActionItem[], choice?: string) {
  await act(async () => {
    if (action.onChoose) {
      await action.onChoose(choice ?? '', rows)
      return
    }
    await action.onPress(rows)
  })
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('useActionItemBulkActions', () => {
  it('moves a selection to a state in one request, files the answer, and says how many', async () => {
    const resolved = [ FIRST, SECOND ].map((item) => ({ ...item, state: 'resolved' as const }))
    routes.changeActionItems.mockResolvedValue({ items: resolved, refused: []})
    const { store, captured } = renderActions([ FIRST, SECOND ])

    await run(liveAction(captured, 'state'), [ FIRST, SECOND ], 'resolved')

    expect(routes.changeActionItems).toHaveBeenCalledTimes(1)
    expect(routes.changeActionItems).toHaveBeenCalledWith(
      [ FIRST.id, SECOND.id ],
      { kind: 'state', state: 'resolved' },
    )
    expect(selectActionItemById(store.getState(), FIRST.id)?.state).toBe('resolved')
    expect(toasts.success).toHaveBeenCalledWith('Resolved 2 items.')
  })

  it('names an item the change was refused for, by title, in a warning', async () => {
    routes.changeActionItems.mockResolvedValue({
      items: [{ ...FIRST, priority: 'urgent' }],
      refused: [{ itemId: SECOND.id, message: 'the item is deleted; restore it first' }],
    })
    const { captured } = renderActions([ FIRST, SECOND ])

    await run(liveAction(captured, 'priority'), [ FIRST, SECOND ], 'urgent')

    expect(toasts.success).toHaveBeenCalledWith('Set 1 item to Urgent priority.')
    expect(toasts.warning).toHaveBeenCalledWith(
      'Left “Review the deck” as it is, since it cannot take that change.',
    )
  })

  it('deletes at once and offers Undo, which restores exactly what it deleted', async () => {
    const deleted = [ FIRST, SECOND ].map((item) => ({ ...item, deletedAt: '2026-09-18T12:00:00.000Z' }))
    routes.changeActionItems.mockResolvedValueOnce({ items: deleted, refused: []})
    const { store, captured } = renderActions([ FIRST, SECOND ])

    await run(liveAction(captured, 'delete'), [ FIRST, SECOND ])

    expect(selectActionItemById(store.getState(), FIRST.id)?.deletedAt).toBe('2026-09-18T12:00:00.000Z')
    const options: ToastOptions | undefined = toasts.success.mock.lastCall?.[1]
    expect(toasts.success.mock.lastCall?.[0]).toBe('Deleted 2 items.')
    expect(options?.action?.label).toBe('Undo')

    routes.changeActionItems.mockResolvedValueOnce({ items: [ FIRST, SECOND ], refused: []})
    await act(async () => {
      options?.action?.onPress()
    })

    expect(routes.changeActionItems).toHaveBeenLastCalledWith([ FIRST.id, SECOND.id ], { kind: 'restore' })
    await vi.waitFor(() => expect(selectActionItemById(store.getState(), FIRST.id)?.deletedAt).toBeNull())
    expect(toasts.success).toHaveBeenLastCalledWith('Restored 2 items.')
  })

  it('sends a selection larger than the route takes in parts', async () => {
    const many = Array.from(
      { length: ACTION_ITEM_BULK_MAX_ITEMS + 1 },
      (_unused, index) => makeActionItem({ id: `item-${index}` }),
    )
    routes.changeActionItems.mockResolvedValue({ items: [], refused: []})
    const { captured } = renderActions(many)

    await run(liveAction(captured, 'snooze'), many, 'end')

    expect(routes.changeActionItems).toHaveBeenCalledTimes(2)
    expect(routes.changeActionItems.mock.calls[0][0]).toHaveLength(ACTION_ITEM_BULK_MAX_ITEMS)
    expect(routes.changeActionItems.mock.calls[1][0]).toEqual([ `item-${ACTION_ITEM_BULK_MAX_ITEMS}` ])
    expect(routes.changeActionItems.mock.calls[0][1]).toEqual({ kind: 'snooze', until: null })
  })

  it('says a request failed and changes nothing more', async () => {
    routes.changeActionItems.mockRejectedValue(new Error('offline'))
    const { captured } = renderActions([ FIRST ])

    await run(liveAction(captured, 'state'), [ FIRST ], 'dismissed')

    expect(toasts.danger).toHaveBeenCalledTimes(1)
    expect(toasts.success).not.toHaveBeenCalled()
  })
})
