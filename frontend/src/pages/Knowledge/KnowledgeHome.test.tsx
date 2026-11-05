// Copyright © 2026 Jalapeno Labs

// Core
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../store'
import { pagesLoaded } from '../../store/pagesSlice'

// User interface
import { KnowledgeHome } from './KnowledgeHome'

// Misc
import { makePageSummary } from '../../testFixtures'
import { getKnowledgePageUrl, UrlTree } from '../../urls'

const { loadStatus } = vi.hoisted(() => ({ loadStatus: { current: 'loaded' }}))

vi.mock('../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../hooks/useServerData')>(),
  usePagesLoader: () => loadStatus.current,
}))

function renderHome(store: ReturnType<typeof createAppStore>, address: string) {
  const router = createMemoryRouter(
    [
      { path: UrlTree.knowledge, element: <KnowledgeHome /> },
      { path: '*', element: null },
    ],
    { initialEntries: [ address ]},
  )
  render(<Provider store={store}>
    <RouterProvider router={router} />
  </Provider>)
  return router
}

describe('KnowledgeHome', () => {
  beforeEach(() => {
    loadStatus.current = 'loaded'
  })

  it('opens on the first page at the top of the tree', () => {
    const store = createAppStore()
    store.dispatch(pagesLoaded([
      makePageSummary({ id: 'second', position: 2 }),
      makePageSummary({ id: 'first', position: 1 }),
    ]))

    const router = renderHome(store, UrlTree.knowledge)

    expect(router.state.location.pathname).toBe(getKnowledgePageUrl('first'))
  })

  it('takes a search\'s address on to the search, query and all', () => {
    const store = createAppStore()
    store.dispatch(pagesLoaded([ makePageSummary({ id: 'first' }) ]))

    const router = renderHome(store, `${UrlTree.knowledge}?q=retries&source=slack`)

    expect(router.state.location.pathname).toBe(UrlTree.knowledgeSearch)
    expect(router.state.location.search).toBe('?q=retries&source=slack')
  })

  it('says so when there are no pages', () => {
    renderHome(createAppStore(), UrlTree.knowledge)

    expect(screen.getByText('No pages yet')).toBeDefined()
  })
})
