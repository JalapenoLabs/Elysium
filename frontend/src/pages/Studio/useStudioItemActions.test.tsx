// Copyright © 2026 Jalapeno Labs

import type { StudioItem } from '../../api/routes/studioRoutes'
import type { PageAction } from '../../layout/page/pageActions'
import type { ToastOptions } from '../../toast/toast'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { selectStudioItemById, studioItemsLoaded } from '../../store/studioItemsSlice'

// User interface
import { ConfirmGate } from '../../gates/ConfirmGate'

// Misc
import { makeStudioItem } from '../../testFixtures'
import { getStudioItemUrl, UrlTree } from '../../urls'
import { useStudioItemActions } from './useStudioItemActions'

// The item requests are the network boundary; toasts are watched for their Undo.
const routes = vi.hoisted(() => ({
  deleteStudioItem: vi.fn(),
  restoreStudioItem: vi.fn(),
}))

vi.mock('../../api/routes/studioRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/studioRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

const ITEM = makeStudioItem({ id: 'studio-1', title: 'Low-poly lantern' })

// Opens the item's page, where a harness holds its actions, beside the grid it leaves for.
function renderItemPage(item: StudioItem) {
  const store = createAppStore()
  store.dispatch(studioItemsLoaded([ item ]))
  const captured: { actions: PageAction[] } = { actions: []}
  function Harness() {
    captured.actions = useStudioItemActions(item)
    return null
  }
  const router = createMemoryRouter([
    { path: UrlTree.studio, element: <p>Studio</p> },
    { path: '/studio/:itemId', element: <Harness /> },
  ], { initialEntries: [ getStudioItemUrl(item.id) ]})
  render(<Provider store={store}>
    <ConfirmGate>
      <RouterProvider router={router} />
    </ConfirmGate>
  </Provider>)
  return { store, router, captured }
}

function press(actions: PageAction[], id: string) {
  const action = actions.find((candidate) => candidate.id === id)
  if (!action || !('onAction' in action)) {
    throw new Error(`No action ${id} to press`)
  }
  act(() => action.onAction())
}

beforeEach(() => {
  vi.clearAllMocks()
  routes.deleteStudioItem.mockResolvedValue(undefined)
  routes.restoreStudioItem.mockResolvedValue({ item: ITEM })
})

describe('useStudioItemActions', () => {
  it('offers Delete and Delete permanently on a live item, and only the second on a deleted one', () => {
    const live = renderItemPage(ITEM)
    expect(live.captured.actions.map((action) => action.id)).toEqual([ 'rename', 'delete', 'deletePermanently' ])

    const deleted = renderItemPage({ ...ITEM, deletedAt: '2026-11-01T00:00:00.000Z' })
    expect(deleted.captured.actions.map((action) => action.id)).toEqual([ 'deletePermanently' ])
  })

  it('deletes softly at once, leaves for the grid, and Undo restores the item and returns to it', async () => {
    const { store, router, captured } = renderItemPage(ITEM)

    press(captured.actions, 'delete')

    await waitFor(() => expect(router.state.location.pathname).toBe(UrlTree.studio))
    expect(routes.deleteStudioItem).toHaveBeenCalledWith(ITEM.id, { permanently: false })
    expect(selectStudioItemById(store.getState(), ITEM.id)).toBeUndefined()
    const options: ToastOptions | undefined = toasts.success.mock.lastCall?.[1]
    expect(options?.action?.label).toBe('Undo')

    act(() => options?.action?.onPress())

    await waitFor(() => expect(router.state.location.pathname).toBe(getStudioItemUrl(ITEM.id)))
    expect(routes.restoreStudioItem).toHaveBeenCalledWith(ITEM.id)
    expect(selectStudioItemById(store.getState(), ITEM.id)).toEqual(ITEM)
  })

  it('asks before deleting permanently, saying it can\'t be undone', async () => {
    const { captured } = renderItemPage(ITEM)

    press(captured.actions, 'deletePermanently')

    expect(await screen.findByText('Permanently delete Low-poly lantern?')).toBeTruthy()
    expect(screen.getByText('This can\'t be undone.')).toBeTruthy()
    expect(routes.deleteStudioItem).not.toHaveBeenCalled()
  })
})
