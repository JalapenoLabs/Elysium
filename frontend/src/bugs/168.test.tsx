// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'

// Redux
import { createAppStore } from '../store'

// User interface
import { Link } from '@heroui/react'
import { ActionItemForm } from '../pages/ActionItems/ActionItemForm'
import { PromptComposer } from '../pages/Coding/PromptComposer'
import { EnvironmentVariableForm } from '../pages/Settings/Environment/EnvironmentVariableForm'
import { Page } from '../layout/page/Page'

// Misc
import { readSessionDraft } from '../drafts/sessionDrafts'
import { makeActionItem } from '../testFixtures'
import { TestAriaRouter } from '../testRouter'

// Issue #168: what is typed into a new form, a prompt, or a comment survives a reload of the
// tab. It comes back as unsaved changes, it ends once it is created or sent, Discard work in
// the leave prompt ends it too, and a secret is never written to storage.

vi.mock('../hooks/useServerData', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return Object.fromEntries(Object.keys(actual).map((name) => [ name, () => 'loaded' ]))
})

const routes = vi.hoisted(() => ({ createActionItem: vi.fn() }))
vi.mock('../api/routes/actionItemRoutes', async (importOriginal) => ({
  ...await importOriginal<object>(),
  createActionItem: routes.createActionItem,
}))

// jsdom has neither observer, which the page and HeroUI's fields use for layout alone.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

// A /new page with a link away from it, inside a data router for the leave prompt. Rendering
// it again after unmounting plays a reload of the tab, which keeps session storage.
function renderNewPage(form: ReactNode) {
  const router = createMemoryRouter([
    {
      path: '/new',
      element: <TestAriaRouter>
        <Page title='New' parents={[{ label: 'List', href: '/list' }]}>
          {form}
          <Link href='/elsewhere'>Elsewhere</Link>
        </Page>
      </TestAriaRouter>,
    },
    { path: '/elsewhere', element: <p>Somewhere else</p> },
    { path: '/list', element: <p>The list</p> },
  ], { initialEntries: [ '/new' ]})
  return render(<Provider store={createAppStore()}>
    <RouterProvider router={router} />
  </Provider>)
}

function newItemForm() {
  return <ActionItemForm initialProjectIds={[]} initialInitiativeIds={[]} onSaved={() => {}} />
}

function type(label: RegExp, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value }})
}

describe('issue #168', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.stubGlobal('IntersectionObserver', InertObserver)
    window.sessionStorage.clear()
    routes.createActionItem.mockReset()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('gives a new item\'s words back after a reload, as changes Save can keep', async () => {
    const first = renderNewPage(newItemForm())
    type(/Title/, 'Renew the certificate')
    type(/Notes/, 'Before Friday, or the API goes down.')
    first.unmount()

    renderNewPage(newItemForm())

    await waitFor(() => expect(screen.getByLabelText(/Title/)).toHaveProperty('value', 'Renew the certificate'))
    expect(screen.getByLabelText(/Notes/)).toHaveProperty('value', 'Before Friday, or the API goes down.')
    const [ headerSave ] = screen.getAllByRole('button', { name: 'Create item' })
    expect(headerSave.hasAttribute('disabled')).toBe(false)
  })

  it('ends the draft once the item is created', async () => {
    const created = makeActionItem({ id: 'item-1', title: 'Renew the certificate' })
    routes.createActionItem.mockResolvedValue({ item: created })
    renderNewPage(newItemForm())
    type(/Title/, 'Renew the certificate')
    expect(readSessionDraft('action-item')).toEqual({
      title: 'Renew the certificate',
      notes: '',
      priority: 'normal',
    })

    act(() => screen.getAllByRole('button', { name: 'Create item' })[0].click())

    await waitFor(() => expect(routes.createActionItem).toHaveBeenCalled())
    await waitFor(() => expect(readSessionDraft('action-item')).toBeNull())
  })

  it('ends the draft when leaving with Discard work, so it never comes back', async () => {
    renderNewPage(newItemForm())
    type(/Title/, 'Not worth it')

    act(() => screen.getByText('Elsewhere').click())
    act(() => screen.getByRole('button', { name: 'Discard work' }).click())

    expect(await screen.findByText('Somewhere else')).toBeTruthy()
    expect(readSessionDraft('action-item')).toBeNull()
  })

  it('never writes a secret value to storage', () => {
    renderNewPage(<EnvironmentVariableForm variable={null} onSaved={() => {}} />)
    type(/Key/, 'DEPLOY_TOKEN')
    act(() => screen.getByRole('switch').click())
    type(/Value/, 'hunter2')

    expect(readSessionDraft('environment-variable')).toEqual({ key: 'DEPLOY_TOKEN', description: '', isSecret: true })
    expect(JSON.stringify(window.sessionStorage)).not.toContain('hunter2')
  })

  it('gives a session\'s prompt back after a reload, and ends it once sent', async () => {
    const onSend = vi.fn(async () => true)
    const first = render(<PromptComposer id='coding-session-1-prompt' onSend={onSend} />)
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Fix the flaky test' }})
    first.unmount()

    const second = render(<PromptComposer id='coding-session-1-prompt' onSend={onSend} />)
    expect(screen.getByRole('textbox')).toHaveProperty('value', 'Fix the flaky test')

    act(() => second.container.querySelector<HTMLButtonElement>('button[type="submit"]')?.click())

    await waitFor(() => expect(onSend).toHaveBeenCalledWith('Fix the flaky test'))
    await waitFor(() => expect(screen.getByRole('textbox')).toHaveProperty('value', ''))
    expect(readSessionDraft('coding-session-1-prompt')).toBeNull()
  })
})
