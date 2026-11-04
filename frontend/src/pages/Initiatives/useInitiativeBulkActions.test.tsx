// Copyright © 2026 Jalapeno Labs

import type { DataTableSelectionAction } from '@jalapenolabs/uikit'
import type { Initiative } from '../../api/routes/initiativeRoutes'
import type { ToastOptions } from '../../toast/toast'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render } from '@testing-library/react'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../../store'
import { initiativesLoaded, selectInitiativeById } from '../../store/initiativesSlice'

// Misc
import { makeInitiative } from '../../testFixtures'
import { useInitiativeBulkActions } from './useInitiativeBulkActions'

// The bulk route is the network boundary; toasts are watched for what they say.
const routes = vi.hoisted(() => ({
  changeInitiatives: vi.fn(),
}))

vi.mock('../../api/routes/initiativeRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/initiativeRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

const LAUNCH = makeInitiative({ id: 'launch', name: 'Launch' })
const DEPLOY = makeInitiative({ id: 'deploy', name: 'Deploy' })

function renderActions() {
  const store = createAppStore()
  store.dispatch(initiativesLoaded([ LAUNCH, DEPLOY ]))
  const captured: { actions?: ReturnType<typeof useInitiativeBulkActions> } = {}
  function Harness() {
    captured.actions = useInitiativeBulkActions()
    return null
  }
  render(<Provider store={store}>
    <Harness />
  </Provider>)
  return { store, captured }
}

function action(captured: { actions?: ReturnType<typeof useInitiativeBulkActions> }, id: string) {
  const found = captured.actions?.liveActions.find((candidate) => candidate.id === id)
  if (!found) {
    throw new Error(`no bulk action ${id}`)
  }
  return found
}

async function run(selected: DataTableSelectionAction<Initiative>, rows: Initiative[], choice?: string) {
  await act(async () => {
    if (selected.onChoose) {
      await selected.onChoose(choice ?? '', rows)
      return
    }
    await selected.onPress(rows)
  })
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('useInitiativeBulkActions', () => {
  it('sets a selection\'s state, files the answer, and names one that refused', async () => {
    routes.changeInitiatives.mockResolvedValue({
      initiatives: [{ ...LAUNCH, state: 'achieved' }],
      refused: [{ initiativeId: DEPLOY.id, message: 'the initiative is deleted; restore it first' }],
    })
    const { store, captured } = renderActions()

    await run(action(captured, 'state'), [ LAUNCH, DEPLOY ], 'achieved')

    expect(routes.changeInitiatives).toHaveBeenCalledWith(
      [ LAUNCH.id, DEPLOY.id ],
      { kind: 'state', state: 'achieved' },
    )
    expect(selectInitiativeById(store.getState(), LAUNCH.id)?.state).toBe('achieved')
    expect(toasts.success).toHaveBeenCalledWith('Set 1 initiative to Achieved.')
    expect(toasts.warning).toHaveBeenCalledWith('Left “Deploy” as it is, since it cannot take that change.')
  })

  it('deletes at once and offers Undo, which restores exactly what it deleted', async () => {
    const deleted = [ LAUNCH, DEPLOY ].map((initiative) => ({ ...initiative, deletedAt: '2026-09-18T12:00:00.000Z' }))
    routes.changeInitiatives.mockResolvedValueOnce({ initiatives: deleted, refused: []})
    const { captured } = renderActions()

    await run(action(captured, 'delete'), [ LAUNCH, DEPLOY ])

    const options: ToastOptions | undefined = toasts.success.mock.lastCall?.[1]
    expect(toasts.success.mock.lastCall?.[0]).toBe('Deleted 2 initiatives.')

    routes.changeInitiatives.mockResolvedValueOnce({ initiatives: [ LAUNCH, DEPLOY ], refused: []})
    await act(async () => {
      options?.action?.onPress()
    })

    expect(routes.changeInitiatives).toHaveBeenLastCalledWith([ LAUNCH.id, DEPLOY.id ], { kind: 'restore' })
    await vi.waitFor(() => expect(toasts.success).toHaveBeenLastCalledWith('Restored 2 initiatives.'))
  })
})
