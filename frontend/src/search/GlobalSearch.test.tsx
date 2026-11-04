// Copyright © 2026 Jalapeno Labs

import type { ElysiaMessage } from '../api/routes/elysiaRoutes'
import type { SearchSourceAnswer } from './searchTypes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'

// Redux
import { createAppStore } from '../store'
import { meLoaded } from '../store/authSlice'
import { elysiaPanelToggled, elysiaPanelViewChanged } from '../store/elysiaSlice'

// User interface
import { LuFolderKanban } from 'react-icons/lu'
import { ConfirmGate } from '../gates/ConfirmGate'
import { GlobalSearch } from './GlobalSearch'

// Misc
import { sendElysiaMessage } from '../api/routes/elysiaRoutes'
import { makeUser } from '../testFixtures'
import { TestAriaRouter } from '../testRouter'

vi.mock('../api/routes/elysiaRoutes', () => ({
  sendElysiaMessage: vi.fn(),
}))

// What the API's records answer, set by each test: the hook's own waiting and cancelling are
// tested in `useRecordSearch.test.ts`, so here it answers at once.
const records = vi.hoisted(() => ({
  answer: { results: [], isLoading: false, hasFailed: false } as SearchSourceAnswer,
}))
vi.mock('./useRecordSearch', () => ({
  useRecordSearch: () => records.answer,
}))

// What the knowledge base answers, set by each test as the records are; `useDebouncedRequest`
// and the knowledge search's own tests cover how it asks.
const knowledge = vi.hoisted(() => ({
  answer: { results: [], isLoading: false, hasFailed: false } as SearchSourceAnswer,
}))
vi.mock('./useKnowledgeResults', () => ({
  useKnowledgeResults: () => knowledge.answer,
}))

beforeEach(() => {
  records.answer = { results: [], isLoading: false, hasFailed: false }
  knowledge.answer = { results: [], isLoading: false, hasFailed: false }
  vi.mocked(sendElysiaMessage).mockReset()
})

function CurrentPath() {
  const location = useLocation()
  return <output aria-label='path'>{`${location.pathname}${location.search}`}</output>
}

function renderSearch() {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser({ role: 'admin' }),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  // Start from a panel that is closed on her activity, so asking her has to change both.
  store.dispatch(elysiaPanelToggled(false))
  store.dispatch(elysiaPanelViewChanged('activity'))
  render(<Provider store={store}>
    <MemoryRouter initialEntries={[ '/projects' ]}>
      <TestAriaRouter>
        <ConfirmGate>
          <GlobalSearch />
          <input aria-label='Another field' />
          <p>Elsewhere on the page</p>
          <Routes>
            <Route path='*' element={<CurrentPath />} />
          </Routes>
        </ConfirmGate>
      </TestAriaRouter>
    </MemoryRouter>
  </Provider>)
  return store
}

// Hidden ones too: a dialog an action opens hides the rest of the page from the tree.
function field() {
  return screen.getByRole('searchbox', { name: 'Search Elysium', hidden: true }) as HTMLInputElement
}

function type(text: string) {
  act(() => field().focus())
  fireEvent.change(field(), { target: { value: text }})
}

function resultTitles() {
  return within(screen.getByRole('menu')).getAllByRole('menuitem').map((item) => item.textContent)
}

describe('GlobalSearch', () => {
  it('focuses on /, unless a field already holds the keyboard', () => {
    renderSearch()

    fireEvent.keyDown(document.body, { key: '/' })
    expect(document.activeElement).toBe(field())

    const other = screen.getByRole('textbox', { name: 'Another field' })
    act(() => other.focus())
    const typed = fireEvent.keyDown(other, { key: '/' })
    expect(typed).toBe(true)
    expect(document.activeElement).toBe(other)
  })

  it('shows nothing until something is typed', () => {
    renderSearch()
    act(() => field().focus())
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('lists the closest five with their icons, then View all and Ask Elysia', () => {
    renderSearch()
    type('settings')

    const menu = screen.getByRole('menu')
    const items = within(menu).getAllByRole('menuitem')
    expect(items).toHaveLength(7)
    expect(items[0].textContent).toBe('Settings')
    for (const item of items) {
      expect(item.querySelector('svg')).not.toBeNull()
    }
    expect(items.at(-2)?.textContent).toBe('View all results')
    expect(items.at(-1)?.textContent).toBe('Ask Elysia about “settings”')
  })

  it('names each section and finds pages by their hidden tags', () => {
    renderSearch()
    type('2fa')

    expect(within(screen.getByRole('menu')).getByText('Pages')).toBeTruthy()
    expect(resultTitles()).toContain('Two-factor authenticationSecurity')
  })

  it('makes every result that goes somewhere a real link, View all included', () => {
    renderSearch()
    type('security')

    const security = within(screen.getByRole('menu')).getAllByRole('menuitem')[0]
    expect(security.tagName).toBe('A')
    expect(security.getAttribute('href')).toBe('/settings/security')
    const viewAll = within(screen.getByRole('menu')).getByRole('menuitem', { name: 'View all results' })
    expect(viewAll.getAttribute('href')).toBe('/search?q=security')
  })

  it('opens a result and clears itself', () => {
    renderSearch()
    type('security')

    fireEvent.click(within(screen.getByRole('menu')).getAllByRole('menuitem')[0])
    expect(screen.getByLabelText('path').textContent).toBe('/settings/security')
    expect(field().value).toBe('')
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('runs an action in place, such as Sign out asking first', async () => {
    renderSearch()
    type('sign out')

    fireEvent.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: 'Sign out' }))
    expect(await screen.findByRole('alertdialog')).toBeTruthy()
    expect(field().value).toBe('')
  })

  it('says when nothing matches, and still offers Elysia', () => {
    renderSearch()
    type('zzzz')

    expect(screen.getByText('No results for “zzzz”').getAttribute('role')).toBe('status')
    expect(resultTitles()).toEqual([ 'Ask Elysia about “zzzz”' ])
  })

  it('lists the records the API found in sections of their own, as real links', () => {
    records.answer = {
      isLoading: false,
      hasFailed: false,
      results: [{
        id: 'records:project:p1',
        group: 'projects',
        title: 'Billing migration',
        icon: LuFolderKanban,
        score: 4,
        target: { kind: 'link', href: '/projects/p1' },
      }],
    }
    renderSearch()
    type('migration')

    const menu = screen.getByRole('menu')
    expect(within(menu).getByText('Projects')).toBeTruthy()
    const project = within(menu).getByRole('menuitem', { name: 'Billing migration' })
    expect(project.getAttribute('href')).toBe('/projects/p1')
  })

  it('lists the knowledge base\'s documents in a section of their own, opening each document', () => {
    knowledge.answer = {
      isLoading: false,
      hasFailed: false,
      results: [{
        id: 'knowledge:d1',
        group: 'knowledge',
        title: '#web',
        detail: 'Slack · Maya Chen',
        icon: LuFolderKanban,
        score: 0,
        sourceRank: 0,
        target: { kind: 'link', href: '/knowledge/documents/d1?passage=0' },
      }],
    }
    renderSearch()
    type('safari login loop')

    const menu = screen.getByRole('menu')
    expect(within(menu).getByText('Knowledge')).toBeTruthy()
    const document = within(menu).getByRole('menuitem', { name: /#web/ })
    expect(document.getAttribute('href')).toBe('/knowledge/documents/d1?passage=0')
  })

  it('says when the records could not be searched, rather than that nothing matched', () => {
    records.answer = { results: [], isLoading: false, hasFailed: true }
    renderSearch()
    type('zzzz')

    expect(screen.getByText(/unavailable right now/).getAttribute('role')).toBe('status')
    expect(screen.queryByText(/No results/)).toBeNull()
  })

  it('shows its loading box and spinner while a source loads', () => {
    records.answer = { results: [], isLoading: true, hasFailed: false }
    renderSearch()
    type('settings')

    // The loading box in the results, and the spinner in place of the magnifying glass.
    expect(screen.getByText('Loading…').getAttribute('role')).toBe('status')
    expect(screen.getByRole('status', { name: 'Loading…' }).getAttribute('data-slot')).toBe('spinner')
    expect(screen.queryByText(/No results/)).toBeNull()
  })

  it('hands the question to Elysia on her conversation, then clears', async () => {
    const message: ElysiaMessage = {
      id: 'message',
      body: 'what is due today',
      fromElysia: false,
      createdBy: 'person',
      createdAt: '2026-11-04T00:00:00Z',
    }
    vi.mocked(sendElysiaMessage).mockResolvedValue({ message })
    const store = renderSearch()
    type('what is due today')

    fireEvent.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: /Ask Elysia/ }))
    expect(sendElysiaMessage).toHaveBeenCalledWith('what is due today')
    await waitFor(() => expect(field().value).toBe(''))
    expect(store.getState().elysia.isPanelOpen).toBe(true)
    expect(store.getState().elysia.panelView).toBe('conversation')
    expect(store.getState().elysia.messages.ids).toEqual([ 'message' ])
  })

  it('keeps the question typed when it could not be sent', async () => {
    vi.mocked(sendElysiaMessage).mockRejectedValue(new Error('offline'))
    renderSearch()
    type('what is due today')

    fireEvent.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: /Ask Elysia/ }))
    await waitFor(() => expect(sendElysiaMessage).toHaveBeenCalled())
    await act(async () => {})
    expect(field().value).toBe('what is due today')
  })

  it('clears when anywhere else is clicked', () => {
    renderSearch()
    type('settings')

    const elsewhere = screen.getByText('Elsewhere on the page')
    fireEvent.pointerDown(elsewhere)
    fireEvent.click(elsewhere)
    expect(field().value).toBe('')
  })

  it('clears with Esc, then leaves search on the next Esc without the page seeing it', () => {
    renderSearch()
    type('settings')

    // A page goes up a level on an Esc nobody used (`readPageShortcut`); both of these are used.
    const clearReachedPage = fireEvent.keyDown(field(), { key: 'Escape' })
    expect(clearReachedPage).toBe(false)
    expect(field().value).toBe('')

    const leaveReachedPage = fireEvent.keyDown(field(), { key: 'Escape' })
    expect(leaveReachedPage).toBe(false)
    expect(document.activeElement).not.toBe(field())
  })
})
