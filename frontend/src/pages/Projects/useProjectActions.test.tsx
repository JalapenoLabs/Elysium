// Copyright © 2026 Jalapeno Labs

import type { PageAction } from '../../layout/page/pageActions'
import type { ToastOptions } from '../../toast/toast'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { projectsLoaded, selectProjectById } from '../../store/projectsSlice'

// Misc
import { makeProject } from '../../testFixtures'
import { useProjectActions } from './useProjectActions'

// The cover requests are the network boundary; toasts are watched for their Undo.
const routes = vi.hoisted(() => ({
  deleteProjectCover: vi.fn(),
  restoreProjectCover: vi.fn(),
}))

vi.mock('../../api/routes/projectRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/projectRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

const COVERED = makeProject({ id: 'project-1', name: 'Elysium', coverUpdatedAt: '2026-11-01T00:00:00.000Z' })
const UNCOVERED = { ...COVERED, coverUpdatedAt: null }

beforeEach(() => {
  vi.clearAllMocks()
  routes.deleteProjectCover.mockResolvedValue({ project: UNCOVERED })
  routes.restoreProjectCover.mockResolvedValue({ project: COVERED })
})

describe('useProjectActions removeCover', () => {
  it('removes the cover at once and brings the same cover back with Undo', async () => {
    const store = createAppStore()
    store.dispatch(projectsLoaded([ COVERED ]))
    const captured: { actions: PageAction[] } = { actions: []}
    function Harness() {
      captured.actions = useProjectActions({ project: COVERED, sessionCount: 0 })
      return null
    }
    render(<Provider store={store}>
      <MemoryRouter>
        <Harness />
      </MemoryRouter>
    </Provider>)

    const removeCover = captured.actions.find((action) => action.id === 'removeCover')
    if (!removeCover || !('onAction' in removeCover)) {
      throw new Error('Remove cover is not an action')
    }
    act(() => removeCover.onAction())

    await waitFor(() => expect(toasts.success).toHaveBeenCalledWith('Cover removed', expect.anything()))
    expect(selectProjectById(store.getState(), COVERED.id)?.coverUpdatedAt).toBeNull()
    const options: ToastOptions | undefined = toasts.success.mock.lastCall?.[1]
    expect(options?.action?.label).toBe('Undo')

    act(() => options?.action?.onPress())

    await waitFor(() => expect(toasts.success).toHaveBeenLastCalledWith('Cover restored'))
    expect(routes.restoreProjectCover).toHaveBeenCalledWith(COVERED.id)
    expect(selectProjectById(store.getState(), COVERED.id)?.coverUpdatedAt).toBe(COVERED.coverUpdatedAt)
  })
})
