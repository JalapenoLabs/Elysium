// Copyright © 2026 Jalapeno Labs

import type { SearchSourceAnswer } from '../../search/searchTypes'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { meLoaded } from '../../store/authSlice'

// User interface
import { LuFolderKanban, LuListTodo } from 'react-icons/lu'
import { ConfirmGate } from '../../gates/ConfirmGate'
import { SearchPage } from './SearchPage'

// Misc
import { makeUser } from '../../testFixtures'
import { TestAriaRouter } from '../../testRouter'

// What the API's records answer, set by each test; `useRecordSearch.test.ts` covers the wait
// and the cancelling. The page asks for the most of each kind.
const records = vi.hoisted(() => ({
  answer: { results: [], isLoading: false, hasFailed: false } as SearchSourceAnswer,
  limits: [] as number[],
}))
vi.mock('../../search/useRecordSearch', () => ({
  useRecordSearch: (_query: string, limit: number) => {
    records.limits.push(limit)
    return records.answer
  },
}))

// The knowledge base's documents, which this page's tests leave empty: `knowledgeSearch.test.ts`
// covers how they become results, and `GlobalSearch.test.tsx` how they show.
vi.mock('../../search/useKnowledgeResults', () => ({
  useKnowledgeResults: () => ({ results: [], isLoading: false, hasFailed: false }),
}))

// jsdom has neither observer, nor the Web Animations API, which the tabs and the page use.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', InertObserver)
  vi.stubGlobal('IntersectionObserver', InertObserver)
  Element.prototype.getAnimations = () => []
  records.limits = []
  records.answer = {
    isLoading: false,
    hasFailed: false,
    results: [
      {
        id: 'records:project:p1',
        group: 'projects',
        title: 'Security review',
        icon: LuFolderKanban,
        score: 4,
        target: { kind: 'link', href: '/projects/p1' },
      },
      {
        id: 'records:actionItem:i1',
        group: 'actionItems',
        title: 'Security audit',
        detail: 'Open',
        icon: LuListTodo,
        score: 4,
        target: { kind: 'link', href: '/action-items/i1' },
      },
    ],
  }
})

afterEach(() => {
  vi.unstubAllGlobals()
  // @ts-expect-error jsdom has no getAnimations of its own to put back.
  delete Element.prototype.getAnimations
})

function renderPage(initialEntry: string) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser({ role: 'admin' }),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  const router = createMemoryRouter([{
    path: '/search',
    element: <TestAriaRouter>
      <ConfirmGate>
        <SearchPage />
      </ConfirmGate>
    </TestAriaRouter>,
  }], { initialEntries: [ initialEntry ]})
  render(<Provider store={store}>
    <RouterProvider router={router} />
  </Provider>)
  return router
}

function shownTitles() {
  const panel = screen.getByRole('tabpanel')
  return within(panel).getAllByRole('link').map((link) => link.textContent)
}

describe('SearchPage', () => {
  it('lists every kind of result in its own section, asking for the most of each kind', () => {
    renderPage('/search?q=security')

    const panel = screen.getByRole('tabpanel')
    expect(within(panel).getByRole('heading', { name: 'Pages' })).toBeTruthy()
    expect(within(panel).getByRole('heading', { name: 'Projects' })).toBeTruthy()
    expect(within(panel).getByRole('heading', { name: 'Action items' })).toBeTruthy()
    expect(shownTitles()).toEqual(expect.arrayContaining([ 'Security review', 'Security auditOpen' ]))
    expect(new Set(records.limits)).toEqual(new Set([ 50 ]))
  })

  it('counts each kind on its tab', () => {
    renderPage('/search?q=security')

    expect(screen.getByRole('tab', { name: /Projects/ }).textContent).toBe('Projects1')
    expect(screen.getByRole('tab', { name: /Initiatives/ }).textContent).toBe('Initiatives0')
  })

  it('narrows to one kind from its tab, kept in the address with the query', () => {
    const router = renderPage('/search?q=security')

    fireEvent.click(screen.getByRole('tab', { name: /Projects/ }))
    expect(router.state.location.search).toBe('?q=security&tab=projects')
    expect(shownTitles()).toEqual([ 'Security review' ])
    expect(within(screen.getByRole('tabpanel')).queryByRole('heading')).toBeNull()
  })

  it('opens on the kind the address names, and says when that kind has nothing', () => {
    renderPage('/search?q=security&tab=initiatives')

    expect(screen.getByRole('tab', { name: /Initiatives/, selected: true })).toBeTruthy()
    expect(within(screen.getByRole('tabpanel')).getByText('No results for “security”')).toBeTruthy()
  })

  it('keeps the open tab while the query changes', () => {
    const router = renderPage('/search?q=security&tab=projects')

    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'review' }})
    expect(router.state.location.search).toBe('?q=review&tab=projects')
  })

  it('leaves counts out while the records load', () => {
    records.answer = { results: [], isLoading: true, hasFailed: false }
    renderPage('/search?q=security')

    expect(screen.getByRole('tab', { name: /Projects/ }).textContent).toBe('Projects')
    expect(screen.getByText('Loading…', { selector: 'p' })).toBeTruthy()
  })

  it('says when the records could not be searched', () => {
    records.answer = { results: [], isLoading: false, hasFailed: true }
    renderPage('/search?q=security')

    expect(screen.getByText(/unavailable right now/).getAttribute('role')).toBe('status')
  })
})
