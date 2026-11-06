// Copyright © 2026 Jalapeno Labs

import type { Key } from 'react-aria-components'

// Core
import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'

// User interface
import { PageTree } from './PageTree'

// Misc
import { makePageSummary } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { getKnowledgePageUrl, getNewChildPageUrl } from '../../../urls'
import { buildPageTree } from './pageTree'

const tree = buildPageTree([
  makePageSummary({ id: 'guide', title: 'Guide' }),
  makePageSummary({ id: 'chapter', parentId: 'guide', title: 'Chapter' }),
  makePageSummary({ id: 'section', parentId: 'chapter', title: 'Section' }),
  makePageSummary({ id: 'notes', title: 'Notes' }),
  makePageSummary({ id: 'learnings', title: 'Learnings', fixed: true }),
])

type HarnessProps = {
  initialOpenIds: string[]
}

function Harness(props: HarnessProps) {
  const [ openIds, setOpenIds ] = useState<Set<Key>>(new Set(props.initialOpenIds))

  return <TestAriaRouter>
    <PageTree
      tree={tree}
      openIds={new Set(Array.from(openIds, String))}
      onOpenIdsChange={setOpenIds}
      onOpenBranches={() => undefined}
    />
  </TestAriaRouter>
}

function renderTree(currentPageId: string, initialOpenIds: string[] = []) {
  const router = createMemoryRouter(
    [{ path: '*', element: <Harness initialOpenIds={initialOpenIds} /> }],
    { initialEntries: [ getKnowledgePageUrl(currentPageId) ]},
  )
  render(<Provider store={createAppStore()}>
    <RouterProvider router={router} />
  </Provider>)
  return router
}

describe('a row\'s menu', () => {
  beforeEach(() => {
    vi.spyOn(window, 'matchMedia').mockImplementation((query) => ({
      matches: query === '(hover: hover)',
    }) as MediaQueryList)
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('mounts as a mouse comes over the row', () => {
    renderTree('notes')
    expect(screen.queryByRole('button', { name: 'Actions for Notes' })).toBeNull()

    fireEvent.pointerEnter(screen.getByRole('row', { name: 'Notes' }), { pointerType: 'mouse' })

    expect(screen.getByRole('button', { name: 'Actions for Notes' })).toBeDefined()
    expect(screen.queryByRole('button', { name: 'Actions for Guide' })).toBeNull()
  })

  it('mounts as focus reaches the row, so a keyboard can tab to it', () => {
    renderTree('notes')

    fireEvent.focus(screen.getByRole('link', { name: 'Notes' }))

    expect(screen.getByRole('button', { name: 'Actions for Notes' })).toBeDefined()
  })
})

describe('PageTree', () => {
  it('lists the top of the tree and shows no closed branch', () => {
    renderTree('notes')

    expect(screen.getByRole('link', { name: 'Guide' })).toBeDefined()
    expect(screen.getByRole('link', { name: 'Notes' })).toBeDefined()
    expect(screen.queryByRole('link', { name: 'Chapter' })).toBeNull()
  })

  it('gives every page a real link to its address', () => {
    renderTree('notes')

    expect(screen.getByRole('link', { name: 'Guide' }).getAttribute('href')).toBe(getKnowledgePageUrl('guide'))
  })

  it('marks the page being read, and only it', () => {
    renderTree('chapter', [ 'guide' ])

    expect(screen.getByRole('link', { name: 'Chapter' }).getAttribute('aria-current')).toBe('page')
    expect(screen.getByRole('link', { name: 'Guide' }).hasAttribute('aria-current')).toBe(false)
  })

  it('offers to open a page with children, and only such a page', () => {
    renderTree('notes')

    const rows = screen.getAllByRole('row')
    expect(rows.map((row) => row.getAttribute('aria-expanded'))).toEqual([ 'false', null, null ])
  })

  it('opens and closes a branch from its chevron without leaving the page', () => {
    const router = renderTree('notes')
    const [ guideRow ] = screen.getAllByRole('row')

    fireEvent.click(guideRow.querySelector('button') as HTMLButtonElement)
    expect(screen.getByRole('link', { name: 'Chapter' })).toBeDefined()
    expect(router.state.location.pathname).toBe(getKnowledgePageUrl('notes'))

    fireEvent.click(guideRow.querySelector('button') as HTMLButtonElement)
    expect(screen.queryByRole('link', { name: 'Chapter' })).toBeNull()
  })

  it('follows a page\'s link in the app', async () => {
    const router = renderTree('notes')

    fireEvent.click(screen.getByRole('link', { name: 'Guide' }))

    await expect.poll(() => router.state.location.pathname).toBe(getKnowledgePageUrl('guide'))
  })

  it('gives every page but a fixed one a drag button, so a keyboard can move it', () => {
    renderTree('notes')

    expect(screen.getByRole('button', { name: 'Drag Notes' }).hasAttribute('disabled')).toBe(false)
    expect(screen.getByRole('button', { name: 'Drag Learnings' }).hasAttribute('disabled')).toBe(true)
  })

  it('offers a child page, moving, and deleting from a row\'s menu', async () => {
    renderTree('notes')

    fireEvent.click(screen.getByRole('button', { name: 'Actions for Notes' }))

    const addChild = await screen.findByRole('menuitem', { name: 'Add child page' })
    expect(addChild.getAttribute('href')).toBe(getNewChildPageUrl('notes'))
    expect(screen.getByRole('menuitem', { name: 'Move' }).getAttribute('aria-disabled')).toBeNull()
    expect(screen.getByRole('menuitem', { name: 'Delete' }).getAttribute('aria-disabled')).toBeNull()
  })

  it('says why a fixed page can\'t move or be deleted, and still takes children', async () => {
    renderTree('notes')

    expect(screen.getByLabelText('Elysium keeps this page in place')).toBeDefined()
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Learnings' }))

    const addChild = await screen.findByRole('menuitem', { name: 'Add child page' })
    expect(addChild.getAttribute('aria-disabled')).toBeNull()
    expect(screen.getByRole('menuitem', { name: 'Move' }).getAttribute('aria-disabled')).toBe('true')
    expect(screen.getByRole('menuitem', { name: 'Delete' }).getAttribute('aria-disabled')).toBe('true')
    expect(screen.getByText('Elysium keeps this page in place')).toBeDefined()
  })
})
