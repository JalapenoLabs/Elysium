// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'

// User interface
import { LuRefreshCw, LuTrash2 } from 'react-icons/lu'
import { PageHeader } from './PageHeader'

function renderHeader(props: Partial<ComponentProps<typeof PageHeader>> = {}) {
  return render(<MemoryRouter>
    <PageHeader title='Jira' {...props} />
  </MemoryRouter>)
}

describe('PageHeader', () => {
  it('leads the breadcrumbs with a back button to the nearest parent', () => {
    renderHeader({
      parents: [
        { label: 'Settings', href: '/settings' },
        { label: 'Connectors', href: '/settings/connectors' },
      ],
    })

    const back = screen.getByRole('link', { name: 'Back to Connectors' })
    expect(back.getAttribute('href')).toBe('/settings/connectors')
    const crumbs = within(screen.getByRole('navigation', { name: 'Breadcrumbs' }))
    expect(crumbs.getByRole('link', { name: 'Settings' }).getAttribute('href')).toBe('/settings')
    expect(crumbs.getByText('Jira')).toBeTruthy()
    expect(screen.getByRole('heading', { level: 1, name: 'Jira' })).toBeTruthy()
  })

  it('has no breadcrumbs and no back button on a top-level page', () => {
    renderHeader()

    expect(screen.queryByRole('navigation', { name: 'Breadcrumbs' })).toBeNull()
    expect(screen.queryByRole('link')).toBeNull()
    expect(screen.getByRole('heading', { level: 1, name: 'Jira' })).toBeTruthy()
  })

  it('shows a lone harmless action as its own button', () => {
    const sync = vi.fn<() => void>()
    renderHeader({ actions: [{ id: 'sync', label: 'Sync now', icon: LuRefreshCw, onAction: sync }]})

    fireEvent.click(screen.getByRole('button', { name: 'Sync now' }))
    expect(sync).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('button', { name: 'Actions' })).toBeNull()
  })

  it('folds more than one action, or a lone destructive one, into the Actions menu', () => {
    const { unmount } = renderHeader({
      actions: [
        { id: 'sync', label: 'Sync now', icon: LuRefreshCw, onAction: () => {} },
        { id: 'delete', label: 'Delete', icon: LuTrash2, isDanger: true, onAction: () => {} },
      ],
    })
    expect(screen.getByRole('button', { name: 'Actions' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Sync now' })).toBeNull()
    unmount()

    renderHeader({ actions: [{ id: 'delete', label: 'Delete', icon: LuTrash2, isDanger: true, onAction: () => {} }]})
    expect(screen.getByRole('button', { name: 'Actions' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull()
  })

  it('keeps the Actions menu on the far right, after the primary action and Save', () => {
    renderHeader({
      primaryAction: <button type='button'>Apply</button>,
      save: <button type='button'>Save</button>,
      actions: [
        { id: 'sync', label: 'Sync now', icon: LuRefreshCw, onAction: () => {} },
        { id: 'pause', label: 'Pause', icon: LuRefreshCw, onAction: () => {} },
      ],
    })

    const names = screen.getAllByRole('button').map((button) => button.textContent)
    expect(names).toEqual([ 'Apply', 'Save', 'Actions' ])
  })
})
