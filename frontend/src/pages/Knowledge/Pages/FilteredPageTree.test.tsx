// Copyright © 2026 Jalapeno Labs

import type { PageTreeFilter } from './pageTree'
import type { PageTreeMemoryState } from './usePageTreeMemory'

// Core
import { fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'

// User interface
import { FilteredPageTree } from './FilteredPageTree'

// Misc
import { makePageSummary } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { buildPageTree } from './pageTree'

const tree = buildPageTree([
  makePageSummary({ id: 'guide', title: 'Guide' }),
  makePageSummary({ id: 'runbook', parentId: 'guide', title: 'Runbook', labels: [ 'on-call' ]}),
  makePageSummary({ id: 'notes', title: 'Notes' }),
])

function makeMemory() {
  return {
    openIds: new Set<string>(),
    setOpenIds: vi.fn(),
    openBranches: vi.fn(),
  } as unknown as PageTreeMemoryState
}

function renderTree(filter: PageTreeFilter, memory = makeMemory()) {
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={createAppStore()}>
      <TestAriaRouter>
        <FilteredPageTree tree={tree} filter={filter} memory={memory} />
      </TestAriaRouter>
    </Provider>,
  }])
  render(<RouterProvider router={router} />)
  return memory
}

describe('FilteredPageTree', () => {
  it('leaves the tree as the person keeps it with no filter on', () => {
    renderTree({ label: null })

    expect(screen.getByRole('link', { name: 'Guide' })).toBeDefined()
    expect(screen.getByRole('link', { name: 'Notes' })).toBeDefined()
    expect(screen.queryByRole('link', { name: 'Runbook' })).toBeNull()
  })

  it('opens every branch holding a match and shows only what the filter leaves', () => {
    renderTree({ label: 'on-call' })

    expect(screen.getByRole('link', { name: 'Runbook' })).toBeDefined()
    expect(screen.getByRole('link', { name: 'Guide' })).toBeDefined()
    expect(screen.queryByRole('link', { name: 'Notes' })).toBeNull()
  })

  it('closes a branch for as long as the filter lasts, leaving what the person keeps open alone', () => {
    const memory = renderTree({ label: 'on-call' })

    fireEvent.click(screen.getByRole('button', { name: /collapse|expand/i }))

    expect(screen.queryByRole('link', { name: 'Runbook' })).toBeNull()
    expect(memory.setOpenIds).not.toHaveBeenCalled()
  })

  it('moves a page among everything, not just what the filter leaves', async () => {
    renderTree({ label: 'on-call' })

    fireEvent.click(screen.getByRole('button', { name: 'Actions for Runbook' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Move' }))
    const parent = await screen.findByRole('combobox', { name: 'Under' })
    fireEvent.keyDown(parent, { key: 'ArrowDown' })

    expect(await screen.findByRole('option', { name: 'Notes' })).toBeDefined()
  })

  it('says so when no page carries the label', () => {
    renderTree({ label: 'never' })

    expect(screen.getByRole('status').textContent).toBe('No pages carry this label.')
    expect(screen.queryByRole('link')).toBeNull()
  })
})
