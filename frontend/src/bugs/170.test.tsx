// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, Route, RouterProvider, Routes } from 'react-router'

// Redux
import { createAppStore } from '../store'

// User interface
import { NextItemCard } from '../pages/ActionItems/NextItemCard'
import { EnvironmentVariableTable } from '../pages/Settings/Environment/EnvironmentVariableTable'
import { LlmTable } from '../pages/Settings/Llms/LlmTable'
import { ManageLlmsPage } from '../pages/Settings/Llms/ManageLlmsPage'
import { StorageLocationTable } from '../pages/Settings/Storage/StorageLocationTable'

// Misc
import { makeActionItem, makeLlm, makeStorageLocation } from '../testFixtures'
import { TestAriaRouter } from '../testRouter'

// Issue #170: buttons and menu items that went somewhere called `navigate()`, so they could not
// be middle-clicked, ctrl+clicked, or opened in a new tab. Each is now a real anchor with the
// address it goes to, which is what lets the browser open it anywhere. A table row's editing is
// its name's link, since row actions are buttons.

// Every loader answers as loaded: these tests are about where things link, not what loads.
vi.mock('../hooks/useServerData', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return Object.fromEntries(Object.keys(actual).map((name) => [ name, () => 'loaded' ]))
})

const noop = () => {}
const asyncNoop = async () => {}

function renderRouted(element: ReactNode) {
  // A data router, as the app has, since a page guards its unsaved changes with a blocker.
  const router = createMemoryRouter([{
    path: '*',
    element: <TestAriaRouter>
      {element}
      <Routes>
        <Route path='/settings/llms/:llmId/edit' element={<p>Edit page</p>} />
        <Route path='*' element={<span />} />
      </Routes>
    </TestAriaRouter>,
  }], { initialEntries: [ '/start' ]})
  return render(<Provider store={createAppStore()}>
    <RouterProvider router={router} />
  </Provider>)
}

async function openMenu(trigger: HTMLElement) {
  fireEvent.click(trigger)
  return screen.findByRole('menu')
}

function hrefOf(element: HTMLElement) {
  return element.closest('a')?.getAttribute('href')
}

describe('issue #170', () => {
  it('starts a coding session from Next with a link', () => {
    renderRouted(<NextItemCard
      item={makeActionItem({ id: 'item-1', title: 'Fix the login cookie' })}
      position={1}
      total={1}
      now={Date.parse('2026-11-03T12:00:00Z')}
      onSkip={noop}
    />)

    expect(hrefOf(screen.getByText('Start a coding session'))).toBe('/coding/new?item=item-1')
    expect(hrefOf(screen.getByText('Fix the login cookie'))).toBe('/action-items/item-1')
  })

  it('edits an environment variable from its row with a link', () => {
    renderRouted(<EnvironmentVariableTable
      variables={[{
        id: 'variable-1',
        key: 'NPM_TOKEN',
        isSecret: true,
        description: '',
        value: null,
        createdAt: '2026-11-03T12:00:00Z',
        createdBy: 'user-1',
        updatedAt: '2026-11-03T12:00:00Z',
      }]}
      onDelete={noop}
    />)

    expect(hrefOf(screen.getByText('NPM_TOKEN'))).toBe('/settings/environment/variable-1/edit')
  })

  it('edits a storage location from its row with a link', () => {
    renderRouted(<StorageLocationTable
      locations={[ makeStorageLocation({ id: 'location-1', name: 'Renders' }) ]}
      onTest={asyncNoop}
      onDelete={noop}
    />)

    expect(hrefOf(screen.getByText('Renders'))).toBe('/settings/storage/location-1/edit')
  })

  it('edits an LLM from its row with a link that still routes on a plain click', async () => {
    renderRouted(<LlmTable
      llms={[ makeLlm({ id: 'llm-1', name: 'Team key' }) ]}
      onToggleActive={asyncNoop}
      onDelete={noop}
    />)

    const edit = screen.getByText('Team key')
    expect(hrefOf(edit)).toBe('/settings/llms/llm-1/edit')

    fireEvent.click(edit)
    expect(await screen.findByText('Edit page')).toBeTruthy()
  })

  it('adds each kind of LLM with a link to its own page', async () => {
    renderRouted(<ManageLlmsPage />)

    const menu = await openMenu(screen.getByRole('button', { name: 'Add LLM' }))
    const hrefs = within(menu)
      .getAllByRole('menuitem')
      .map((item) => item.tagName === 'A' && item.getAttribute('href'))
    expect(hrefs).toEqual([
      '/settings/llms/new/claude-account',
      '/settings/llms/new/claude-api-key',
      '/settings/llms/new/claude-code-oauth',
      '/settings/llms/new/openai-api-key',
      '/settings/llms/new/codex-oauth',
    ])
  })
})
