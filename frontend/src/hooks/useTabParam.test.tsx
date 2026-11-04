// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { createMemoryRouter, RouterProvider, useNavigate } from 'react-router'

// User interface
import { Tabs } from '@heroui/react'
import { PageTabs } from '../components/PageTabs'
import { Page } from '../layout/page/Page'

// Misc
import { usePageSave } from '../layout/page/pageSave'
import { TestAriaRouter } from '../testRouter'
import { useTabParam } from './useTabParam'

const TABS = [ 'people', 'pending' ] as const

// jsdom has neither observer, nor the Web Animations API: the tab list measures itself with
// one observer, the page watches its Save with the other, and the tab's indicator waits for its
// animations before moving.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', InertObserver)
  vi.stubGlobal('IntersectionObserver', InertObserver)
  Element.prototype.getAnimations = () => []
})

afterEach(() => {
  vi.unstubAllGlobals()
  // @ts-expect-error jsdom has no getAnimations of its own to put back.
  delete Element.prototype.getAnimations
})

// A page with a form above two counted tabs, as the project and Users pages are.
function TabbedForm() {
  const [ selectedTab, selectTab ] = useTabParam(TABS)
  const [ name, setName ] = useState('')
  const navigate = useNavigate()
  usePageSave({ save: vi.fn().mockResolvedValue(true), isDirty: Boolean(name) })

  return <>
    <input aria-label='Name' value={name} onChange={(event) => setName(event.target.value)} />
    <button type='button' onClick={() => void navigate('?project=other')}>Other project</button>
    <PageTabs
      label='Lists'
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={[
        { id: 'people', label: 'People', count: 3 },
        { id: 'pending', label: 'Pending', count: 0, isWaiting: true },
      ]}
    >
      <Tabs.Panel id='people'>Everyone approved</Tabs.Panel>
      <Tabs.Panel id='pending'>Waiting sign-ups</Tabs.Panel>
    </PageTabs>
  </>
}

function renderPage(initialEntry = '/things') {
  const router = createMemoryRouter([{
    path: '/things',
    element: <TestAriaRouter>
      <Page title='Things'>
        <TabbedForm />
      </Page>
    </TestAriaRouter>,
  }], { initialEntries: [ initialEntry ]})
  render(<RouterProvider router={router} />)
  return router
}

describe('useTabParam', () => {
  it('opens the tab the address names, and the first for anything else', () => {
    renderPage('/things?tab=pending')
    expect(screen.getByText('Waiting sign-ups')).toBeTruthy()
  })

  it('falls back to the first tab for a tab the page does not show', () => {
    renderPage('/things?tab=feed')
    expect(screen.getByText('Everyone approved')).toBeTruthy()
  })

  it('keeps the open tab in the address, leaving it plain for the first', () => {
    const router = renderPage()

    fireEvent.click(screen.getByRole('tab', { name: /Pending/ }))
    expect(router.state.location.search).toBe('?tab=pending')
    expect(screen.getByText('Waiting sign-ups')).toBeTruthy()

    fireEvent.click(screen.getByRole('tab', { name: /People/ }))
    expect(router.state.location.search).toBe('')
  })

  it('switches tabs on a page with unsaved changes without asking, keeping what was typed', () => {
    const router = renderPage()
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Lamp' }})

    fireEvent.click(screen.getByRole('tab', { name: /Pending/ }))
    expect(router.state.location.search).toBe('?tab=pending')
    expect(screen.queryByText('Unsaved changes')).toBeNull()
    expect(screen.getByLabelText('Name')).toHaveProperty('value', 'Lamp')
  })

  it('still asks before any other query change, which can swap what the page edits', async () => {
    const router = renderPage()
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Lamp' }})

    fireEvent.click(screen.getByRole('button', { name: 'Other project' }))
    expect(await screen.findByText('Unsaved changes')).toBeTruthy()
    expect(router.state.location.search).toBe('')
  })
})

describe('PageTabs', () => {
  it('shows every count, zero included, beside its tab\'s name', () => {
    renderPage()
    expect(screen.getByRole('tab', { name: /People/ }).textContent).toContain('3')
    expect(screen.getByRole('tab', { name: /Pending/ }).textContent).toContain('0')
  })
})
