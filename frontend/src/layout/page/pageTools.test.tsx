// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { createPortal } from 'react-dom'
import { createMemoryRouter, Outlet, RouterProvider } from 'react-router'

// User interface
import { Page } from './Page'

// Misc
import { usePageToolsContainer } from './pageTools'
import { TestAriaRouter } from '../../testRouter'

// Stands in for a list table: puts its controls where `controls={{ container }}` would.
function ListWithControls() {
  const container = usePageToolsContainer()
  return <>
    {container && createPortal(<button type='button'>Filters</button>, container)}
    <p>The list</p>
  </>
}

function renderLayout(path: string) {
  const router = createMemoryRouter([{
    path: '/items',
    element: <TestAriaRouter>
      <Page title='Items' primaryAction={<button type='button'>New item</button>}>
        <Outlet />
      </Page>
    </TestAriaRouter>,
    children: [
      { path: 'all', element: <ListWithControls /> },
      { path: 'next', element: <p>One at a time</p> },
    ],
  }], { initialEntries: [ path ]})
  render(<RouterProvider router={router} />)
}

describe('usePageToolsContainer', () => {
  beforeEach(() => {
    vi.stubGlobal('IntersectionObserver', class {
      observe() {}
      disconnect() {}
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reaches the titlebar of the layout a child route renders in, before its primary action', () => {
    renderLayout('/items/all')

    const header = within(screen.getByRole('banner'))
    const filters = header.getByRole('button', { name: 'Filters' })
    const newItem = header.getByRole('button', { name: 'New item' })
    // The table's controls lead the titlebar's actions, as search and filters do.
    expect(filters.compareDocumentPosition(newItem) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(screen.getByText('The list').closest('header')).toBeNull()
  })

  it('leaves the slot empty for a child with no list', () => {
    renderLayout('/items/next')

    expect(screen.queryByRole('button', { name: 'Filters' })).toBeNull()
    expect(screen.getByRole('banner').querySelector('.page-tools-slot')?.childElementCount).toBe(0)
  })
})
