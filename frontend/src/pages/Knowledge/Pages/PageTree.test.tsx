// Copyright © 2026 Jalapeno Labs

import type { Key } from 'react-aria-components'
import type { PageTree as PageTreeShape } from './pageTree'

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
import { MEETINGS_PAGE_ID } from '../../../constants'
import { makeMeeting, makePageSummary } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { getKnowledgePageUrl, getMeetingUrl, getNewChildPageUrl, getProjectMeetingsUrl, UrlTree } from '../../../urls'
import { buildPageTree, withMeetingEntries } from './pageTree'

const tree = buildPageTree([
  makePageSummary({ id: 'guide', title: 'Guide' }),
  makePageSummary({ id: 'chapter', parentId: 'guide', title: 'Chapter' }),
  makePageSummary({ id: 'section', parentId: 'chapter', title: 'Section' }),
  makePageSummary({ id: 'notes', title: 'Notes' }),
  makePageSummary({ id: 'learnings', title: 'Learnings', fixed: true }),
])

type HarnessProps = {
  tree: PageTreeShape
  initialOpenIds: string[]
}

function Harness(props: HarnessProps) {
  const [ openIds, setOpenIds ] = useState<Set<Key>>(new Set(props.initialOpenIds))

  return <TestAriaRouter>
    <PageTree
      tree={props.tree}
      openIds={new Set(Array.from(openIds, String))}
      onOpenIdsChange={setOpenIds}
      onOpenBranches={() => undefined}
    />
  </TestAriaRouter>
}

function renderTree(currentPageId: string, initialOpenIds: string[] = []) {
  return renderAt(getKnowledgePageUrl(currentPageId), tree, initialOpenIds)
}

function renderAt(path: string, shownTree: PageTreeShape, initialOpenIds: string[]) {
  const router = createMemoryRouter(
    [{ path: '*', element: <Harness tree={shownTree} initialOpenIds={initialOpenIds} /> }],
    { initialEntries: [ path ]},
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

describe('meetings in the tree', () => {
  const meetingsTree = withMeetingEntries(
    buildPageTree([
      makePageSummary({ id: MEETINGS_PAGE_ID, title: 'Meetings', fixed: true }),
      makePageSummary({ id: 'briefs', title: 'Project briefs', fixed: true }),
      makePageSummary({ id: 'launch-brief', parentId: 'briefs', title: 'Launch brief', briefProjectId: 'launch' }),
    ]),
    [
      makeMeeting({ id: 'standup', title: 'Standup', projectIds: [ 'launch' ]}),
      makeMeeting({ id: 'untitled', title: '', projectIds: []}),
    ],
  )

  function renderMeetingsTree(path: string) {
    return renderAt(path, meetingsTree, [ MEETINGS_PAGE_ID, 'briefs', 'launch-brief', 'meetings:launch' ])
  }

  it('opens the Meetings page as the Meetings list, and lists every meeting under it', () => {
    renderMeetingsTree(UrlTree.knowledgeMeetings)

    expect(screen.getByRole('link', { name: 'Meetings' }).getAttribute('href')).toBe(UrlTree.knowledgeMeetings)
    expect(screen.getByRole('link', { name: 'Meetings' }).getAttribute('aria-current')).toBe('page')
    const homes = screen.getAllByRole('link', { name: 'Standup' })
    expect(homes[0].getAttribute('href')).toBe(getMeetingUrl('standup'))
    expect(screen.getByRole('link', { name: 'Untitled meeting' }).getAttribute('href')).toBe(getMeetingUrl('untitled'))
  })

  it('groups a project\'s meetings under its brief, linking each to the one meeting page', () => {
    renderMeetingsTree(getMeetingUrl('standup'))

    // The group's row is named as the Meetings page's is, so it is found by where it goes.
    const groupRows = screen.getAllByRole('row', { name: 'Meetings', expanded: true })
    const groupLinks = groupRows.map((row) => row.querySelector('a')?.getAttribute('href'))
    expect(groupLinks).toEqual([ UrlTree.knowledgeMeetings, getProjectMeetingsUrl('launch') ])
    // Its home and its linked entry are one page, so both read as the page open.
    const entries = screen.getAllByRole('link', { name: 'Standup' })
    expect(entries).toHaveLength(2)
    for (const entry of entries) {
      expect(entry.getAttribute('aria-current')).toBe('page')
    }
  })

  it('gives a meeting no menu and no drag a keyboard could start', () => {
    renderMeetingsTree(UrlTree.knowledgeMeetings)

    expect(screen.queryByRole('button', { name: 'Actions for Standup' })).toBeNull()
    for (const drag of screen.getAllByRole('button', { name: 'Drag Standup', hidden: true })) {
      expect(drag.hasAttribute('disabled')).toBe(true)
    }
  })
})
