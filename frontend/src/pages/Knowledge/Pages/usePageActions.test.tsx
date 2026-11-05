// Copyright © 2026 Jalapeno Labs

import type { ConfirmOptions } from '../../../gates/ConfirmGate'
import type { ToastOptions } from '../../../toast/toast'

// Core
import { act, render, waitFor } from '@testing-library/react'
import { HTTPError } from 'ky'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'

// Misc
import { ConfirmContext } from '../../../gates/ConfirmGate'
import { makePage, makePageSummary } from '../../../testFixtures'
import { getKnowledgePageUrl, UrlTree } from '../../../urls'
import { buildPageTree } from './pageTree'
import { usePageActions } from './usePageActions'

// The page requests are the network boundary; toasts are watched for their Undo.
const routes = vi.hoisted(() => ({
  deletePage: vi.fn(),
  movePage: vi.fn(),
  restorePage: vi.fn(),
}))

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({ toast: toasts }))

const HANDBOOK = makePageSummary({ id: 'handbook', title: 'Handbook' })
const GUIDE = makePageSummary({ id: 'guide', parentId: 'handbook', title: 'Guide' })
const CHAPTER = makePageSummary({ id: 'chapter', parentId: 'guide', title: 'Chapter' })
const SECTION = makePageSummary({ id: 'section', parentId: 'chapter', title: 'Section' })
const NOTES = makePageSummary({ id: 'notes', parentId: 'handbook', title: 'Notes' })
const tree = buildPageTree([ HANDBOOK, GUIDE, NOTES, CHAPTER, SECTION ])

// Opens a page, where a harness holds the actions, with the app's confirmation dialog replaced by a
// spy that shows what it was asked.
function renderOnPage(pageId: string) {
  const store = createAppStore()
  const confirm = vi.fn<(options: ConfirmOptions) => void>()
  const captured: { actions?: ReturnType<typeof usePageActions> } = {}
  function Harness() {
    captured.actions = usePageActions(tree)
    return null
  }
  const router = createMemoryRouter([
    { path: UrlTree.knowledge, element: <p>Knowledge</p> },
    { path: UrlTree.knowledgePage, element: <Harness /> },
  ], { initialEntries: [ getKnowledgePageUrl(pageId) ]})
  render(<Provider store={store}>
    <ConfirmContext.Provider value={confirm}>
      <RouterProvider router={router} />
    </ConfirmContext.Provider>
  </Provider>)
  return { store, router, captured, confirm }
}

function refusal(code: string) {
  const response = new Response(null, { status: 409 })
  const error = new HTTPError(response, new Request('http://localhost/'), {} as never)
  error.data = { code, message: 'refused' }
  return error
}

function lastSuccessOptions(): ToastOptions | undefined {
  return toasts.success.mock.lastCall?.[1]
}

beforeEach(() => {
  vi.clearAllMocks()
  routes.deletePage.mockResolvedValue(undefined)
  routes.movePage.mockResolvedValue({ page: makePage({ id: 'notes', title: 'Notes' }) })
  routes.restorePage.mockResolvedValue({ page: makePage({ id: 'notes', title: 'Notes' }) })
})

describe('usePageActions remove', () => {
  it('deletes a page with nothing under it at once, asking nothing', async () => {
    const { confirm, captured } = renderOnPage('guide')

    await act(async () => captured.actions?.remove(NOTES))

    await waitFor(() => expect(toasts.success).toHaveBeenCalled())
    expect(confirm).not.toHaveBeenCalled()
    expect(routes.deletePage).toHaveBeenCalledWith('notes', { keepChildren: false })
    expect(lastSuccessOptions()?.action?.label).toBe('Undo')
  })

  it('asks about a page\'s children, naming where they go and how many would go with it', () => {
    const { confirm, captured } = renderOnPage('notes')

    act(() => captured.actions?.remove(GUIDE))

    const asked = confirm.mock.lastCall?.[0]
    expect(routes.deletePage).not.toHaveBeenCalled()
    expect(asked?.title).toBe('Delete Guide?')
    expect(asked?.message).toBe('Its child page moves up to Handbook, unless you delete it too.')
    expect(asked?.switches?.[0].label).toBe('Delete the 2 pages under it too')
    expect(asked?.tone).toBe('danger')
  })

  it('names the top of the tree when a page has no parent', () => {
    const { confirm, captured } = renderOnPage('notes')

    act(() => captured.actions?.remove(HANDBOOK))

    expect(confirm.mock.lastCall?.[0].message)
      .toBe('Its 2 child pages move to the top of the tree, unless you delete them too.')
  })

  it('keeps the children unless the switch says otherwise', async () => {
    const { confirm, captured } = renderOnPage('notes')

    act(() => captured.actions?.remove(GUIDE))
    const asked = confirm.mock.lastCall?.[0]

    await act(async () => asked?.onConfirm(new Set()))
    expect(routes.deletePage).toHaveBeenLastCalledWith('guide', { keepChildren: true })

    const [ deleteChildren ] = asked?.switches ?? []
    await act(async () => asked?.onConfirm(new Set([ deleteChildren.id ])))
    expect(routes.deletePage).toHaveBeenLastCalledWith('guide', { keepChildren: false })
  })

  it('leaves a page being read for its parent, and Undo comes back to it', async () => {
    const { router, captured } = renderOnPage('notes')

    await act(async () => captured.actions?.remove(NOTES))
    await waitFor(() => expect(toasts.success).toHaveBeenCalled())
    expect(router.state.location.pathname).toBe(getKnowledgePageUrl('handbook'))

    await act(async () => lastSuccessOptions()?.action?.onPress())

    await waitFor(() => expect(router.state.location.pathname).toBe(getKnowledgePageUrl('notes')))
    expect(routes.restorePage).toHaveBeenCalledWith('notes')
    expect(toasts.success).toHaveBeenLastCalledWith('Notes restored')
  })

  it('leaves a page at the top of the tree for the knowledge base', async () => {
    const childless = makePageSummary({ id: 'guide', title: 'Guide' })
    const { router, confirm, captured } = renderOnPage('guide')

    act(() => captured.actions?.remove({ ...childless, id: 'handbook' }))
    expect(confirm).toHaveBeenCalled()
    await act(async () => confirm.mock.lastCall?.[0].onConfirm(new Set([ 'delete-children' ])))

    await waitFor(() => expect(router.state.location.pathname).toBe(UrlTree.knowledge))
  })

  it('leaves a page taken with its ancestor, but stays on a child that was kept', async () => {
    const { router, confirm, captured } = renderOnPage('chapter')

    act(() => captured.actions?.remove(GUIDE))
    await act(async () => confirm.mock.lastCall?.[0].onConfirm(new Set()))
    expect(router.state.location.pathname).toBe(getKnowledgePageUrl('chapter'))

    await act(async () => confirm.mock.lastCall?.[0].onConfirm(new Set([ 'delete-children' ])))
    await waitFor(() => expect(router.state.location.pathname).toBe(getKnowledgePageUrl('handbook')))
  })

  it('stays where it is on another page, and Undo stays too', async () => {
    const { router, captured } = renderOnPage('guide')

    await act(async () => captured.actions?.remove(NOTES))
    await waitFor(() => expect(toasts.success).toHaveBeenCalled())
    await act(async () => lastSuccessOptions()?.action?.onPress())

    await waitFor(() => expect(routes.restorePage).toHaveBeenCalled())
    expect(router.state.location.pathname).toBe(getKnowledgePageUrl('guide'))
  })

  it('keeps the dialog open when the delete is refused, with nothing to undo', async () => {
    routes.deletePage.mockRejectedValue(refusal('page_fixed'))
    const { confirm, captured } = renderOnPage('notes')

    act(() => captured.actions?.remove(GUIDE))

    await expect(confirm.mock.lastCall?.[0].onConfirm(new Set())).rejects.toThrow()
    expect(toasts.danger).toHaveBeenCalledWith('Elysium keeps this page in place.')
    expect(toasts.success).not.toHaveBeenCalled()
  })

  it('says so when Undo finds the parent gone', async () => {
    routes.restorePage.mockRejectedValue(refusal('page_parent_gone'))
    const { captured } = renderOnPage('guide')

    await act(async () => captured.actions?.remove(NOTES))
    await waitFor(() => expect(toasts.success).toHaveBeenCalled())
    await act(async () => lastSuccessOptions()?.action?.onPress())

    await waitFor(() => expect(toasts.danger).toHaveBeenCalledWith('The page it goes under has been deleted.'))
  })
})

describe('usePageActions restore', () => {
  it('brings a deleted page back from its own address, and stays there', async () => {
    const { store, router, captured } = renderOnPage('notes')

    let isRestored = false
    await act(async () => {
      isRestored = await captured.actions?.restore(NOTES) ?? false
    })

    expect(isRestored).toBe(true)
    expect(store.getState().pages.openedById.notes.id).toBe('notes')
    expect(router.state.location.pathname).toBe(getKnowledgePageUrl('notes'))
    expect(toasts.success).toHaveBeenLastCalledWith('Notes restored')
  })
})

describe('usePageActions move', () => {
  it('moves a page, and Undo puts it back where it was', async () => {
    const { captured } = renderOnPage('guide')
    const placement = { parentId: null, after: null }

    let isMoved = false
    await act(async () => {
      isMoved = await captured.actions?.move(NOTES, placement) ?? false
    })

    expect(isMoved).toBe(true)
    expect(routes.movePage).toHaveBeenLastCalledWith('notes', placement)

    await act(async () => lastSuccessOptions()?.action?.onPress())
    expect(routes.movePage).toHaveBeenLastCalledWith('notes', { parentId: 'handbook', after: 'guide' })
  })

  it('feeds the tree the answer at once', async () => {
    const { store, captured } = renderOnPage('guide')

    await act(async () => captured.actions?.move(NOTES, { parentId: null, after: null }))

    expect(store.getState().pages.openedById.notes.title).toBe('Notes')
  })

  it('says what stopped a move, in its own words', async () => {
    routes.movePage.mockRejectedValue(refusal('page_placement_gone'))
    const { captured } = renderOnPage('guide')

    let isMoved = true
    await act(async () => {
      isMoved = await captured.actions?.move(NOTES, { parentId: null, after: 'gone' }) ?? true
    })

    expect(isMoved).toBe(false)
    expect(toasts.danger).toHaveBeenCalledWith('The tree changed. Try again in its new order.')
    expect(toasts.success).not.toHaveBeenCalled()
  })
})
