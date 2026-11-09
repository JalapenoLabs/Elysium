// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { projectsLoaded } from '../../store/projectsSlice'

// User interface
import { ActionItemMemberships } from './ActionItemMemberships'

// Misc
import { makeActionItem, makeProject } from '../../testFixtures'

// Projects arrive through Redux; the loaders' requests are the network boundary.
vi.mock('../../hooks/useServerData', () => ({
  useProjectsLoader: () => 'loaded',
  useInitiativesLoader: () => 'loaded',
}))

function renderFor(item: ActionItem) {
  const store = createAppStore()
  store.dispatch(projectsLoaded([
    makeProject({ id: 'website', name: 'Website' }),
    makeProject({ id: 'launch', name: 'Launch' }),
  ]))
  return render(<Provider store={store}>
    <MemoryRouter>
      <ActionItemMemberships item={item} />
    </MemoryRouter>
  </Provider>)
}

describe('ActionItemMemberships', () => {
  it('marks the lead project and says how sure Elysia was of a project she guessed', () => {
    renderFor(makeActionItem({
      id: 'login',
      projectIds: [ 'launch', 'website' ],
      leadProjectId: 'website',
      projectAttributions: [
        { projectId: 'launch', confidence: 1, attributedBy: 'user:u1' },
        { projectId: 'website', confidence: 0.8, attributedBy: 'elysia' },
      ],
    }))

    const website = screen.getByText('Website').closest('a')
    expect(website?.querySelector('[aria-label="Lead project"]')).toBeTruthy()
    expect(website?.textContent).toContain('Elysia, 80% sure')

    const launch = screen.getByText('Launch').closest('a')
    expect(launch?.querySelector('[aria-label="Project"]')).toBeTruthy()
    expect(launch?.textContent).not.toContain('Elysia')
  })
})
