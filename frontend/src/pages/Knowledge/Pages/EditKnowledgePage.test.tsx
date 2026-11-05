// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { HTTPError } from 'ky'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { pageLoaded } from '../../../store/pagesSlice'

// User interface
import { EditKnowledgePage } from './EditKnowledgePage'

// Misc
import { getPage, getPageVersion, publishPage } from '../../../api/routes/pageRoutes'
import { keepSessionDraft } from '../../../drafts/sessionDrafts'
import { makePage } from '../../../testFixtures'
import { getKnowledgePageEditUrl, getKnowledgePageUrl, UrlTree } from '../../../urls'

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  getPage: vi.fn(),
  getPageVersion: vi.fn(),
  publishPage: vi.fn(),
}))

vi.mock('../../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../hooks/useServerData')>(),
  usePageLoader: () => 'loaded',
  useLearningCategoriesLoader: () => 'loaded',
}))

// The editor has tests of its own; here it is the field it stands for.
vi.mock('../../../components/markdownEditor/MarkdownEditor', () => ({
  MarkdownEditor: (props: { label: string, markdown: string, onChange: (markdown: string) => void }) => <textarea
    aria-label={props.label}
    value={props.markdown}
    onChange={(event) => props.onChange(event.target.value)}
  />,
}))

const PAGE_ID = 'runbook'

// jsdom has neither observer, nor the Web Animations API, which the conflict's tabs measure with.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function pageChanged() {
  return Object.assign(Object.create(HTTPError.prototype), { data: { code: 'page_changed' }})
}

function renderEditor(page: KnowledgePage) {
  const store = createAppStore()
  store.dispatch(pageLoaded(page))
  const router = createMemoryRouter(
    [
      { path: UrlTree.knowledgePageEdit, element: <EditKnowledgePage /> },
      { path: UrlTree.knowledgePage, element: <p>Reading</p> },
    ],
    { initialEntries: [ getKnowledgePageEditUrl(PAGE_ID) ]},
  )
  render(<Provider store={store}>
    <RouterProvider router={router} />
  </Provider>)
  return { store, router }
}

function typeBody(text: string) {
  fireEvent.change(screen.getByRole('textbox', { name: 'Page text' }), { target: { value: text }})
}

// While it waits, a disabled button's tooltip wraps it in a second one by the same name.
function publishButton() {
  const button = screen
    .getAllByRole('button', { name: 'Publish' })
    .find((element) => element.tagName === 'BUTTON')
  if (!button) {
    throw new Error('the page has no Publish')
  }
  return button
}

describe('EditKnowledgePage', () => {
  beforeEach(() => {
    vi.stubGlobal('IntersectionObserver', InertObserver)
    vi.stubGlobal('ResizeObserver', InertObserver)
    Element.prototype.getAnimations = () => []
    window.sessionStorage.clear()
    vi.mocked(publishPage).mockReset()
    vi.mocked(getPage).mockReset()
    vi.mocked(getPageVersion).mockReset()
  })

  it('sends nothing for a page opened and left alone', () => {
    renderEditor(makePage({ id: PAGE_ID, title: 'Runbook', body: 'Restart it.', currentVersion: 3 }))

    expect(publishButton()).toHaveProperty('disabled', true)

    fireEvent.keyDown(window, { key: 's', ctrlKey: true })

    expect(publishPage).not.toHaveBeenCalled()
  })

  it('publishes against the version it was opened at, then goes back to reading', async () => {
    const page = makePage({ id: PAGE_ID, title: 'Runbook', body: 'Restart it.', currentVersion: 3 })
    vi.mocked(publishPage).mockResolvedValue({ page: { ...page, currentVersion: 4 }, outcome: 'published' })
    const { router } = renderEditor(page)

    typeBody('Restart it twice.')
    fireEvent.click(publishButton())

    await waitFor(() => expect(router.state.location.pathname).toBe(getKnowledgePageUrl(PAGE_ID)))
    expect(publishPage).toHaveBeenCalledWith(PAGE_ID, {
      baseVersion: 3,
      title: 'Runbook',
      body: 'Restart it twice.',
    })
  })

  it('keeps the person\'s text when a newer version arrives while they edit', () => {
    const page = makePage({ id: PAGE_ID, body: 'Restart it.', currentVersion: 3 })
    const { store } = renderEditor(page)

    typeBody('Mine.')
    act(() => {
      store.dispatch(pageLoaded({ ...page, body: 'Theirs.', currentVersion: 4 }))
    })

    expect(screen.getByRole('textbox', { name: 'Page text' })).toHaveProperty('value', 'Mine.')
  })

  it('shows what changed when someone published first, and keeps the text', async () => {
    const page = makePage({ id: PAGE_ID, title: 'Runbook', body: 'Restart it.', currentVersion: 3 })
    const latest = { ...page, body: 'Restart it, then check.', currentVersion: 4 }
    vi.mocked(publishPage).mockRejectedValueOnce(pageChanged())
    vi.mocked(getPage).mockResolvedValue({ page: latest })
    vi.mocked(getPageVersion).mockResolvedValue({
      version: {
        pageId: PAGE_ID,
        version: 3,
        title: 'Runbook',
        restoredFrom: null,
        createdBy: 'user-1',
        agent: null,
        createdAt: '2026-11-01T12:00:00Z',
        body: 'Restart it.',
        category: null,
        evidence: null,
      },
    })
    const { router } = renderEditor(page)

    typeBody('My words.')
    fireEvent.click(publishButton())

    expect(await screen.findByText('This page changed since you opened it')).toBeTruthy()
    expect(getPageVersion).toHaveBeenCalledWith(PAGE_ID, 3)
    expect(screen.getByRole('group', { name: 'What changed' }).textContent).toContain('Restart it, then check.')
    expect(router.state.location.pathname).toBe(getKnowledgePageEditUrl(PAGE_ID))

    fireEvent.click(screen.getByRole('button', { name: 'Keep editing' }))
    await waitFor(() => expect(screen.queryByText('This page changed since you opened it')).toBeNull())
    expect(screen.getByRole('textbox', { name: 'Page text' })).toHaveProperty('value', 'My words.')

    // Having seen it, the next publish goes on top of it.
    vi.mocked(publishPage).mockResolvedValue({ page: { ...latest, currentVersion: 5 }, outcome: 'published' })
    fireEvent.click(publishButton())

    await waitFor(() => expect(router.state.location.pathname).toBe(getKnowledgePageUrl(PAGE_ID)))
    expect(vi.mocked(publishPage).mock.lastCall?.[1].baseVersion).toBe(4)
  })

  it('goes back to reading a page already deleted', () => {
    const { router } = renderEditor(makePage({ id: PAGE_ID, deletedAt: '2026-11-01T12:00:00Z' }))

    expect(router.state.location.pathname).toBe(getKnowledgePageUrl(PAGE_ID))
  })

  it('keeps the text of someone editing a page that is deleted meanwhile', () => {
    const page = makePage({ id: PAGE_ID, body: 'Text.' })
    const { store, router } = renderEditor(page)

    typeBody('My words.')
    act(() => {
      store.dispatch(pageLoaded({ ...page, deletedAt: '2026-11-01T12:00:00Z' }))
    })

    expect(router.state.location.pathname).toBe(getKnowledgePageEditUrl(PAGE_ID))
    expect(screen.getByRole('textbox', { name: 'Page text' })).toHaveProperty('value', 'My words.')
  })

  it('tells a refusal it knows in its own words and stays', async () => {
    const page = makePage({ id: PAGE_ID, body: 'Text.' })
    vi.mocked(publishPage).mockRejectedValue(
      Object.assign(Object.create(HTTPError.prototype), { data: { code: 'page_deleted', message: 'gone' }}),
    )
    const { router } = renderEditor(page)

    typeBody('More text.')
    fireEvent.click(publishButton())

    await waitFor(() => expect(publishPage).toHaveBeenCalled())
    expect(router.state.location.pathname).toBe(getKnowledgePageEditUrl(PAGE_ID))
    expect(screen.getByRole('textbox', { name: 'Page text' })).toHaveProperty('value', 'More text.')
  })

  it('keeps a learning\'s category and evidence', async () => {
    const page = makePage({
      id: PAGE_ID,
      title: 'Lint first',
      body: 'Run the linter.',
      category: 'convention',
      evidence: 'WAY-123.',
    })
    vi.mocked(publishPage).mockResolvedValue({ page, outcome: 'published' })
    renderEditor(page)

    typeBody('Run the linter first.')
    fireEvent.click(publishButton())

    await waitFor(() => expect(publishPage).toHaveBeenCalled())
    expect(vi.mocked(publishPage).mock.lastCall?.[1]).toMatchObject({
      category: 'convention',
      evidence: 'WAY-123.',
      body: 'Run the linter first.',
    })
  })

  it('leaves a fixed page\'s title as it is', () => {
    renderEditor(makePage({ id: PAGE_ID, title: 'Glossary', fixed: true, proposalOnly: true }))

    expect(screen.getByRole('heading', { name: 'Glossary' })).toBeTruthy()
    expect(screen.queryByLabelText('Title')).toBeNull()
  })

  it('brings back text kept through a reload, with the version it was edited from', async () => {
    const page = makePage({ id: PAGE_ID, title: 'Runbook', body: 'Newer text.', currentVersion: 7 })
    keepSessionDraft(
      `knowledge-page.${PAGE_ID}`,
      { title: 'Runbook', body: 'Kept.', evidence: '', category: '', baseVersion: 5 },
      [ 'Kept.' ],
    )
    vi.mocked(publishPage).mockResolvedValue({ page, outcome: 'published' })
    renderEditor(page)

    await waitFor(() => {
      expect(screen.getByRole('textbox', { name: 'Page text' })).toHaveProperty('value', 'Kept.')
    })
    fireEvent.click(publishButton())

    await waitFor(() => expect(publishPage).toHaveBeenCalled())
    expect(vi.mocked(publishPage).mock.lastCall?.[1].baseVersion).toBe(5)
  })
})
