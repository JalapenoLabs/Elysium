// Copyright © 2026 Jalapeno Labs

import type { ToastOptions } from '../../toast/toast'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../../store'
import { actionItemsLoaded, selectActionItemById } from '../../store/actionItemsSlice'

// User interface
import { LeaveInitiativeButton } from './LeaveInitiativeButton'

// Misc
import { makeActionItem } from '../../testFixtures'

// The membership requests are the network boundary; toasts are watched for their Undo.
const routes = vi.hoisted(() => ({
  joinInitiative: vi.fn(),
  leaveInitiative: vi.fn(),
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

const MEMBER = makeActionItem({ id: 'item-1', title: 'Reply to Sam', initiativeIds: [ 'initiative-1' ]})
const LEFT = { ...MEMBER, initiativeIds: []}

beforeEach(() => {
  vi.clearAllMocks()
  routes.leaveInitiative.mockResolvedValue({ item: LEFT })
  routes.joinInitiative.mockResolvedValue({ item: MEMBER })
})

describe('LeaveInitiativeButton', () => {
  it('takes the item out at once and puts it back with Undo', async () => {
    const store = createAppStore()
    store.dispatch(actionItemsLoaded([ MEMBER ]))
    render(<Provider store={store}>
      <LeaveInitiativeButton item={MEMBER} initiativeId='initiative-1' />
    </Provider>)

    fireEvent.click(screen.getByRole('button', { name: 'Take out of this initiative' }))

    await waitFor(() => expect(toasts.success).toHaveBeenCalled())
    expect(routes.leaveInitiative).toHaveBeenCalledWith(MEMBER.id, 'initiative-1')
    expect(selectActionItemById(store.getState(), MEMBER.id)?.initiativeIds).toEqual([])
    const options: ToastOptions | undefined = toasts.success.mock.lastCall?.[1]
    expect(options?.action?.label).toBe('Undo')

    act(() => options?.action?.onPress())

    await waitFor(() => expect(routes.joinInitiative).toHaveBeenCalledWith(MEMBER.id, 'initiative-1'))
    await waitFor(() => {
      expect(selectActionItemById(store.getState(), MEMBER.id)?.initiativeIds).toEqual([ 'initiative-1' ])
    })
  })
})
