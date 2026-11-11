// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ManagerPlan } from '../../api/routes/actionItemRoutes'
import type { User } from '../../api/routes/userRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { meLoaded } from '../../store/authSlice'
import { projectsLoaded } from '../../store/projectsSlice'
import { usersLoaded } from '../../store/usersSlice'

// User interface
import { ActionItemManagerAssignment } from './ActionItemManagerAssignment'

// Misc
import { makeActionItem, makeProject, makeUser } from '../../testFixtures'

// The assignment routes are the network boundary; the plan is what its route answered.
const routes = vi.hoisted(() => ({
  assignActionItemToManager: vi.fn(),
  unassignActionItemFromManager: vi.fn(),
}))

vi.mock('../../api/routes/actionItemRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/actionItemRoutes')>(),
  ...routes,
}))

const server = vi.hoisted(() => ({
  plan: undefined as ManagerPlan | null | undefined,
}))

vi.mock('../../hooks/useServerData', () => ({
  useProjectsLoader: () => 'loaded',
  useUsersLoader: () => 'loaded',
  useManagerPlan: () => ({ plan: server.plan, status: 'loaded' }),
}))

vi.mock('../../toast/toast', () => ({ toast: { success: vi.fn(), warning: vi.fn(), danger: vi.fn() }}))

const ALEX = makeUser({ id: 'alex', name: 'Alex' })

function renderFor(item: ActionItem, me: User = ALEX) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: me,
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  store.dispatch(usersLoaded([ ALEX ]))
  store.dispatch(projectsLoaded([ makeProject({ id: 'launch', name: 'Launch' }) ]))
  return render(<Provider store={store}>
    <MemoryRouter>
      <ActionItemManagerAssignment item={item} />
    </MemoryRouter>
  </Provider>)
}

beforeEach(() => {
  vi.clearAllMocks()
  server.plan = undefined
})

describe('ActionItemManagerAssignment', () => {
  it('says why an item no project leads cannot be assigned, and offers nothing to press', () => {
    renderFor(makeActionItem({ id: 'loose', title: 'Annual pricing' }))

    expect(screen.getByText(/No project leads this item/)).toBeTruthy()
    const assign = screen.getByRole('button', { name: 'Assign to Project Manager' })
    expect(assign.hasAttribute('disabled') || assign.getAttribute('aria-disabled') === 'true').toBe(true)
  })

  it('assigns an item its lead project leads', async () => {
    const item = makeActionItem({ id: 'pricing', title: 'Annual pricing', leadProjectId: 'launch' })
    routes.assignActionItemToManager.mockResolvedValue({
      item: { ...item, managerAssignment: { assignedAt: '2026-11-09T12:00:00.000Z', assignedBy: 'alex' }},
    })
    renderFor(item)

    expect(screen.getByText('Not assigned yet.')).toBeTruthy()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Assign to Project Manager' }))
    })

    expect(routes.assignActionItemToManager).toHaveBeenCalledWith('pricing')
  })

  it('shows who assigned it, where its plan stands, and the batches planned for it', () => {
    server.plan = {
      projectId: 'launch',
      assignedAt: '2026-11-09T12:00:00.000Z',
      assignedBy: 'alex',
      stage: 'planned',
      turnId: 'turn-1',
      batches: [{
        id: 'batch-1',
        title: 'Show annual pricing',
        state: 'proposed',
        createdAt: '2026-11-09T12:01:00.000Z',
      }],
    }
    renderFor(makeActionItem({
      id: 'pricing',
      leadProjectId: 'launch',
      managerAssignment: { assignedAt: '2026-11-09T12:00:00.000Z', assignedBy: 'alex' },
    }))

    const manager = screen.getByText('Assigned to Launch\'s Project Manager')
    expect(manager.closest('a')?.getAttribute('href')).toBe('/projects/launch?tab=manager')
    expect(screen.getByText('Alex')).toBeTruthy()
    expect(screen.getByText('Planned')).toBeTruthy()
    expect(screen.getByText('Show annual pricing').closest('a')?.getAttribute('href')).toBe('/work-queue/batch-1')
    expect(screen.getByText('Proposed')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Take back' })).toBeTruthy()
  })

  it('is the company members\' alone, as Project Managers are', () => {
    const { container } = renderFor(
      makeActionItem({ id: 'pricing', leadProjectId: 'launch' }),
      makeUser({ id: 'guest', role: 'guest' }),
    )

    expect(container.textContent).toBe('')
  })
})
