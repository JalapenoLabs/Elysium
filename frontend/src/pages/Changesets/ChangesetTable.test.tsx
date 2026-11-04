// Copyright © 2026 Jalapeno Labs

import type { Changeset } from '../../api/routes/changesetRoutes'
import type { ChangesetList } from './ChangesetTable'

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter, Route, Routes } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { projectsLoaded } from '../../store/projectsSlice'

// User interface
import { ChangesetTable } from './ChangesetTable'

// Misc
import { makeChangeset, makeChangesetOperation, makeProject } from '../../testFixtures'
import { UrlTree } from '../../urls'

// Names are read from the store; nothing is fetched.
vi.mock('../../hooks/useServerData', () => ({
  useProjectsLoader: () => 'loaded',
  useUsersLoader: () => 'loaded',
}))

// Noon UTC, so each day is the same in every zone a test machine might run in. "April" sorts
// before "March" as text, and after it in time.
const MARCH = '2027-03-01T12:00:00Z'
const APRIL = '2027-04-02T12:00:00Z'

function operation(position: number, decision: 'pending' | 'approved' | 'rejected') {
  return makeChangesetOperation({
    id: `operation-${position}`,
    position,
    decision,
    operation: { kind: 'comment', item: { id: 'item-1' }, body: 'Done' },
  })
}

function renderTable(changesets: Changeset[], list: ChangesetList) {
  const store = createAppStore()
  store.dispatch(projectsLoaded([ makeProject({ id: 'project-1', name: 'Website' }) ]))
  render(<Provider store={store}>
    <MemoryRouter initialEntries={[ UrlTree.changesets ]}>
      <Routes>
        <Route
          path={UrlTree.changesets}
          element={<ChangesetTable
            changesets={changesets}
            list={list}
            controlsContainer={null}
            emptyNotice={<p>Nothing here</p>}
          />}
        />
        <Route path={UrlTree.changesetView} element={<p>The review</p>} />
      </Routes>
    </MemoryRouter>
  </Provider>)
}

// Presses a column's header until it sorts ascending: number columns start descending.
function sortAscending(columnLabel: string) {
  const button = screen.getByText(columnLabel).closest('button')
  if (!button) {
    throw new Error(`The ${columnLabel} column cannot be sorted`)
  }
  for (let press = 0; press < 3 && button.getAttribute('aria-label') !== 'Sorted ascending'; press++) {
    fireEvent.click(button)
  }
  expect(button.getAttribute('aria-label')).toBe('Sorted ascending')
}

// The rows' summaries, top to bottom.
function rowOrder(summaries: string[]) {
  return screen
    .getAllByText(new RegExp(`^(${summaries.join('|')})$`))
    .map((element) => element.textContent)
}

describe('ChangesetTable', () => {
  it('counts the decisions so far of a changeset waiting for review, and leaves out its state', () => {
    renderTable([
      makeChangeset({
        id: 'waiting',
        projectId: 'project-1',
        operations: [ operation(1, 'approved'), operation(2, 'pending'), operation(3, 'pending') ],
      }),
    ], 'pending')

    expect(screen.getByText('3 changes')).toBeTruthy()
    expect(screen.getByText('1 approved, 0 rejected, 2 undecided')).toBeTruthy()
    expect(screen.getByText('Website')).toBeTruthy()
    expect(screen.queryByText('State')).toBeNull()
    expect(screen.queryByText('Decided')).toBeNull()
  })

  it('shows a reviewed changeset\'s state and when it was decided, without a tally', () => {
    renderTable([
      makeChangeset({
        id: 'applied',
        state: 'applied',
        decidedAt: APRIL,
        operations: [ operation(1, 'approved') ],
      }),
    ], 'reviewed')

    expect(screen.getByText('Applied')).toBeTruthy()
    expect(screen.getByText('Decided')).toBeTruthy()
    expect(screen.getByText('1 change')).toBeTruthy()
    expect(screen.queryByText(/undecided/)).toBeNull()
  })

  it('sorts by when each changeset was proposed, in time rather than by its words', () => {
    renderTable([
      makeChangeset({ id: 'april', summary: 'April changes', createdAt: APRIL }),
      makeChangeset({ id: 'march', summary: 'March changes', createdAt: MARCH }),
    ], 'pending')

    sortAscending('Proposed')
    expect(rowOrder([ 'April changes', 'March changes' ])).toEqual([ 'March changes', 'April changes' ])
  })

  it('opens a changeset\'s review from its summary and from a click on its row', async () => {
    renderTable([ makeChangeset({ id: 'changeset-1', summary: 'Close the launch items' }) ], 'pending')

    const summary = screen.getByText('Close the launch items').closest('a')
    expect(summary?.getAttribute('href')).toBe('/action-items/changesets/changeset-1')

    fireEvent.click(screen.getByText('Elysia'))
    await waitFor(() => expect(screen.getByText('The review')).toBeTruthy())
  })

  it('shows its empty notice in place of the rows when there are none', () => {
    renderTable([], 'reviewed')

    expect(screen.getByText('Nothing here')).toBeTruthy()
  })
})
