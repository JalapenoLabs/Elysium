// Copyright © 2026 Jalapeno Labs

import type { KnowledgeResult } from '../../api/routes/knowledgeRoutes'

// Core
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../store'
import { pagesLoaded } from '../../store/pagesSlice'

// User interface
import { KnowledgeSearchPage } from './KnowledgeSearchPage'

// Misc
import { searchKnowledge } from '../../api/routes/knowledgeRoutes'
import { makePageSummary } from '../../testFixtures'
import { TestAriaRouter } from '../../testRouter'
import { UrlTree } from '../../urls'

vi.mock('../../api/routes/knowledgeRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/knowledgeRoutes')>(),
  searchKnowledge: vi.fn(),
}))

vi.mock('../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../hooks/useServerData')>(),
  useProjectsLoader: () => 'loaded',
  useUsersLoader: () => 'loaded',
}))

// jsdom has no observers, which the tabs measure with.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const pageResult: KnowledgeResult = {
  citation: {
    documentId: 'd1',
    source: 'page',
    title: 'Runbook',
    url: null,
    pageId: 'runbook',
    meetingId: null,
    occurredAt: '2026-11-01T10:00:00Z',
    person: { id: 'u1', name: 'Maya Chen' },
  },
  passages: [{ position: 2, content: 'Restart the fleet.' }],
  alsoRecordedBy: [],
  score: 0.03,
  pendingSuggestions: 0,
}

function renderSearch(address: string) {
  const store = createAppStore()
  store.dispatch(pagesLoaded([
    makePageSummary({ id: 'guide', title: 'Guide' }),
    makePageSummary({ id: 'runbook', parentId: 'guide', title: 'Runbook' }),
  ]))
  const router = createMemoryRouter(
    [{
      path: UrlTree.knowledgeSearch,
      element: <TestAriaRouter>
        <KnowledgeSearchPage />
      </TestAriaRouter>,
    }],
    { initialEntries: [ address ]},
  )
  render(<Provider store={store}>
    <RouterProvider router={router} />
  </Provider>)
  return router
}

beforeEach(() => {
  vi.stubGlobal('IntersectionObserver', InertObserver)
  vi.stubGlobal('ResizeObserver', InertObserver)
  Element.prototype.getAnimations = () => []
  vi.mocked(searchKnowledge).mockReset()
  vi.mocked(searchKnowledge).mockResolvedValue({ results: [ pageResult ], searchedByMeaning: true })
})

afterEach(() => {
  vi.unstubAllGlobals()
  Reflect.deleteProperty(Element.prototype, 'getAnimations')
})

describe('KnowledgeSearchPage', () => {
  it('searches every source on its first tab', async () => {
    renderSearch('/knowledge/search?q=fleet')

    await waitFor(() => expect(searchKnowledge).toHaveBeenCalled())

    expect(vi.mocked(searchKnowledge).mock.calls[0][0].sources).toEqual([])
  })

  it('searches pages, learnings, and the company doc on the Pages tab, dropping what cannot match', async () => {
    renderSearch('/knowledge/search?q=fleet&tab=pages&source=slack&project=p1')

    await waitFor(() => expect(searchKnowledge).toHaveBeenCalled())

    const asked = vi.mocked(searchKnowledge).mock.calls[0][0]
    expect(asked.sources).toEqual([ 'page', 'learning', 'company_doc' ])
    expect(asked.projectId).toBeNull()
  })

  it('opens a page\'s result in the page at its passage, saying where it sits', async () => {
    renderSearch('/knowledge/search?q=fleet&tab=pages')

    const title = await screen.findByRole('link', { name: 'Runbook' })

    expect(title.getAttribute('href')).toBe('/knowledge/pages/runbook?passage=2')
    expect(screen.getByRole('link', { name: 'Restart the fleet.' }).getAttribute('href'))
      .toBe('/knowledge/pages/runbook?passage=2')
    expect(screen.getByText('In Guide')).toBeDefined()
  })

  it('keeps the query across tabs', async () => {
    const router = renderSearch('/knowledge/search?q=fleet')

    fireEvent.click(await screen.findByRole('tab', { name: 'Pages' }))

    await waitFor(() => expect(router.state.location.search).toBe('?q=fleet&tab=pages'))
  })

  it('offers pages\' own filters on the Pages tab, in the words of a page\'s byline', async () => {
    renderSearch('/knowledge/search?q=fleet&tab=pages')

    fireEvent.click(await screen.findByRole('button', { name: 'Filters' }))

    const drawer = await screen.findByRole('dialog')
    expect(within(drawer).getByText('Kinds of page')).toBeDefined()
    expect(within(drawer).getByRole('checkbox', { name: 'Learnings' })).toBeDefined()
    expect(within(drawer).queryByRole('checkbox', { name: 'Slack' })).toBeNull()
    expect(within(drawer).queryByText('Project')).toBeNull()
    expect(within(drawer).getAllByText('Last edited by').length).toBeGreaterThan(0)
    expect(within(drawer).getAllByText('Edited since').length).toBeGreaterThan(0)
  })
})
