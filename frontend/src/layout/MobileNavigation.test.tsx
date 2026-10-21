// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'

// User interface
import { MobileNavigation } from './MobileNavigation'
import { Sidebar } from './Sidebar'

// At phone widths the sidebar kept its full 14rem, squeezing every page into the rest of a
// 375px screen. Below md the sidebar now hides and the topbar's menu opens the same links.
describe('Sidebar', () => {
  it('is hidden below md and shown from md up', () => {
    const { container } = render(<MemoryRouter>
      <Sidebar />
    </MemoryRouter>)

    const sidebar = container.querySelector('aside')
    expect(sidebar?.classList.contains('hidden')).toBe(true)
    expect(sidebar?.classList.contains('md:flex')).toBe(true)
  })
})

describe('MobileNavigation', () => {
  function renderAt(path: string) {
    return render(<MemoryRouter initialEntries={[ path ]}>
      <MobileNavigation />
      <Routes>
        <Route path='*' element={<span />} />
      </Routes>
    </MemoryRouter>)
  }

  it('opens the primary links in a drawer', async () => {
    renderAt('/action-items')

    expect(screen.queryByRole('dialog')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }))

    const drawer = await screen.findByRole('dialog')
    expect(drawer.textContent).toContain('Coding')
    expect(drawer.textContent).toContain('Studio')
  })

  it('closes once a link is followed', async () => {
    renderAt('/action-items')

    fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }))
    fireEvent.click(await screen.findByRole('link', { name: 'Coding' }))

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })
})
