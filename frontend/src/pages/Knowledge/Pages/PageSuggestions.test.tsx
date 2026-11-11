// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage, PageSuggestion } from '../../../api/routes/pageRoutes'

// Core
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { usersLoaded } from '../../../store/usersSlice'

// User interface
import { PromptGate } from '../../../gates/PromptGate'
import { PageSuggestions } from './PageSuggestions'

// Misc
import { makePage, makeUser } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { getKnowledgePageUrl, PAGE_SUGGESTIONS_TAB, TAB_PARAM, UrlTree } from '../../../urls'

// The requests are the network boundary.
const routes = vi.hoisted(() => ({
  listPageSuggestions: vi.fn(),
  acceptPageSuggestion: vi.fn(),
  dismissPageSuggestion: vi.fn(),
}))

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  listPageSuggestions: routes.listPageSuggestions,
  acceptPageSuggestion: routes.acceptPageSuggestion,
  dismissPageSuggestion: routes.dismissPageSuggestion,
}))

vi.mock('../../../hooks/useServerData', () => ({
  useUsersLoader: () => 'loaded',
}))

const PAGE_ID = 'page-1'

function suggestion(overrides: Partial<PageSuggestion> = {}): PageSuggestion {
  return {
    id: 'suggestion-1',
    pageId: PAGE_ID,
    baseVersion: 2,
    changes: [
      { index: 0, line: 1, before: '- Priya Ramen', after: '- Priya Raman', applies: false },
      { index: 1, line: 3, before: '- Tom', after: '- Tom\n- Uma', applies: true },
    ],
    reason: 'Heard in the kickoff.',
    quote: 'Uma joins Monday.',
    source: 'Kickoff, November 3rd',
    workflowRunId: null,
    state: 'pending',
    acceptedChanges: null,
    dismissalReason: null,
    decidedBy: null,
    decidedAt: null,
    createdBy: 'elysia',
    agent: null,
    createdAt: '2026-11-03T09:00:00Z',
    ...overrides,
  }
}

function renderSuggestions(page: KnowledgePage) {
  const store = createAppStore()
  store.dispatch(usersLoaded([ makeUser({ id: 'elysia', name: 'Elysia' }) ]))
  const router = createMemoryRouter(
    [{
      path: UrlTree.knowledgePage,
      element: <TestAriaRouter>
        <PromptGate>
          <PageSuggestions page={page} />
        </PromptGate>
      </TestAriaRouter>,
    }],
    { initialEntries: [ `${getKnowledgePageUrl(page.id)}?${TAB_PARAM}=${PAGE_SUGGESTIONS_TAB}` ]},
  )
  render(<SWRConfig value={{ provider: () => new Map(), dedupingInterval: 0 }}>
    <Provider store={store}>
      <RouterProvider router={router} />
    </Provider>
  </SWRConfig>)
}

describe('PageSuggestions', () => {
  beforeEach(() => {
    routes.listPageSuggestions.mockReset()
    routes.acceptPageSuggestion.mockReset()
    routes.dismissPageSuggestion.mockReset()
  })

  it('says so when nothing waits for review', async () => {
    routes.listPageSuggestions.mockResolvedValue({ suggestions: []})
    renderSuggestions(makePage({ id: PAGE_ID, currentVersion: 2 }))

    expect(await screen.findByText('No suggested edits wait for review.')).toBeTruthy()
  })

  it('takes only the changes that still merge, and accepts those it is asked to', async () => {
    const page = makePage({ id: PAGE_ID, currentVersion: 3, body: '- Priya Raman\n- Tom\n- Uma' })
    routes.listPageSuggestions.mockResolvedValue({ suggestions: [ suggestion() ]})
    routes.acceptPageSuggestion.mockResolvedValue({ suggestion: suggestion({ state: 'accepted' }), page })
    renderSuggestions(page)

    expect(await screen.findByText('Heard in the kickoff.')).toBeTruthy()
    expect(screen.getByText('Written against version 2. Accepting merges it with what changed since.'))
      .toBeTruthy()
    expect(screen.getByText(/The page changed in these lines since/)).toBeTruthy()
    const changedSince = screen.getByRole('checkbox', { name: 'Take the change at line 1' })
    expect((changedSince as HTMLInputElement).disabled).toBe(true)
    expect((changedSince as HTMLInputElement).checked).toBe(false)

    fireEvent.click(screen.getByText('Accept 1 of 2').closest('button') as HTMLElement)

    await waitFor(() => expect(routes.acceptPageSuggestion).toHaveBeenCalledWith(PAGE_ID, 'suggestion-1', [ 1 ]))
    await waitFor(() => expect(routes.listPageSuggestions).toHaveBeenCalledTimes(2))
  })

  it('asks why before it dismisses a suggestion', async () => {
    const page = makePage({ id: PAGE_ID, currentVersion: 2 })
    const lone = suggestion({ changes: [{ index: 0, line: 1, before: '', after: '- ACME', applies: true }]})
    routes.listPageSuggestions.mockResolvedValue({ suggestions: [ lone ]})
    routes.dismissPageSuggestion.mockResolvedValue({ suggestion: lone, page })
    renderSuggestions(page)

    expect(await screen.findByText('Accept')).toBeTruthy()
    expect(screen.queryByRole('checkbox'), 'one change needs no choice').toBeNull()
    fireEvent.click(screen.getByText('Dismiss').closest('button') as HTMLElement)
    const reason = await screen.findByLabelText(/Why/)
    fireEvent.change(reason, { target: { value: 'ACME is the client\'s old name' }})
    fireEvent.keyDown(reason, { key: 'Enter', ctrlKey: true })

    await waitFor(() => expect(routes.dismissPageSuggestion)
      .toHaveBeenCalledWith(PAGE_ID, 'suggestion-1', 'ACME is the client\'s old name'))
  })
})
