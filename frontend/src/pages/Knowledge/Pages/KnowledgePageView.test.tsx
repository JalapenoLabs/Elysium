// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { act, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { pageLoaded } from '../../../store/pagesSlice'

// User interface
import { KnowledgePageView } from './KnowledgePageView'

// Misc
import { getPagePassage } from '../../../api/routes/pageRoutes'
import { MARKED_PASSAGE_CLASS } from '../../../components/markdown/rehypeMarkPassage'
import { makePage } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { getKnowledgePageUrl, UrlTree } from '../../../urls'

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  getPagePassage: vi.fn(),
}))

vi.mock('../../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../hooks/useServerData')>(),
  usePageLoader: () => 'loaded',
  useUsersLoader: () => 'loaded',
}))

// jsdom has no observers, which the tabs measure with.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const PAGE_ID = 'runbook'
const scrollIntoView = vi.fn()

function renderPage(page: KnowledgePage, address: string) {
  const store = createAppStore()
  store.dispatch(pageLoaded(page))
  const router = createMemoryRouter(
    [{
      path: UrlTree.knowledgePage,
      element: <TestAriaRouter>
        <KnowledgePageView />
      </TestAriaRouter>,
    }],
    { initialEntries: [ address ]},
  )
  render(<SWRConfig value={{ provider: () => new Map() }}>
    <Provider store={store}>
      <RouterProvider router={router} />
    </Provider>
  </SWRConfig>)
  return { store, router }
}

function markedBlock() {
  return document.querySelector(`.${MARKED_PASSAGE_CLASS}`)
}

beforeEach(() => {
  vi.stubGlobal('IntersectionObserver', InertObserver)
  vi.stubGlobal('ResizeObserver', InertObserver)
  scrollIntoView.mockClear()
  Element.prototype.scrollIntoView = scrollIntoView
  vi.mocked(getPagePassage).mockReset()
})

afterEach(() => {
  vi.unstubAllGlobals()
  Reflect.deleteProperty(Element.prototype, 'scrollIntoView')
})

describe('KnowledgePageView', () => {
  const page = makePage({ id: PAGE_ID, title: 'Runbook', body: 'Restart the fleet.\n\nThen check the logs.' })

  it('opens at the passage a citation named, marked and in view', async () => {
    vi.mocked(getPagePassage).mockResolvedValue({
      passage: { position: 1, content: 'Then check the logs.' },
    })

    renderPage(page, getKnowledgePageUrl(PAGE_ID, 1))

    await waitFor(() => expect(markedBlock()?.textContent).toBe('Then check the logs.'))
    expect(getPagePassage).toHaveBeenCalledWith(PAGE_ID, 1)
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledOnce())
  })

  it('leaves a page\'s title out of its first passage', async () => {
    vi.mocked(getPagePassage).mockResolvedValue({
      passage: { position: 0, content: 'Runbook\n\nRestart the fleet.' },
    })

    renderPage(page, getKnowledgePageUrl(PAGE_ID, 0))

    await waitFor(() => expect(markedBlock()?.textContent).toBe('Restart the fleet.'))
  })

  it('opens at the top when the index no longer holds the passage', async () => {
    vi.mocked(getPagePassage).mockRejectedValue(new Error('404'))

    renderPage(page, getKnowledgePageUrl(PAGE_ID, 9))

    await screen.findByText('Restart the fleet.')
    await waitFor(() => expect(getPagePassage).toHaveBeenCalled())
    expect(markedBlock()).toBeNull()
    expect(scrollIntoView).not.toHaveBeenCalled()
  })

  it('opens at the top when the passage is text the page no longer says', async () => {
    vi.mocked(getPagePassage).mockResolvedValue({
      passage: { position: 1, content: 'Something older.' },
    })

    renderPage(page, getKnowledgePageUrl(PAGE_ID, 1))

    await screen.findByText('Restart the fleet.')
    await waitFor(() => expect(getPagePassage).toHaveBeenCalled())
    await act(async () => {
      await Promise.resolve()
    })
    expect(markedBlock()).toBeNull()
  })

  it('reads no passage on a page opened plainly, or on its History tab', async () => {
    renderPage(page, getKnowledgePageUrl(PAGE_ID))
    await screen.findByText('Restart the fleet.')

    expect(getPagePassage).not.toHaveBeenCalled()
  })

  it('keeps the mark through a publish, and the reader where they are', async () => {
    vi.mocked(getPagePassage).mockResolvedValue({
      passage: { position: 1, content: 'Then check the logs.' },
    })
    const { store } = renderPage(page, getKnowledgePageUrl(PAGE_ID, 1))
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledOnce())

    act(() => {
      store.dispatch(pageLoaded({
        ...page,
        currentVersion: page.currentVersion + 1,
        body: `A new line.\n\n${page.body}`,
      }))
    })

    await screen.findByText('A new line.')
    await waitFor(() => expect(getPagePassage).toHaveBeenCalledTimes(2))
    expect(markedBlock()?.textContent).toBe('Then check the logs.')
    expect(scrollIntoView).toHaveBeenCalledOnce()
  })
})
