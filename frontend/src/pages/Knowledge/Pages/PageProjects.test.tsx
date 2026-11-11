// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { HTTPError } from 'ky'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { mutate } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { projectsLoaded } from '../../../store/projectsSlice'

// User interface
import { PageProjects } from './PageProjects'

// Misc
import { linkPageToProject, projectPagesKey, unlinkPageFromProject } from '../../../api/routes/pageRoutes'
import { makePage, makeProject } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { toast } from '../../../toast/toast'
import { getProjectDocsUrl } from '../../../urls'

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  linkPageToProject: vi.fn(),
  unlinkPageFromProject: vi.fn(),
}))

vi.mock('../../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../hooks/useServerData')>(),
  useProjectsLoader: () => 'loaded',
}))

vi.mock('../../../toast/toast', () => ({
  toast: { success: vi.fn(), danger: vi.fn() },
}))

vi.mock('swr', async (importOriginal) => ({
  ...await importOriginal<typeof import('swr')>(),
  mutate: vi.fn(),
}))

function renderProjects(page: KnowledgePage) {
  const store = createAppStore()
  store.dispatch(projectsLoaded([
    makeProject({ id: 'launch', name: 'Launch' }),
    makeProject({ id: 'website', name: 'Website' }),
  ]))
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={store}>
      <TestAriaRouter>
        <PageProjects page={page} />
      </TestAriaRouter>
    </Provider>,
  }])
  render(<RouterProvider router={router} />)
  return store
}

function openPicker() {
  const [ trigger ] = screen.getAllByRole('button', { name: /Add to project/ })
  fireEvent.click(trigger)
}

async function pickProject(name: string) {
  openPicker()
  fireEvent.click(await screen.findByRole('option', { name }))
}

describe('PageProjects', () => {
  beforeEach(() => {
    vi.mocked(linkPageToProject).mockReset()
    vi.mocked(unlinkPageFromProject).mockReset()
    vi.mocked(toast.success).mockReset()
    vi.mocked(toast.danger).mockReset()
    vi.mocked(mutate).mockReset()
  })

  it('opens each project\'s docs by its name', () => {
    renderProjects(makePage({ id: 'page', projectIds: [ 'launch' ]}))

    expect(screen.getByRole('link', { name: 'Launch' }).getAttribute('href'))
      .toBe(getProjectDocsUrl('launch'))
  })

  it('keeps a project\'s brief on its project, and lets it be taken off any other', () => {
    renderProjects(makePage({ id: 'brief', briefProjectId: 'launch', projectIds: [ 'launch', 'website' ]}))

    expect(screen.queryByRole('button', { name: 'Take off Launch' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Take off Website' })).toBeTruthy()
  })

  it('offers the projects the page is not on, and links the one picked', async () => {
    const page = makePage({ id: 'page', title: 'Checklist', projectIds: [ 'website' ]})
    vi.mocked(linkPageToProject).mockResolvedValue({
      page: { ...page, projectIds: [ 'website', 'launch' ]},
    })
    const store = renderProjects(page)

    openPicker()
    const option = await screen.findByRole('option', { name: 'Launch' })
    expect(screen.queryByRole('option', { name: 'Website' })).toBeNull()

    fireEvent.click(option)

    await waitFor(() => expect(linkPageToProject).toHaveBeenCalledWith('page', 'launch'))
    await waitFor(() => expect(store.getState().pages.openedById.page?.projectIds)
      .toEqual([ 'website', 'launch' ]))
    expect(mutate).toHaveBeenCalledWith(projectPagesKey('launch'))
    expect(toast.success).toHaveBeenCalledWith('Linked Checklist to Launch', expect.anything())
  })

  it('takes a page off a project, and Undo in the toast links it again', async () => {
    const page = makePage({ id: 'page', title: 'Checklist', projectIds: [ 'launch' ]})
    vi.mocked(unlinkPageFromProject).mockResolvedValue({ page: { ...page, projectIds: []}})
    vi.mocked(linkPageToProject).mockResolvedValue({ page })
    renderProjects(page)

    fireEvent.click(screen.getByRole('button', { name: 'Take off Launch' }))

    await waitFor(() => expect(unlinkPageFromProject).toHaveBeenCalledWith('page', 'launch'))
    await waitFor(() => expect(toast.success).toHaveBeenCalled())
    const [ message, options ] = vi.mocked(toast.success).mock.calls[0]
    expect(message).toBe('Took Checklist off Launch')

    options?.action?.onPress()

    await waitFor(() => expect(linkPageToProject).toHaveBeenCalledWith('page', 'launch'))
  })

  it('says why a link was refused', async () => {
    vi.mocked(linkPageToProject).mockRejectedValue(new HTTPError(
      new Response(null, { status: 409 }),
      new Request('http://localhost/'),
      {} as never,
    ))
    renderProjects(makePage({ id: 'page' }))

    await pickProject('Launch')

    await waitFor(() => expect(toast.danger).toHaveBeenCalledWith('The page wasn\'t linked', expect.anything()))
    expect(toast.success).not.toHaveBeenCalled()
  })

  it('only reads a deleted page\'s projects, and takes no space without any', () => {
    const deletedAt = '2026-11-02T12:00:00Z'
    renderProjects(makePage({ id: 'page', deletedAt, projectIds: [ 'launch' ]}))

    expect(screen.getByRole('link', { name: 'Launch' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Take off Launch' })).toBeNull()
    expect(screen.queryByRole('button', { name: /Add to project/ })).toBeNull()
  })
})
