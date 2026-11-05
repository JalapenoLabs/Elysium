// Copyright © 2026 Jalapeno Labs

// Core
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { describe, expect, it } from 'vitest'

// User interface
import { foldPageCrumbs, PageBreadcrumbs } from './PageBreadcrumbs'

function crumbs(count: number) {
  return Array.from({ length: count }, (_, index) => ({
    label: `Parent ${index + 1}`,
    href: `/parent-${index + 1}`,
  }))
}

describe('foldPageCrumbs', () => {
  it('shows a short path whole', () => {
    expect(foldPageCrumbs(crumbs(3))).toEqual({ leading: crumbs(3), folded: [], trailing: []})
  })

  it('keeps a long path\'s first and last parents, and folds the rest', () => {
    const parents = crumbs(6)

    expect(foldPageCrumbs(parents)).toEqual({
      leading: parents.slice(0, 1),
      folded: parents.slice(1, 5),
      trailing: parents.slice(5),
    })
  })
})

describe('PageBreadcrumbs', () => {
  it('links what it shows, ending in the title, with a menu for what it folded', () => {
    render(<MemoryRouter>
      <PageBreadcrumbs parents={crumbs(5)} title='The page' />
    </MemoryRouter>)

    expect(screen.getByRole('link', { name: 'Parent 1' }).getAttribute('href')).toBe('/parent-1')
    expect(screen.getByRole('link', { name: 'Parent 5' }).getAttribute('href')).toBe('/parent-5')
    expect(screen.queryByRole('link', { name: 'Parent 3' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Pages in between' })).toBeDefined()
    expect(screen.getByText('The page')).toBeDefined()
  })

  it('has no menu for a short path', () => {
    render(<MemoryRouter>
      <PageBreadcrumbs parents={crumbs(2)} title='The page' />
    </MemoryRouter>)

    expect(screen.queryByRole('button')).toBeNull()
  })
})
