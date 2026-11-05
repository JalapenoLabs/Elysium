// Copyright © 2026 Jalapeno Labs

// Core
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { pagesLoaded } from '../../../store/pagesSlice'

// User interface
import { NewKnowledgePage } from './NewKnowledgePage'

// Misc
import { createPage } from '../../../api/routes/pageRoutes'
import { keepSessionDraft, readSessionDraft } from '../../../drafts/sessionDrafts'
import { makePage, makePageSummary } from '../../../testFixtures'
import { getKnowledgePageUrl, getNewChildPageUrl, UrlTree } from '../../../urls'

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  createPage: vi.fn(),
}))

vi.mock('../../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../hooks/useServerData')>(),
  usePagesLoader: () => 'loaded',
}))

// The editor has tests of its own; here it is the field it stands for.
vi.mock('../../../components/markdownEditor/MarkdownEditor', () => ({
  MarkdownEditor: (props: { label: string, markdown: string, onChange: (markdown: string) => void }) => <textarea
    aria-label={props.label}
    value={props.markdown}
    onChange={(event) => props.onChange(event.target.value)}
  />,
}))

const DRAFT_NAME = 'knowledge-page'

class InertObserver {
  observe() {}
  disconnect() {}
}

function renderNewPage(address: string = UrlTree.knowledgePageNew) {
  const store = createAppStore()
  store.dispatch(pagesLoaded([ makePageSummary({ id: 'handbook', title: 'Handbook' }) ]))
  const router = createMemoryRouter(
    [
      { path: UrlTree.knowledgePageNew, element: <NewKnowledgePage /> },
      { path: UrlTree.knowledgePage, element: <p>Reading</p> },
    ],
    { initialEntries: [ address ]},
  )
  render(<Provider store={store}>
    <RouterProvider router={router} />
  </Provider>)
  return router
}

// While it waits, a disabled button's tooltip wraps it in a second one by the same name.
function publishButtons() {
  return screen
    .getAllByRole('button', { name: 'Publish' })
    .filter((element) => element.tagName === 'BUTTON')
}

describe('NewKnowledgePage', () => {
  beforeEach(() => {
    vi.stubGlobal('IntersectionObserver', InertObserver)
    window.sessionStorage.clear()
    vi.mocked(createPage).mockReset()
  })

  it('offers Publish in the header and below the form, waiting for a title', () => {
    renderNewPage()

    expect(publishButtons()).toHaveLength(2)
    expect(createPage).not.toHaveBeenCalled()

    fireEvent.click(publishButtons()[0])

    expect(createPage).not.toHaveBeenCalled()
  })

  it('publishes under the parent its address names, then opens the page', async () => {
    vi.mocked(createPage).mockResolvedValue({ page: makePage({ id: 'created', title: 'Runbook' }) })
    const router = renderNewPage(getNewChildPageUrl('handbook'))

    fireEvent.change(screen.getByLabelText(/Title/u), { target: { value: '  Runbook   notes ' }})
    fireEvent.change(screen.getByRole('textbox', { name: 'Page text' }), { target: { value: 'Restart it.' }})
    fireEvent.click(publishButtons()[0])

    await waitFor(() => expect(router.state.location.pathname).toBe(getKnowledgePageUrl('created')))
    expect(createPage).toHaveBeenCalledWith({ title: 'Runbook notes', body: 'Restart it.', parentId: 'handbook' })
    expect(readSessionDraft(DRAFT_NAME)).toBeNull()
  })

  it('gives back what was typed after a reload, and Publish is ready', async () => {
    keepSessionDraft(DRAFT_NAME, { title: 'Half done', body: 'Typed before a refresh.', parentId: '' }, [ 'Half done' ])
    vi.mocked(createPage).mockResolvedValue({ page: makePage({ id: 'created' }) })

    renderNewPage()

    await waitFor(() => {
      expect(screen.getByRole('textbox', { name: 'Page text' })).toHaveProperty('value', 'Typed before a refresh.')
    })
    fireEvent.click(publishButtons()[0])

    await waitFor(() => expect(createPage).toHaveBeenCalled())
    expect(vi.mocked(createPage).mock.lastCall?.[0]).toMatchObject({ title: 'Half done', parentId: undefined })
  })

  it('lets the address choose the parent over a kept draft', async () => {
    keepSessionDraft(DRAFT_NAME, { title: 'Half done', body: '', parentId: 'elsewhere' }, [ 'Half done' ])
    vi.mocked(createPage).mockResolvedValue({ page: makePage({ id: 'created' }) })

    renderNewPage(getNewChildPageUrl('handbook'))
    await screen.findByDisplayValue('Half done')
    fireEvent.click(publishButtons()[0])

    await waitFor(() => expect(createPage).toHaveBeenCalled())
    expect(vi.mocked(createPage).mock.lastCall?.[0].parentId).toBe('handbook')
  })

  it('goes to the top of the tree when the draft\'s parent is gone', async () => {
    keepSessionDraft(DRAFT_NAME, { title: 'Half done', body: '', parentId: 'gone' }, [ 'Half done' ])
    vi.mocked(createPage).mockResolvedValue({ page: makePage({ id: 'created' }) })

    renderNewPage()
    await screen.findByDisplayValue('Half done')
    fireEvent.click(publishButtons()[0])

    await waitFor(() => expect(createPage).toHaveBeenCalled())
    expect(vi.mocked(createPage).mock.lastCall?.[0].parentId).toBeUndefined()
  })
})
