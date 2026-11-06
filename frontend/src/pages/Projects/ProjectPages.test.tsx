// Copyright © 2026 Jalapeno Labs

import type { ProjectPage } from '../../api/routes/pageRoutes'
import type { LoadStatus } from '../../hooks/useServerData'

// Core
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { mutate } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../store'
import { pagesLoaded } from '../../store/pagesSlice'
import { projectsLoaded } from '../../store/projectsSlice'

// User interface
import { ProjectPages } from './ProjectPages'

// Misc
import { linkPageToProject, projectPagesKey, unlinkPageFromProject } from '../../api/routes/pageRoutes'
import { makePage, makePageSummary, makeProject } from '../../testFixtures'
import { TestAriaRouter } from '../../testRouter'
import { toast } from '../../toast/toast'
import { getKnowledgePageUrl } from '../../urls'

vi.mock('../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/pageRoutes')>(),
  linkPageToProject: vi.fn(),
  unlinkPageFromProject: vi.fn(),
}))

vi.mock('../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../hooks/useServerData')>(),
  usePagesLoader: () => 'loaded',
  useUsersLoader: () => 'loaded',
}))

vi.mock('../../toast/toast', () => ({
  toast: { success: vi.fn(), danger: vi.fn() },
}))

vi.mock('swr', async (importOriginal) => ({
  ...await importOriginal<typeof import('swr')>(),
  mutate: vi.fn(),
}))

// jsdom has no ResizeObserver, which the kit's table measures with.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const CHECKLIST = {
  ...makePageSummary({ id: 'checklist', title: 'Checklist', parentId: 'runbooks', labels: [ 'on-call' ]}),
  publishedBy: 'person',
  publishedAt: '2026-11-01T12:00:00Z',
  linkedBy: 'person',
  linkedAt: '2026-11-02T12:00:00Z',
} as const satisfies ProjectPage

function renderDocs(pages: ProjectPage[], status: LoadStatus = 'loaded') {
  const store = createAppStore()
  store.dispatch(projectsLoaded([ makeProject({ id: 'launch', name: 'Launch' }) ]))
  store.dispatch(pagesLoaded([
    makePageSummary({ id: 'runbooks', title: 'Runbooks' }),
    makePageSummary({ id: 'checklist', title: 'Checklist', parentId: 'runbooks' }),
    makePageSummary({ id: 'rollback', title: 'Rollback', parentId: 'runbooks', position: 1 }),
  ]))
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={store}>
      <TestAriaRouter>
        <ProjectPages projectId='launch' status={status} pages={pages} />
      </TestAriaRouter>
    </Provider>,
  }])
  render(<RouterProvider router={router} />)
}

function openPicker() {
  const [ trigger ] = screen.getAllByRole('button', { name: /Link a page/ })
  fireEvent.click(trigger)
}

describe('ProjectPages', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.mocked(linkPageToProject).mockReset()
    vi.mocked(unlinkPageFromProject).mockReset()
    vi.mocked(toast.success).mockReset()
    vi.mocked(mutate).mockReset()
  })

  it('opens each linked page, and says where it sits', () => {
    renderDocs([ CHECKLIST ])

    expect(screen.getByRole('link', { name: 'Checklist' }).getAttribute('href'))
      .toBe(getKnowledgePageUrl('checklist'))
    expect(within(screen.getByRole('row', { name: /Checklist/ })).getByText('Runbooks')).toBeTruthy()
    expect(screen.getByRole('link', { name: 'on-call' })).toBeTruthy()
  })

  it('offers the pages not linked yet, by their path, and links the one picked', async () => {
    vi.mocked(linkPageToProject).mockResolvedValue({ page: makePage({ id: 'rollback', projectIds: [ 'launch' ]}) })
    renderDocs([ CHECKLIST ])

    openPicker()
    const option = await screen.findByRole('option', { name: /Rollback/ })
    expect(option.textContent).toContain('Runbooks')
    expect(screen.queryByRole('option', { name: /Checklist/ })).toBeNull()

    fireEvent.click(option)

    await waitFor(() => expect(linkPageToProject).toHaveBeenCalledWith('rollback', 'launch'))
    await waitFor(() => expect(mutate).toHaveBeenCalledWith(projectPagesKey('launch')))
    expect(toast.success).toHaveBeenCalledWith('Linked Rollback to Launch', expect.anything())
  })

  it('says so when nothing is linked yet, and still offers the tree', async () => {
    renderDocs([])

    expect(screen.getByText('No pages are linked to this project yet.')).toBeTruthy()

    openPicker()
    expect(await screen.findByRole('option', { name: /Checklist/ })).toBeTruthy()
  })

  it('says so when the docs cannot be read', () => {
    renderDocs([], 'failed')

    expect(screen.getByText('This project\'s docs can\'t be loaded right now.')).toBeTruthy()
  })
})
