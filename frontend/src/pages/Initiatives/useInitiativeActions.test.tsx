// Copyright © 2026 Jalapeno Labs

import type { ToastOptions } from '../../toast/toast'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { initiativesLoaded, selectInitiativeById } from '../../store/initiativesSlice'

// Misc
import { makeInitiative } from '../../testFixtures'
import { getInitiativeViewUrl, UrlTree } from '../../urls'
import { useInitiativeActions } from './useInitiativeActions'

// The initiative requests are the network boundary; toasts are watched for their Undo.
const routes = vi.hoisted(() => ({
  deleteInitiative: vi.fn(),
  restoreInitiative: vi.fn(),
}))

vi.mock('../../api/routes/initiativeRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/initiativeRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

const INITIATIVE = makeInitiative({ id: 'initiative-1', name: 'Stability and polish' })

// Opens the initiative's page, where a harness holds the actions, beside the list it leaves
// for. No confirmation dialog is mounted, so asking for one would throw.
function renderInitiativePage() {
  const store = createAppStore()
  store.dispatch(initiativesLoaded([ INITIATIVE ]))
  const captured: { actions?: ReturnType<typeof useInitiativeActions> } = {}
  function Harness() {
    captured.actions = useInitiativeActions()
    return null
  }
  const router = createMemoryRouter([
    { path: UrlTree.initiatives, element: <p>Initiatives</p> },
    { path: '/action-items/initiatives/:initiativeId', element: <Harness /> },
  ], { initialEntries: [ getInitiativeViewUrl(INITIATIVE.id) ]})
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
  routes.deleteInitiative.mockResolvedValue(undefined)
  routes.restoreInitiative.mockResolvedValue({ initiative: INITIATIVE })
})

describe('useInitiativeActions remove', () => {
  it('deletes at once and leaves for the list, with Undo in its toast', async () => {
    const { store, router, captured } = renderInitiativePage()

    await act(async () => {
      await captured.actions?.remove(INITIATIVE, UrlTree.initiatives)
    })

    expect(routes.deleteInitiative).toHaveBeenCalledWith(INITIATIVE.id)
    expect(router.state.location.pathname).toBe(UrlTree.initiatives)
    expect(selectInitiativeById(store.getState(), INITIATIVE.id)).toBeUndefined()
    expect(toasts.success).toHaveBeenCalledWith('Deleted “Stability and polish”.', expect.anything())
    expect(lastSuccessOptions()?.action?.label).toBe('Undo')
  })

  it('restores the initiative through Undo and returns to its page', async () => {
    const { store, router, captured } = renderInitiativePage()
    await act(async () => {
      await captured.actions?.remove(INITIATIVE, UrlTree.initiatives)
    })

    act(() => lastSuccessOptions()?.action?.onPress())

    await waitFor(() => expect(router.state.location.pathname).toBe(getInitiativeViewUrl(INITIATIVE.id)))
    expect(routes.restoreInitiative).toHaveBeenCalledWith(INITIATIVE.id)
    expect(selectInitiativeById(store.getState(), INITIATIVE.id)).toEqual(INITIATIVE)
  })
})
