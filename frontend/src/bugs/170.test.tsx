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
import { EnvironmentVariableRowActions } from '../pages/Settings/Environment/EnvironmentVariableRowActions'
import { LlmRowActions } from '../pages/Settings/Llms/LlmRowActions'
import { ManageLlmsPage } from '../pages/Settings/Llms/ManageLlmsPage'
import { StorageLocationRowActions } from '../pages/Settings/Storage/StorageLocationRowActions'

// Misc
import { makeActionItem, makeLlm, makeStorageLocation } from '../testFixtures'
import { TestAriaRouter } from '../testRouter'

// Issue #170: buttons and menu items that went somewhere called `navigate()`, so they could not
// be middle-clicked, ctrl+clicked, or opened in a new tab. Each is now a real anchor with the
// address it goes to, which is what lets the browser open it anywhere.

// Every loader answers as loaded: these tests are about where things link, not what loads.
vi.mock('../hooks/useServerData', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return Object.fromEntries(Object.keys(actual).map((name) => [ name, () => 'loaded' ]))
})

const noop = () => {}

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

    expect(hrefOf(screen.getByText('Start a coding session'))).toBe('/coding?item=item-1')
    expect(hrefOf(screen.getByText('Fix the login cookie'))).toBe('/action-items/item-1')
  })

  it('edits an environment variable from its row with a link', async () => {
    renderRouted(<EnvironmentVariableRowActions
      variable={{
        id: 'variable-1',
        key: 'NPM_TOKEN',
        isSecret: true,
        description: '',
        value: null,
        createdAt: '2026-11-03T12:00:00Z',
        createdBy: 'user-1',
        updatedAt: '2026-11-03T12:00:00Z',
      }}
      onDelete={noop}
    />)

    const menu = await openMenu(screen.getByRole('button', { name: 'More actions' }))
    const edit = within(menu).getByRole('menuitem', { name: 'Edit' })
    expect(edit.tagName).toBe('A')
    expect(edit.getAttribute('href')).toBe('/settings/environment/variable-1/edit')
  })

  it('edits a storage location from its row with a link', async () => {
    renderRouted(<StorageLocationRowActions
      location={makeStorageLocation({ id: 'location-1' })}
      onTest={noop}
      onDelete={noop}
    />)

    const menu = await openMenu(screen.getByRole('button', { name: 'More actions' }))
    const edit = within(menu).getByRole('menuitem', { name: 'Edit' })
    expect(edit.tagName).toBe('A')
    expect(edit.getAttribute('href')).toBe('/settings/storage/location-1/edit')
  })

  it('edits an LLM from its row with a link that still routes on a plain click', async () => {
    renderRouted(<LlmRowActions
      llm={makeLlm({ id: 'llm-1' })}
      onToggleActive={noop}
      onDelete={noop}
    />)

    const menu = await openMenu(screen.getByRole('button', { name: 'More actions' }))
    const edit = within(menu).getByRole('menuitem', { name: 'Edit' })
    expect(edit.tagName).toBe('A')
    expect(edit.getAttribute('href')).toBe('/settings/llms/llm-1/edit')

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
      '/settings/llms/add-claude-account',
      '/settings/llms/add-claude-api-key',
      '/settings/llms/add-claude-code-oauth',
      '/settings/llms/add-openai-api-key',
      '/settings/llms/add-codex-oauth',
    ])
  })
})
