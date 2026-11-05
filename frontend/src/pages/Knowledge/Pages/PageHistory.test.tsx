// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage, PageHistoryEntry, PageVersion } from '../../../api/routes/pageRoutes'

// Core
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { pagesLoaded } from '../../../store/pagesSlice'
import { usersLoaded } from '../../../store/usersSlice'

// User interface
import { PageHistory } from './PageHistory'

// Misc
import { makePage, makePageHistoryEntry, makePageSummary, makePageVersion, makeUser } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import {
  getPageCompareUrl,
  getPageHistoryUrl,
  getPageVersionUrl,
  UrlTree,
} from '../../../urls'

// The requests are the network boundary; the loaders' data arrives through Redux.
const routes = vi.hoisted(() => ({
  listPageHistory: vi.fn(),
  getPageVersion: vi.fn(),
}))

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  listPageHistory: routes.listPageHistory,
  getPageVersion: routes.getPageVersion,
}))

vi.mock('../../../hooks/useServerData', () => ({
  useUsersLoader: () => 'loaded',
}))

vi.mock('../../../hooks/useLearningCategories', () => ({
  useLearningCategories: () => ({ labels: { decision: 'Decision', convention: 'Convention' }}),
}))

vi.mock('../../../components/markdown/Markdown', () => ({
  Markdown: (props: { markdown: string }) => <div>{props.markdown}</div>,
}))

const PAGE_ID = 'page-1'

function published(version: number, overrides: Partial<PageHistoryEntry> = {}) {
  return makePageHistoryEntry({ id: `entry-${version}`, data: { version }, ...overrides })
}

function renderHistory(page: KnowledgePage, search = '') {
  const store = createAppStore()
  store.dispatch(usersLoaded([ makeUser({ id: 'user-1', name: 'Grace Hopper' }) ]))
  store.dispatch(pagesLoaded([ makePageSummary({ id: 'parent', title: 'Handbook' }) ]))

  const router = createMemoryRouter(
    [{
      path: UrlTree.knowledgePage,
      element: <TestAriaRouter>
        <PageHistory page={page} />
      </TestAriaRouter>,
    }],
    { initialEntries: [ `${getPageHistoryUrl(page.id)}${search}` ]},
  )

  render(<SWRConfig value={{ provider: () => new Map(), dedupingInterval: 0 }}>
    <Provider store={store}>
      <RouterProvider router={router} />
    </Provider>
  </SWRConfig>)
}

function respondWithVersions(versions: PageVersion[]) {
  routes.getPageVersion.mockImplementation(async (_pageId: string, version: number) => {
    const found = versions.find((candidate) => candidate.version === version)
    if (!found) {
      throw new Error(`No version ${version}`)
    }
    return { version: found }
  })
}

describe('PageHistory', () => {
  beforeEach(() => {
    routes.listPageHistory.mockReset()
    routes.getPageVersion.mockReset()
  })

  describe('the list', () => {
    it('says who published each version, the agent acting for them, and everything between', async () => {
      routes.listPageHistory.mockResolvedValue({
        history: [
          makePageHistoryEntry({ id: 'created', kind: 'created', data: { parentId: null, version: 1 }}),
          makePageHistoryEntry({
            id: 'moved',
            kind: 'moved',
            data: { from: { parentId: null, after: null }, to: { parentId: 'parent', after: null }},
          }),
          published(2, { agent: { id: 'mcp:client', name: 'Claude Code' }}),
        ],
      })

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 2 }))

      expect(await screen.findByText('Grace Hopper through Claude Code published this version')).toBeDefined()
      expect(screen.getByText('Grace Hopper moved this page from the top of the tree to Handbook')).toBeDefined()
      expect(screen.getByText('Grace Hopper created this page')).toBeDefined()
    })

    it('opens a version on what it changed, and the first on what it said', async () => {
      routes.listPageHistory.mockResolvedValue({
        history: [ published(1), published(2) ],
      })

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 2 }))

      const second = await screen.findByRole('link', { name: 'Version 2' })
      expect(second.getAttribute('href')).toBe(getPageCompareUrl(PAGE_ID, 1, 2))
      expect(screen.getByRole('link', { name: 'Version 1' }).getAttribute('href'))
        .toBe(getPageVersionUrl(PAGE_ID, 1))
    })

    it('compares the two versions picked, a third replacing the earlier pick', async () => {
      routes.listPageHistory.mockResolvedValue({
        history: [ published(1), published(2), published(3) ],
      })

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 3 }))

      const compareLink = () => screen.queryByRole('link', { name: 'Compare' })
      fireEvent.click(await screen.findByRole('checkbox', { name: 'Pick version 1 to compare' }))
      expect(screen.getByText('Pick one more version')).toBeDefined()
      expect(compareLink()).toBeNull()

      fireEvent.click(screen.getByRole('checkbox', { name: 'Pick version 3 to compare' }))
      expect(compareLink()?.getAttribute('href')).toBe(getPageCompareUrl(PAGE_ID, 1, 3))

      fireEvent.click(screen.getByRole('checkbox', { name: 'Pick version 2 to compare' }))
      expect(screen.getByText('Version 3 and version 2')).toBeDefined()
      expect(screen.getByRole<HTMLInputElement>('checkbox', { name: 'Pick version 1 to compare' }).checked)
        .toBe(false)
    })

    it('offers nothing to compare on a page with one version', async () => {
      routes.listPageHistory.mockResolvedValue({ history: [ published(1) ]})

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 1 }))

      await screen.findByRole('link', { name: 'Version 1' })
      expect(screen.queryByRole('checkbox')).toBeNull()
      expect(screen.queryByText('Pick two versions to compare')).toBeNull()
    })

    it('draws a long history a stretch at a time', async () => {
      const history = Array.from({ length: 45 }, (_, index) => published(index + 1))
      routes.listPageHistory.mockResolvedValue({ history })

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 45 }))

      const list = await screen.findByRole('list')
      expect(within(list).getAllByRole('listitem')).toHaveLength(30)
      expect(screen.queryByRole('link', { name: 'Version 15' })).toBeNull()

      fireEvent.click(screen.getByRole('button', { name: 'Show older' }))

      expect(within(list).getAllByRole('listitem')).toHaveLength(45)
      expect(screen.queryByRole('button', { name: 'Show older' })).toBeNull()
    })

    it('offers Restore on every version but the current one', async () => {
      routes.listPageHistory.mockResolvedValue({ history: [ published(1), published(2) ]})

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 2 }))

      expect(await screen.findByRole('button', { name: 'Restore version 1' })).toBeDefined()
      expect(screen.queryByRole('button', { name: 'Restore version 2' })).toBeNull()
    })

    it('keeps Restore off a deleted page, which refuses it', async () => {
      routes.listPageHistory.mockResolvedValue({ history: [ published(1), published(2) ]})

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 2, deletedAt: '2026-11-02T12:00:00Z' }))

      await screen.findByRole('link', { name: 'Version 1' })
      expect(screen.queryByRole('button', { name: 'Restore version 1' })).toBeNull()
    })

    it('reads a move to a parent deleted since', async () => {
      routes.listPageHistory.mockResolvedValue({
        history: [
          makePageHistoryEntry({
            id: 'moved',
            kind: 'moved',
            data: { from: { parentId: 'parent', after: null }, to: { parentId: 'gone', after: null }},
          }),
        ],
      })

      renderHistory(makePage({ id: PAGE_ID }))

      expect(await screen.findByText('Grace Hopper moved this page from Handbook to a deleted page')).toBeDefined()
    })

    it('says when the history cannot be read', async () => {
      routes.listPageHistory.mockRejectedValue(new Error('down'))

      renderHistory(makePage({ id: PAGE_ID }))

      expect(await screen.findByText('This page\'s history can\'t be loaded right now.')).toBeDefined()
    })
  })

  describe('comparing', () => {
    it('loads the two versions alone and shows what changed', async () => {
      respondWithVersions([
        makePageVersion({ pageId: PAGE_ID, version: 1, body: 'Before' }),
        makePageVersion({ pageId: PAGE_ID, version: 2, body: 'After' }),
      ])

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 3 }), '&from=1&to=2')

      const diff = await screen.findByRole('group', { name: 'Text' })
      expect(within(diff).getByText('Before')).toBeDefined()
      expect(within(diff).getByText('After')).toBeDefined()
      expect(routes.getPageVersion).toHaveBeenCalledTimes(2)
      expect(routes.listPageHistory).not.toHaveBeenCalled()
    })

    it('shows a learning\'s changes to more than its text', async () => {
      const shared = { pageId: PAGE_ID, title: 'A learning', body: 'Same' }
      respondWithVersions([
        makePageVersion({ ...shared, version: 1, category: 'decision', evidence: 'Seen once' }),
        makePageVersion({ ...shared, version: 2, category: 'convention', evidence: 'Seen twice' }),
      ])

      renderHistory(makePage({ id: PAGE_ID, title: 'A learning', currentVersion: 2 }), '&from=1&to=2')

      expect(await screen.findByRole('group', { name: 'Evidence' })).toBeDefined()
      expect(within(screen.getByRole('group', { name: 'Category' })).getByText('Decision')).toBeDefined()
      expect(screen.queryByRole('group', { name: 'Text' })).toBeNull()
    })

    it('says when two versions hold the same', async () => {
      respondWithVersions([
        makePageVersion({ pageId: PAGE_ID, version: 1 }),
        makePageVersion({ pageId: PAGE_ID, version: 2 }),
      ])

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 2 }), '&from=1&to=2')

      expect(await screen.findByText('These versions say the same thing.')).toBeDefined()
    })

    it('says when a version does not exist, with the way back', async () => {
      respondWithVersions([ makePageVersion({ pageId: PAGE_ID, version: 1 }) ])

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 1 }), '&from=1&to=9')

      expect(await screen.findByText('This version isn\'t available.')).toBeDefined()
      expect(screen.getByRole('link', { name: 'All versions' }).getAttribute('href'))
        .toBe(getPageHistoryUrl(PAGE_ID))
    })
  })

  describe('reading', () => {
    it('reads a version with who published it and what it was called', async () => {
      respondWithVersions([ makePageVersion({
        pageId: PAGE_ID,
        version: 2,
        title: 'The old title',
        body: 'What it said then',
        agent: { id: 'session:7', name: null },
      }) ])

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 3 }), '&version=2')

      expect(await screen.findByText('What it said then')).toBeDefined()
      expect(screen.getByText('The old title')).toBeDefined()
      expect(screen.getByText('Last edited by Grace Hopper through session 7')).toBeDefined()
      expect(screen.getByRole('button', { name: 'Restore version 2' })).toBeDefined()
      expect(screen.getByRole('link', { name: 'Changes' }).getAttribute('href'))
        .toBe(getPageCompareUrl(PAGE_ID, 1, 2))
    })

    it('reads a learning\'s evidence as it was', async () => {
      respondWithVersions([ makePageVersion({
        pageId: PAGE_ID,
        version: 1,
        category: 'decision',
        evidence: 'Seen once',
      }) ])

      renderHistory(makePage({ id: PAGE_ID, currentVersion: 2 }), '&version=1')

      expect(await screen.findByText('Seen once')).toBeDefined()
      await waitFor(() => expect(screen.getByText('Decision')).toBeDefined())
    })
  })
})
