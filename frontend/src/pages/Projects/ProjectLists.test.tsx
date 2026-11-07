// Copyright © 2026 Jalapeno Labs

import type { UserRole } from '../../api/routes/userRoutes'

// Core
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../store'
import { meLoaded } from '../../store/authSlice'
import { projectManagerLoaded } from '../../store/projectManagersSlice'
import { projectsLoaded } from '../../store/projectsSlice'

// User interface
import { ProjectLists } from './ProjectLists'

// Misc
import {
  makePageSummary,
  makeProject,
  makeProjectManager,
  makeUser,
  makeWorkBatch,
} from '../../testFixtures'
import { TestAriaRouter } from '../../testRouter'
import { getProjectDocsUrl } from '../../urls'
import { useProjectPages } from './useProjectPages'

vi.mock('./useProjectPages', () => ({ useProjectPages: vi.fn() }))

vi.mock('../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../hooks/useServerData')>(),
  useActionItemsLoader: () => 'loaded',
  useInitiativesLoader: () => 'loaded',
  useCodingSessionsLoader: () => 'loaded',
  useProjectManagerLoader: () => 'loaded',
  useProjectTimelineLoader: () => ({ total: 0 }),
  usePagesLoader: () => 'loaded',
  useUsersLoader: () => 'loaded',
}))

// jsdom has no ResizeObserver, which the kit's table and HeroUI's scroll shadows measure with.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const LINKED_PAGE = {
  ...makePageSummary({ id: 'page', title: 'Launch checklist' }),
  publishedBy: 'person',
  publishedAt: '2026-11-01T12:00:00Z',
  linkedBy: 'person',
  linkedAt: '2026-11-02T12:00:00Z',
}

function renderLists(role: UserRole, path: string) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser({ role }),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  store.dispatch(projectsLoaded([ makeProject({ id: 'launch', name: 'Launch' }) ]))
  store.dispatch(projectManagerLoaded(makeProjectManager({
    projectId: 'launch',
    proposed: [ makeWorkBatch({
      id: 'pricing',
      projectId: 'launch',
      title: 'Annual pricing page',
      origin: 'found',
      generation: 1,
    }) ],
  })))
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={store}>
      <TestAriaRouter>
        <ProjectLists projectId='launch' />
      </TestAriaRouter>
    </Provider>,
  }], { initialEntries: [ path ]})
  render(<RouterProvider router={router} />)
}

describe('ProjectLists', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.mocked(useProjectPages).mockReturnValue({ pages: [ LINKED_PAGE ], status: 'loaded' })
  })

  it('shows a member the project\'s docs with their count', () => {
    renderLists('member', getProjectDocsUrl('launch'))

    expect(screen.getByRole('tab', { name: /Docs/ }).textContent).toContain('1')
    expect(screen.getByRole('link', { name: 'Launch checklist' })).toBeTruthy()
    expect(useProjectPages).toHaveBeenCalledWith('launch', true)
  })

  it('lists what waits for the person\'s approval on its own tab, counted', () => {
    renderLists('member', '/projects/launch?tab=proposed')

    expect(screen.getByRole('tab', { name: /Proposed/, selected: true }).textContent).toContain('1')
    expect(screen.getByRole('link', { name: 'Annual pricing page' })).toBeTruthy()
    // A person deciding here sees whether anyone asked for it, as on the work queue.
    expect(screen.getByText('Found by an agent')).toBeTruthy()
  })

  it('shows no count until the docs are known', () => {
    vi.mocked(useProjectPages).mockReturnValue({ pages: undefined, status: 'loading' })

    renderLists('member', '/projects/launch')

    expect(screen.getByRole('tab', { name: 'Docs' }).textContent).toBe('Docs')
  })

  it('gives a guest no Docs tab and asks nothing for it, even at its address', () => {
    vi.mocked(useProjectPages).mockReturnValue({ pages: undefined, status: 'loading' })

    renderLists('guest', getProjectDocsUrl('launch'))

    expect(screen.queryByRole('tab', { name: /Docs/ })).toBeNull()
    expect(useProjectPages).toHaveBeenCalledWith('launch', false)
    expect(screen.getByRole('tab', { name: /Action items/, selected: true })).toBeTruthy()
  })
})
