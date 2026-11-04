// Copyright © 2026 Jalapeno Labs

import type { ToastOptions } from '../../toast/toast'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { actionItemsLoaded, selectActionItemById } from '../../store/actionItemsSlice'

// Misc
import { makeActionItem } from '../../testFixtures'
import { getActionItemViewUrl, UrlTree } from '../../urls'
import { useActionItemActions } from './useActionItemActions'

// The item requests are the network boundary; toasts are watched for their Undo.
const routes = vi.hoisted(() => ({
  deleteActionItem: vi.fn(),
  restoreActionItem: vi.fn(),
}))

vi.mock('../../api/routes/actionItemRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/actionItemRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

const ITEM = makeActionItem({ id: 'item-1', title: 'Reply to Sam' })

// Opens the item's page, where a harness holds the actions, beside the list it leaves for. No
// confirmation dialog is mounted, so asking for one would throw.
function renderItemPage() {
  const store = createAppStore()
  store.dispatch(actionItemsLoaded([ ITEM ]))
  const captured: { actions?: ReturnType<typeof useActionItemActions> } = {}
  function Harness() {
    captured.actions = useActionItemActions()
    return null
  }
  const router = createMemoryRouter([
    { path: UrlTree.actionItemsAll, element: <p>All items</p> },
    { path: '/action-items/:itemId', element: <Harness /> },
  ], { initialEntries: [ getActionItemViewUrl(ITEM.id) ]})
  render(<Provider store={store}>
    <RouterProvider router={router} />
  </Provider>)
  return { store, router, captured }
}

function lastSuccessOptions(): ToastOptions | undefined {
  return toasts.success.mock.lastCall?.[1]
}

beforeEach(() => {
  vi.clearAllMocks()
  routes.deleteActionItem.mockResolvedValue(undefined)
  routes.restoreActionItem.mockResolvedValue({ item: ITEM })
})

describe('useActionItemActions remove', () => {
  it('deletes at once, leaves the item\'s page, and offers Undo', async () => {
    const { store, router, captured } = renderItemPage()

    let isDeleted = false
    await act(async () => {
      isDeleted = await captured.actions?.remove(ITEM, UrlTree.actionItemsAll) ?? false
    })

    expect(isDeleted).toBe(true)
    expect(routes.deleteActionItem).toHaveBeenCalledWith(ITEM.id)
    expect(router.state.location.pathname).toBe(UrlTree.actionItemsAll)
    expect(selectActionItemById(store.getState(), ITEM.id)).toBeUndefined()
    expect(toasts.success).toHaveBeenCalledWith('Deleted “Reply to Sam”.', expect.anything())
    expect(lastSuccessOptions()?.action?.label).toBe('Undo')
  })

  it('restores the item through Undo and returns to its page', async () => {
    const { store, router, captured } = renderItemPage()
    await act(async () => {
      await captured.actions?.remove(ITEM, UrlTree.actionItemsAll)
    })

    act(() => lastSuccessOptions()?.action?.onPress())

    await waitFor(() => expect(router.state.location.pathname).toBe(getActionItemViewUrl(ITEM.id)))
    expect(routes.restoreActionItem).toHaveBeenCalledWith(ITEM.id)
    expect(selectActionItemById(store.getState(), ITEM.id)).toEqual(ITEM)
    expect(toasts.success).toHaveBeenLastCalledWith('Restored “Reply to Sam”.')
  })

  it('stays where it is when the delete is refused, with nothing to undo', async () => {
    routes.deleteActionItem.mockRejectedValue(new Error('offline'))
    const { store, router, captured } = renderItemPage()

    let isDeleted = true
    await act(async () => {
      isDeleted = await captured.actions?.remove(ITEM, UrlTree.actionItemsAll) ?? true
    })

    expect(isDeleted).toBe(false)
    expect(router.state.location.pathname).toBe(getActionItemViewUrl(ITEM.id))
    expect(selectActionItemById(store.getState(), ITEM.id)).toEqual(ITEM)
    expect(toasts.success).not.toHaveBeenCalled()
    expect(toasts.danger).toHaveBeenCalled()
  })
})
