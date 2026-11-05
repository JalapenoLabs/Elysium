// Copyright © 2026 Jalapeno Labs

import type { Key } from 'react-aria-components'

// Core
import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { describe, expect, it } from 'vitest'

// User interface
import { PageTree } from './PageTree'

// Misc
import { makePageSummary } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { getKnowledgePageUrl } from '../../../urls'
import { buildPageTree } from './pageTree'

const tree = buildPageTree([
  makePageSummary({ id: 'guide', title: 'Guide' }),
  makePageSummary({ id: 'chapter', parentId: 'guide', title: 'Chapter' }),
  makePageSummary({ id: 'section', parentId: 'chapter', title: 'Section' }),
  makePageSummary({ id: 'notes', title: 'Notes' }),
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
    />
  </TestAriaRouter>
}

function renderTree(currentPageId: string, initialOpenIds: string[] = []) {
  const router = createMemoryRouter(
    [{ path: '*', element: <Harness initialOpenIds={initialOpenIds} /> }],
    { initialEntries: [ getKnowledgePageUrl(currentPageId) ]},
  )
  render(<RouterProvider router={router} />)
  return router
}

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
    expect(rows.map((row) => row.getAttribute('aria-expanded'))).toEqual([ 'false', null ])
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
})
