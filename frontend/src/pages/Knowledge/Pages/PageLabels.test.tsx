// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { HTTPError } from 'ky'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { pageLoaded, pagesLoaded } from '../../../store/pagesSlice'

// User interface
import { PageLabels } from './PageLabels'

// Misc
import { addPageLabel, removePageLabel } from '../../../api/routes/pageRoutes'
import { makePage, makePageSummary } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { toast } from '../../../toast/toast'
import { getKnowledgeLabelUrl } from '../../../urls'

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  addPageLabel: vi.fn(),
  removePageLabel: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({
  toast: { success: vi.fn(), danger: vi.fn() },
}))

function renderLabels(page: KnowledgePage) {
  const store = createAppStore()
  store.dispatch(pagesLoaded([
    makePageSummary({ id: page.id, labels: page.labels }),
    makePageSummary({ id: 'other', labels: [ 'billing', 'runbook' ]}),
  ]))
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={store}>
      <TestAriaRouter>
        <PageLabels page={page} />
      </TestAriaRouter>
    </Provider>,
  }])
  render(<RouterProvider router={router} />)
  return store
}

function typeLabel(text: string) {
  const input = screen.getByRole('combobox', { name: 'Add label' })
  fireEvent.focus(input)
  fireEvent.change(input, { target: { value: text }})
  return input
}

describe('PageLabels', () => {
  beforeEach(() => {
    vi.mocked(addPageLabel).mockReset()
    vi.mocked(removePageLabel).mockReset()
    vi.mocked(toast.success).mockReset()
    vi.mocked(toast.danger).mockReset()
  })

  it('links each label to the pages that carry it', () => {
    renderLabels(makePage({ id: 'page', labels: [ 'on call' ]}))

    expect(screen.getByRole('link', { name: 'on call' }).getAttribute('href'))
      .toBe(getKnowledgeLabelUrl('on call'))
  })

  it('adds what was typed on Enter, shows what the API made of it, and clears the field', async () => {
    const page = makePage({ id: 'page' })
    vi.mocked(addPageLabel).mockResolvedValue({ page: { ...page, labels: [ 'on-call' ]}})
    const store = renderLabels(page)

    const input = typeLabel('On Call')
    fireEvent.keyDown(input, { key: 'Enter' })

    await waitFor(() => expect(addPageLabel).toHaveBeenCalledWith('page', 'On Call'))
    await waitFor(() => expect(store.getState().pages.openedById.page?.labels).toEqual([ 'on-call' ]))
    await waitFor(() => expect(input).toHaveProperty('value', ''))
  })

  it('offers the labels in use that the page does not carry', async () => {
    const page = makePage({ id: 'page', labels: [ 'runbook' ]})
    vi.mocked(addPageLabel).mockResolvedValue({ page })
    renderLabels(page)

    const input = screen.getByRole('combobox', { name: 'Add label' })
    fireEvent.focus(input)
    fireEvent.keyDown(input, { key: 'ArrowDown' })

    const option = await screen.findByRole('option', { name: 'billing' })
    expect(screen.queryByRole('option', { name: 'runbook' })).toBeNull()

    fireEvent.click(option)
    await waitFor(() => expect(addPageLabel).toHaveBeenCalledWith('page', 'billing'))
  })

  it('says why a refused label was refused and keeps what was typed', async () => {
    vi.mocked(addPageLabel).mockRejectedValue(new HTTPError(
      new Response(null, { status: 422 }),
      new Request('http://localhost/'),
      {} as never,
    ))
    renderLabels(makePage({ id: 'page' }))

    const input = typeLabel('a/b')
    fireEvent.keyDown(input, { key: 'Enter' })

    await waitFor(() => expect(toast.danger).toHaveBeenCalled())
    expect(vi.mocked(toast.danger).mock.calls[0][0]).toBe('The label wasn\'t added')
    expect(input).toHaveProperty('value', 'a/b')
  })

  it('takes a label off at once and puts it back from the toast', async () => {
    const page = makePage({ id: 'page', labels: [ 'runbook' ]})
    vi.mocked(removePageLabel).mockResolvedValue({ page: { ...page, labels: []}})
    vi.mocked(addPageLabel).mockResolvedValue({ page })
    const store = renderLabels(page)
    store.dispatch(pageLoaded(page))

    fireEvent.click(screen.getByRole('button', { name: 'Remove runbook' }))

    await waitFor(() => expect(removePageLabel).toHaveBeenCalledWith('page', 'runbook'))
    await waitFor(() => expect(toast.success).toHaveBeenCalled())
    const options = vi.mocked(toast.success).mock.calls[0][1]
    options?.action?.onPress()
    await waitFor(() => expect(addPageLabel).toHaveBeenCalledWith('page', 'runbook'))
  })

  it('only reads the labels of a deleted page', () => {
    renderLabels(makePage({
      id: 'page',
      labels: [ 'runbook' ],
      deletedAt: '2026-11-02T12:00:00Z',
    }))

    expect(screen.getByRole('link', { name: 'runbook' })).toBeDefined()
    expect(screen.queryByRole('combobox')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Remove runbook' })).toBeNull()
  })
})
